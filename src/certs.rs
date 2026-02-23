use openssl::asn1::{Asn1Integer, Asn1Time};
use openssl::bn::{BigNum, MsbOption};
use openssl::error::ErrorStack;
use openssl::hash::MessageDigest;
use openssl::nid::Nid;
use openssl::pkey::{PKey, Private};
use openssl::rsa::Rsa;
use openssl::x509::{X509, X509Builder, X509NameBuilder, X509ReqBuilder};
use openssl::x509::extension::{AuthorityKeyIdentifier, BasicConstraints, KeyUsage, SubjectAlternativeName, SubjectKeyIdentifier};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::ops::Add;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::collections::HashSet;
use thiserror::Error;
use serde::{Serialize, Deserialize};

pub const CA_CERT_FILENAME: &str = "ca.pem";
pub const CA_KEY_FILENAME: &str = "ca-key.pem";
pub const SERVER_CERT_FILENAME: &str = "server.pem";
pub const SERVER_KEY_FILENAME: &str = "server-key.pem";

#[derive(Error, Debug)]
pub enum CertificateError {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("OpenSSL error: {0}")]
    OpenSslError(#[from] openssl::error::ErrorStack),
    
    #[error("Certificate file not found: {0}")]
    FileNotFound(String),
    
    #[error("Invalid tenant ID: {0}")]
    InvalidTenantId(String),
    
    #[error("Certificate validation failed: {0}")]
    ValidationError(String),
    
    #[error("Missing certificate data: {0}")]
    MissingData(String),
    
    #[error("Certificate common name mismatch: expected '{expected}', found '{found}'")]
    CommonNameMismatch { expected: String, found: String },
    
    #[error("Missing hostnames in certificate: {0:?}")]
    MissingHostnames(Vec<String>),
    
    #[error("Certificate exists but is invalid: {0}")]
    InvalidCertificate(String),
}

// A type alias for our result type
pub type CertResult<T> = Result<T, CertificateError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct CertificateData {
    pub cert: String,
    pub key: String,
}

/// Certificate Manager for handling CA, server and client certificates
pub struct CertificateManager {
    cert_dir: PathBuf,
    tenant_id: Option<String>,
}

impl CertificateManager {
    /// Create a new certificate manager that stores certificates in the specified directory
    pub fn new<P: AsRef<Path>>(cert_dir: P, tenant_id: Option<String>) -> CertResult<Self> {
        let dir_path = cert_dir.as_ref().to_path_buf();
        
        // Validate tenant_id if provided
        if let Some(tenant) = &tenant_id {
            if !tenant.chars().all(|c| c.is_alphanumeric() || c == '-') {
                return Err(CertificateError::InvalidTenantId(
                    "Tenant ID must only contain alphanumeric characters and hyphens".to_string()
                ));
            }
        }

        let n = Self { cert_dir: dir_path, tenant_id };
        n.ensure_dirs_exist()?;
        Ok(n)
    }

    pub fn ensure_dirs_exist(&self) -> CertResult<()> {
        // Create the base directory
        if !self.cert_dir.exists() {
            fs::create_dir_all(&self.cert_dir)?;
        }
        
        // Create tenant directory if needed
        if let Some(tenant) = &self.tenant_id {
            let tenant_dir = self.cert_dir.join(tenant);
            if !tenant_dir.exists() {
                fs::create_dir_all(&tenant_dir)?;
            }
        }

        // Create base cacerts directory
        let cacerts_dir = self.cert_dir.join("cacerts");
        if !cacerts_dir.exists() {
            fs::create_dir_all(&cacerts_dir)?;
        }

        Ok(())
    }

    /// Create a new CertificateManager for a specific tenant, sharing the same base directory
    pub fn for_tenant(&self, tenant_id: String) -> CertResult<Self> {
        Self::new(self.cert_dir.clone(), Some(tenant_id))
    }

