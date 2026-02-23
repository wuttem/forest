use super::*;
use tempfile::tempdir;

#[test]
fn test_create_ca() {
    let temp_dir = tempdir().unwrap();
    let cert_manager = CertificateManager::new(&temp_dir, None).unwrap();

    assert!(!cert_manager.ca_exists());

    // Create CA
    cert_manager.create_ca(None).unwrap();

    // Check CA files exist
    assert!(cert_manager.ca_exists());
    assert!(cert_manager.get_ca_file_path().exists());
    assert!(cert_manager.get_ca_key_path().exists());
}

#[test]
fn test_create_tenant_ca() {
    let temp_dir = tempdir().unwrap();
    let tenant_id = "test-tenant".to_string();
    let cert_manager = CertificateManager::new(&temp_dir, Some(tenant_id.clone())).unwrap();

    assert!(!cert_manager.ca_exists());

    // Create CA
    cert_manager.create_ca(None).unwrap();

    // Check CA files exist in tenant directory
    assert!(cert_manager.ca_exists());
    assert!(cert_manager.get_ca_file_path().exists());
    assert!(cert_manager.get_ca_key_path().exists());
}

#[test]
fn test_create_server_cert() {
    let temp_dir = tempdir().unwrap();
    let cert_manager = CertificateManager::new(&temp_dir, None).unwrap();

    cert_manager.ensure_ca_exists().unwrap();
    // Create server cert (should create CA first)
    cert_manager.create_server_cert("example.com").unwrap();

    // Check files exist
    assert!(cert_manager.get_ca_file_path().exists());
    assert!(cert_manager.get_ca_key_path().exists());
    assert!(temp_dir.path().join(SERVER_CERT_FILENAME).exists());
    assert!(temp_dir.path().join(SERVER_KEY_FILENAME).exists());
}

#[test]
fn test_create_client_cert() {
    let temp_dir = tempdir().unwrap();
    let cert_manager = CertificateManager::new(&temp_dir, None).unwrap();

    // Create client cert (should create CA first)
    cert_manager.create_client_cert("client1").unwrap();

    // Check files exist
    assert!(cert_manager.get_ca_file_path().exists());
    assert!(cert_manager.get_ca_key_path().exists());
    assert!(temp_dir.path().join("client1-cert.pem").exists());
    assert!(temp_dir.path().join("client1-key.pem").exists());
}

#[test]
fn test_invalid_tenant_id() {
    let temp_dir = tempdir().unwrap();
    let result = CertificateManager::new(&temp_dir, Some("invalid tenant@id".to_string()));

    assert!(result.is_err());
    match result {
        Err(CertificateError::InvalidTenantId(_)) => {} // Expected error
        _ => panic!("Expected InvalidTenantId error"),
    }
}

#[test]
fn test_setup_creates_new_cert() {
    let temp_dir = tempdir().unwrap();
    let cert_manager = CertificateManager::new(&temp_dir, None).unwrap();

    // Setup with server name and hostnames
    cert_manager
        .setup("example.com", &["example.com", "www.example.com"])
        .unwrap();

    // Check that files exist
    assert!(cert_manager.get_ca_file_path().exists());
    assert!(cert_manager.get_ca_key_path().exists());
    assert!(temp_dir.path().join(SERVER_CERT_FILENAME).exists());
    assert!(temp_dir.path().join(SERVER_KEY_FILENAME).exists());

    // Verify that the certificate is valid for both hostnames
    assert!(cert_manager
        .is_server_cert_valid("example.com", &["example.com", "www.example.com"])
        .unwrap());
}

#[test]
fn test_setup_reuses_existing_key() {
    let temp_dir = tempdir().unwrap();
    let cert_manager = CertificateManager::new(&temp_dir, None).unwrap();

    cert_manager.ensure_ca_exists().unwrap();

    // First create a server cert with just one hostname
    cert_manager
        .create_server_cert_with_hostnames("example.com", &["example.com"])
        .unwrap();

    // Read the contents of the key file
    let key_path = temp_dir.path().join(SERVER_KEY_FILENAME);
    let original_key_content = fs::read_to_string(&key_path).unwrap();

    // Now call setup with an additional hostname
    cert_manager
        .setup("example.com", &["example.com", "www.example.com"])
        .unwrap();

    // The key file should not have changed (same contents)
    let new_key_content = fs::read_to_string(&key_path).unwrap();
    assert_eq!(original_key_content, new_key_content);

    // But the certificate should now include both hostnames
    assert!(cert_manager
        .is_server_cert_valid("example.com", &["example.com", "www.example.com"])
        .unwrap());
}

#[test]
fn test_is_server_cert_valid() {
    let temp_dir = tempdir().unwrap();
    let cert_manager = CertificateManager::new(&temp_dir, None).unwrap();

    cert_manager.ensure_ca_exists().unwrap();

    // Create a certificate with specific hostnames
    cert_manager
        .create_server_cert_with_hostnames("example.com", &["example.com", "www.example.com"])
        .unwrap();

    // Test valid cases
    assert!(cert_manager
        .is_server_cert_valid("example.com", &["example.com"])
        .unwrap());
    assert!(cert_manager
        .is_server_cert_valid("example.com", &["www.example.com"])
        .unwrap());
    assert!(cert_manager
        .is_server_cert_valid("example.com", &["example.com", "www.example.com"])
        .unwrap());

    // Test invalid cases
    assert!(!cert_manager
        .is_server_cert_valid("example.com", &["example.com", "another.example.com"])
        .unwrap());
    assert!(!cert_manager
        .is_server_cert_valid("wrong.com", &["example.com"])
        .unwrap());
}