    /// Setup CA and server certificate with proper hostnames
    pub fn setup(&self, server_name: &str, host_names: &[&str]) -> CertResult<()> {
        // Ensure CA exists
        self.ensure_ca_exists()?;
        
        // Check if server certificate exists and is valid
        if !self.is_server_cert_valid(server_name, host_names)? {
            // Load existing server key if it exists, or create new one
            let server_key = if self.get_file_path(SERVER_KEY_FILENAME).exists() {
                self.load_private_key(SERVER_KEY_FILENAME)?
            } else {
                Self::generate_private_key()?
            };
            
            // Create server certificate with the key and hostnames
            self.create_server_cert_with_key(server_name, host_names, &server_key)?;
        }
        
        Ok(())
    }

    /// Generate an RSA private key with 2048 bits
    fn generate_private_key() -> Result<PKey<Private>, ErrorStack> {
        let rsa = Rsa::generate(2048)?;
        PKey::from_rsa(rsa)
    }

    /// Get the path to a certificate or key file
    fn get_file_path(&self, filename: &str) -> PathBuf {
        match &self.tenant_id {
            Some(tenant) => self.cert_dir.join(tenant).join(filename),
            None => self.cert_dir.join(filename),
        }
    }

    /// Get the path to a CA certificate
    pub fn get_ca_file_path(&self) -> PathBuf {
        let cacerts_dir = self.cert_dir.join("cacerts");
        match &self.tenant_id {
            Some(tenant) => cacerts_dir.join(format!("{}_ca.pem", tenant)),
            None => cacerts_dir.join(CA_CERT_FILENAME),
        }
    }

    /// Get the path to a CA key
    pub fn get_ca_key_path(&self) -> PathBuf {
        let cacerts_dir = self.cert_dir.join("cacerts");
        match &self.tenant_id {
            Some(tenant) => cacerts_dir.join(format!("{}_ca-key.pem", tenant)),
            None => cacerts_dir.join(CA_KEY_FILENAME),
        }
    }

    /// Get the organization name for certificates
    fn get_org_name(&self) -> String {
        match &self.tenant_id {
            Some(tenant) => tenant.clone(),
            None => "Forest".to_string(),
        }
    }

    /// Save private key to file and return the PEM content as a string
    fn save_private_key(&self, key: &PKey<Private>, filename: &str) -> CertResult<String> {
        let file_path = self.get_file_path(filename);
        self.save_private_key_absolute(key, &file_path)
    }

    /// Save private key to an absolute path and return the PEM content as a string
    fn save_private_key_absolute(&self, key: &PKey<Private>, file_path: &Path) -> CertResult<String> {
        let key_pem = key.private_key_to_pem_pkcs8()?;

        
        // Ensure directory exists
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        
        let mut file = File::create(file_path)?;
        file.write_all(&key_pem)?;
        
        // Convert to string to return
        let key_string = String::from_utf8(key_pem)
            .map_err(|_| CertificateError::ValidationError("Invalid UTF-8 in private key".to_string()))?;
        
        Ok(key_string)
    }

    /// Save certificate to file
    fn save_certificate(&self, cert: &X509, filename: &str) -> CertResult<String> {
        let file_path = self.get_file_path(filename);
        self.save_certificate_absolute(cert, &file_path)
    }

    /// Save certificate to an absolute path
    fn save_certificate_absolute(&self, cert: &X509, file_path: &Path) -> CertResult<String> {
        let cert_pem = cert.to_pem()?;

        
        // Ensure directory exists
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }
        
        let mut file = File::create(file_path)?;
        file.write_all(&cert_pem)?;

        // Convert to string to return
        let key_string = String::from_utf8(cert_pem)
            .map_err(|_| CertificateError::ValidationError("Invalid UTF-8 in certificate".to_string()))?;
        
        Ok(key_string)
    }

    /// Load private key from file
    fn load_private_key(&self, filename: &str) -> CertResult<PKey<Private>> {
        let path = self.get_file_path(filename);
        self.load_private_key_absolute(&path)
    }

    /// Load private key from an absolute path
    fn load_private_key_absolute(&self, path: &Path) -> CertResult<PKey<Private>> {
        if !path.exists() {
            return Err(CertificateError::FileNotFound(path.display().to_string()));
        }
        
        let mut file = File::open(&path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        
        PKey::private_key_from_pem(&contents)
            .map_err(|e| e.into())
    }

    /// Load certificate from file
    fn load_certificate(&self, filename: &str) -> CertResult<X509> {
        let path = self.get_file_path(filename);
        self.load_certificate_absolute(&path)
    }

    /// Load certificate from an absolute path
    fn load_certificate_absolute(&self, path: &Path) -> CertResult<X509> {
        if !path.exists() {
            return Err(CertificateError::FileNotFound(path.display().to_string()));
        }
        
        let mut file = File::open(&path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        
        X509::from_pem(&contents)
            .map_err(|e| e.into())
    }

    /// Check if CA exists
    pub fn ca_exists(&self) -> bool {
        let ca_cert_path = self.get_ca_file_path();
        let ca_key_path = self.get_ca_key_path();
        
        // Both files must exist for CA to be considered fully present
        ca_cert_path.exists() && ca_key_path.exists()
    }

    /// Create Certificate Authority (CA) if it doesn't exist
    pub fn ensure_ca_exists(&self) -> CertResult<()> {
        let ca_cert_path = self.get_ca_file_path();
        let ca_key_path = self.get_ca_key_path();
        
        // If both exist, we're good
        if ca_cert_path.exists() && ca_key_path.exists() {
            return Ok(());
        }
        
        // If only the key exists, load it and create a certificate with it
        if !ca_cert_path.exists() && ca_key_path.exists() {
            let ca_key = self.load_private_key_absolute(&ca_key_path)?;
            return self.create_ca(Some(&ca_key));
        }
        
        // Otherwise, create both key and certificate
        self.create_ca(None)
    }

    /// Retrieve the CA cert in PEM format
    pub fn get_ca_cert_pem(&self) -> CertResult<String> {
        let ca_cert_path = self.get_ca_file_path();
        if !ca_cert_path.exists() {
            return Err(CertificateError::FileNotFound(ca_cert_path.display().to_string()));
        }
        let mut file = File::open(&ca_cert_path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;
        Ok(contents)
    }

    /// Create a new Certificate Authority
    pub fn create_ca(&self, private_key: Option<&PKey<Private>>) -> CertResult<()> {
        // Use the provided key or generate a new one
        let ca_key = match private_key {
            Some(key) => key.clone(),
            None => Self::generate_private_key()?
        };
        
        // Create CA certificate
        let mut x509_name = X509NameBuilder::new()?;
        x509_name.append_entry_by_nid(Nid::COMMONNAME, "Forest CA")?;
        x509_name.append_entry_by_nid(Nid::ORGANIZATIONNAME, &self.get_org_name())?;
        let x509_name = x509_name.build();
        
        let mut cert_builder = X509Builder::new()?;
        cert_builder.set_version(2)?;
        
        // Generate random serial number
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        let serial = Asn1Integer::from_bn(&serial)?;
        cert_builder.set_serial_number(&serial)?;
        
        cert_builder.set_subject_name(&x509_name)?;
        cert_builder.set_issuer_name(&x509_name)?;
        
        // Certificate valid for 20 years
        let not_before = Asn1Time::from_unix(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64)?;
        
        let not_after = Asn1Time::from_unix(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .add(Duration::from_secs(20 * 365 * 24 * 60 * 60))
            .as_secs() as i64)?;
        
        cert_builder.set_not_before(&not_before)?;
        cert_builder.set_not_after(&not_after)?;
        
        cert_builder.set_pubkey(&ca_key)?;
        
        // Set CA extensions
        let basic_constraints = BasicConstraints::new().critical().ca().build()?;
        cert_builder.append_extension(basic_constraints)?;
        
        let key_usage = KeyUsage::new()
            .critical()
            .key_cert_sign()
            .crl_sign()
            .build()?;
        cert_builder.append_extension(key_usage)?;
        
        let subject_key_identifier = SubjectKeyIdentifier::new().build(&cert_builder.x509v3_context(None, None))?;
        cert_builder.append_extension(subject_key_identifier)?;
        
        // Self-sign the CA certificate with its private key
        cert_builder.sign(&ca_key, MessageDigest::sha256())?;
        let ca_cert = cert_builder.build();
        
        // Backup old CA if it exists
        if self.get_ca_file_path().exists() {
            let backup_path = self.get_ca_file_path().with_extension("pem.bak");
            fs::copy(self.get_ca_file_path(), backup_path).ok();
        }

        // Save the CA certificate and private key
        self.save_private_key_absolute(&ca_key, &self.get_ca_key_path())?;
        self.save_certificate_absolute(&ca_cert, &self.get_ca_file_path())?;
        
        Ok(())
    }

    /// Save a custom CA certificate and backup the old one if it exists
    pub fn save_custom_ca(&self, file_contents: &[u8]) -> CertResult<()> {
        let ca_cert_path = self.get_ca_file_path();
        
        // Backup old CA if it exists
        if ca_cert_path.exists() {
            let backup_path = ca_cert_path.with_extension("pem.bak");
            fs::copy(&ca_cert_path, backup_path).ok();
        }

        let cert = X509::from_pem(file_contents)?;
        self.save_certificate_absolute(&cert, &ca_cert_path)?;
        
        Ok(())
    }

    /// Create a client certificate signed by the CA
    pub fn create_client_cert(&self, client_name: &str) -> CertResult<CertificateData> {
        // Ensure CA exists
        self.ensure_ca_exists()?;
        
        // Load CA key and certificate
        let ca_key = self.load_private_key_absolute(&self.get_ca_key_path())?;
        let ca_cert = self.load_certificate_absolute(&self.get_ca_file_path())?;
        
        // Generate client private key
        let client_key = Self::generate_private_key()?;
        
        // Create client certificate request
        let mut req_builder = X509ReqBuilder::new()?;
        let mut x509_name = X509NameBuilder::new()?;
        x509_name.append_entry_by_nid(Nid::COMMONNAME, client_name)?;
        x509_name.append_entry_by_nid(Nid::ORGANIZATIONNAME, &self.get_org_name())?;
        let x509_name = x509_name.build();
        
        req_builder.set_subject_name(&x509_name)?;
        req_builder.set_pubkey(&client_key)?;
        req_builder.sign(&client_key, MessageDigest::sha256())?;
        let req = req_builder.build();
        
        // Create client certificate
        let mut cert_builder = X509Builder::new()?;
        cert_builder.set_version(2)?;
        
        // Generate random serial number
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        let serial = Asn1Integer::from_bn(&serial)?;
        cert_builder.set_serial_number(&serial)?;
        
        cert_builder.set_subject_name(req.subject_name())?;
        cert_builder.set_issuer_name(ca_cert.subject_name())?;
        
        // Certificate valid for 10 years
        let not_before = Asn1Time::from_unix(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64)?;
        
        let not_after = Asn1Time::from_unix(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .add(Duration::from_secs(10 * 365 * 24 * 60 * 60))
            .as_secs() as i64)?;
        
        cert_builder.set_not_before(&not_before)?;
        cert_builder.set_not_after(&not_after)?;
        
        cert_builder.set_pubkey(&client_key)?;
        
        // Set client certificate extensions
        let basic_constraints = BasicConstraints::new().build()?;
        cert_builder.append_extension(basic_constraints)?;
        
        let key_usage = KeyUsage::new()
            .digital_signature()
            .build()?;
        cert_builder.append_extension(key_usage)?;
        
        let subject_key_identifier = SubjectKeyIdentifier::new().build(&cert_builder.x509v3_context(None, None))?;
        cert_builder.append_extension(subject_key_identifier)?;
        
        let auth_key_identifier = AuthorityKeyIdentifier::new()
            .keyid(false)
            .issuer(false)
            .build(&cert_builder.x509v3_context(Some(&ca_cert), None))?;
        cert_builder.append_extension(auth_key_identifier)?;
        
        // Sign the client certificate with the CA key
        cert_builder.sign(&ca_key, MessageDigest::sha256())?;
        let client_cert = cert_builder.build();
        
        // Save the client certificate and private key
        let client_cert_filename = format!("{}-cert.pem", client_name);
        let client_key_filename = format!("{}-key.pem", client_name);
        
        let key = self.save_private_key(&client_key, &client_key_filename)?;
        let cert = self.save_certificate(&client_cert, &client_cert_filename)?;
        
        Ok(CertificateData { cert, key })
    }

    /// Check if server certificate exists and contains all required hostnames
    pub fn is_server_cert_valid(&self, server_name: &str, host_names: &[&str]) -> CertResult<bool> {
        // Check if certificate files exist
        let cert_path = self.get_file_path(SERVER_CERT_FILENAME);
        let key_path = self.get_file_path(SERVER_KEY_FILENAME);
        
        if !cert_path.exists() || !key_path.exists() {
            return Ok(false);
        }
        
        // Load server certificate
        let cert = self.load_certificate(SERVER_CERT_FILENAME)?;
        
        // Check if the common name matches
        let subject_name = cert.subject_name();
        let cn_entry = subject_name.entries_by_nid(Nid::COMMONNAME).next();
        
        let cn = match cn_entry {
            Some(entry) => {
                match entry.data().as_utf8() {
                    Ok(cn) => cn.to_string(),
                    Err(_) => return Err(CertificateError::InvalidCertificate("Common name is not valid UTF-8".to_string())),
                }
            },
            None => return Err(CertificateError::MissingData("Certificate is missing Common Name".to_string())),
        };
        
        if cn != server_name {
            return Ok(false);
        }
        
        // Convert required hostnames to a HashSet for efficient lookup
        let required_hostnames: HashSet<String> = host_names.iter().map(|&s| s.to_string()).collect();
        
        // Extract Subject Alternative Names from certificate
        let mut cert_hostnames = HashSet::new();
        
        // Get the SAN extension directly using subject_alt_names()
        if let Some(subject_alt_names) = cert.subject_alt_names() {
            for name in subject_alt_names {
                if let Some(dns_name) = name.dnsname() {
                    cert_hostnames.insert(dns_name.to_string());
                }
            }
        }
        
        // Find missing hostnames, if any
        let missing_hostnames: Vec<String> = required_hostnames
            .iter()
            .filter(|h| !cert_hostnames.contains(*h))
            .cloned()
            .collect();
        
        if !missing_hostnames.is_empty() {
            return Ok(false);
        }
        
        Ok(true)
    }

    /// Create a server certificate with provided key and hostnames
    fn create_server_cert_with_key(
        &self, 
        server_name: &str, 
        host_names: &[&str], 
        server_key: &PKey<Private>
    ) -> CertResult<()> {
        // Load CA key and certificate
        let ca_key = match self.load_private_key_absolute(&self.get_ca_key_path()) {
            Ok(key) => key,
            Err(e) => return Err(CertificateError::ValidationError(
                format!("Failed to load CA key: {}", e)
            )),
        };
        
        let ca_cert = match self.load_certificate_absolute(&self.get_ca_file_path()) {
            Ok(cert) => cert,
            Err(e) => return Err(CertificateError::ValidationError(
                format!("Failed to load CA certificate: {}", e)
            )),
        };
        
        // Create server certificate request
        let mut req_builder = X509ReqBuilder::new()?;
        let mut x509_name = X509NameBuilder::new()?;
        x509_name.append_entry_by_nid(Nid::COMMONNAME, server_name)?;
        x509_name.append_entry_by_nid(Nid::ORGANIZATIONNAME, &self.get_org_name())?;
        let x509_name = x509_name.build();
        
        req_builder.set_subject_name(&x509_name)?;
        req_builder.set_pubkey(server_key)?;
        req_builder.sign(server_key, MessageDigest::sha256())?;
        let req = req_builder.build();
        
        // Create server certificate
        let mut cert_builder = X509Builder::new()?;
        cert_builder.set_version(2)?;
        
        // Generate random serial number
        let mut serial = BigNum::new()?;
        serial.rand(159, MsbOption::MAYBE_ZERO, false)?;
        let serial = Asn1Integer::from_bn(&serial)?;
        cert_builder.set_serial_number(&serial)?;
        
        cert_builder.set_subject_name(req.subject_name())?;
        cert_builder.set_issuer_name(ca_cert.subject_name())?;
        
        // Certificate valid for 5 years
        let not_before = Asn1Time::from_unix(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64)?;
        
        let not_after = Asn1Time::from_unix(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .add(Duration::from_secs(5 * 365 * 24 * 60 * 60))
            .as_secs() as i64)?;
        
        cert_builder.set_not_before(&not_before)?;
        cert_builder.set_not_after(&not_after)?;
        
        cert_builder.set_pubkey(server_key)?;
        
        // Set server certificate extensions
        let basic_constraints = BasicConstraints::new().build()?;
        cert_builder.append_extension(basic_constraints)?;
        
        let key_usage = KeyUsage::new()
            .digital_signature()
            .key_encipherment()
            .build()?;
        cert_builder.append_extension(key_usage)?;
        
        // Add Subject Alternative Names (SAN) for all host names
        let ctx = cert_builder.x509v3_context(Some(&ca_cert), None);
        let mut subject_alt_name_builder = SubjectAlternativeName::new();
        
        // Add all host names as DNS entries in SAN
        for host in host_names {
            subject_alt_name_builder.dns(host);
        }
        
        // Always include the server_name if not in host_names
        if !host_names.contains(&server_name) {
            subject_alt_name_builder.dns(server_name);
        }
        
        let subject_alt_name = subject_alt_name_builder.build(&ctx)?;
        cert_builder.append_extension(subject_alt_name)?;
        
        let subject_key_identifier = SubjectKeyIdentifier::new().build(&cert_builder.x509v3_context(None, None))?;
        cert_builder.append_extension(subject_key_identifier)?;
        
        let auth_key_identifier = AuthorityKeyIdentifier::new()
            .keyid(false)
            .issuer(false)
            .build(&cert_builder.x509v3_context(Some(&ca_cert), None))?;
        cert_builder.append_extension(auth_key_identifier)?;
        
        // Sign the server certificate with the CA key
        cert_builder.sign(&ca_key, MessageDigest::sha256())?;
        let server_cert = cert_builder.build();
        
        // Save the server certificate and private key
        self.save_private_key(server_key, SERVER_KEY_FILENAME)?;
        self.save_certificate(&server_cert, SERVER_CERT_FILENAME)?;
        
        Ok(())
    }
    
    /// Create a server certificate signed by the CA with multiple host names
    pub fn create_server_cert(&self, server_name: &str) -> CertResult<()> {
        // Generate server private key
        let server_key = Self::generate_private_key()?;
        // Create with just the server_name as a hostname
        self.create_server_cert_with_key(server_name, &[server_name], &server_key)
    }
    
    /// Create a server certificate with multiple hostnames
    pub fn create_server_cert_with_hostnames(&self, server_name: &str, host_names: &[&str]) -> CertResult<()> {
        // Generate server private key
        let server_key = Self::generate_private_key()?;
        // Create with multiple hostnames
        self.create_server_cert_with_key(server_name, host_names, &server_key)
    }
}

#[cfg(test)]
mod tests;