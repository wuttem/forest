use serde::{Deserialize, Serialize};
use std::fmt::Display;

#[derive(Debug, Clone, PartialEq)]
pub enum DefaultString {
    Default,
    Custom(String),
}

impl Serialize for DefaultString {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            DefaultString::Default => serializer.serialize_str("default"),
            DefaultString::Custom(name) => serializer.serialize_str(name),
        }
    }
}

impl<'de> Deserialize<'de> for DefaultString {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(if s.to_lowercase() == "default" {
            DefaultString::Default
        } else {
            DefaultString::Custom(s)
        })
    }
}

impl Display for DefaultString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DefaultString::Default => write!(f, "default"),
            DefaultString::Custom(name) => write!(f, "{}", name),
        }
    }
}

impl DefaultString {
    pub fn new(name: &str) -> Self {
        if name.to_lowercase() == "default" {
            DefaultString::Default
        } else {
            DefaultString::Custom(name.to_string())
        }
    }

    pub fn from_str(s: &str) -> Self {
        Self::new(s)
    }

    pub fn from_option(s: Option<&str>) -> Self {
        match s {
            Some(s) => Self::new(s),
            None => DefaultString::Default,
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            DefaultString::Default => "default",
            DefaultString::Custom(name) => name,
        }
    }
}

pub type ShadowName = DefaultString;
pub type TenantId = DefaultString;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub allow_passwords: bool,
    pub allow_certificates: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            allow_passwords: false,
            allow_certificates: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tenant {
    pub tenant_id: TenantId,
    pub auth_config: AuthConfig,
    pub created_at: u64,
}

impl Tenant {
    pub fn new(tenant_id: &TenantId) -> Self {
        Self {
            tenant_id: tenant_id.clone(),
            auth_config: AuthConfig::default(),
            created_at: chrono::Utc::now().timestamp() as u64,
        }
    }

    pub fn with_auth_config(mut self, auth_config: AuthConfig) -> Self {
        self.auth_config = auth_config;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCredential {
    pub tenant_id: TenantId,
    pub device_id: String,
    pub username: String,
    pub password_hash: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceMetadata {
    pub device_id: String,
    pub tenant_id: TenantId,
    pub certificate: Option<String>,
    pub key: Option<String>,
    pub created_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinuteRate {
    pub timestamp: u64,
    pub mqtt_message_rate_in: u32,
}

// Add this struct to your models.rs file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInformation {
    pub device_id: String,
    pub tenant_id: TenantId,
    pub certificate: Option<String>,
    pub connected: bool,
    pub past_minute_rates: Option<Vec<MinuteRate>>,
    pub last_shadow_update: Option<u64>,
}

impl DeviceMetadata {
    pub fn new(device_id: &str, tenant_id: &TenantId) -> Self {
        Self {
            device_id: device_id.to_string(),
            tenant_id: tenant_id.to_owned(),
            certificate: None,
            key: None,
            created_at: chrono::Utc::now().timestamp() as u64,
        }
    }

    pub fn with_credentials(mut self, certificate: String, key: String) -> Self {
        self.certificate = Some(certificate);
        self.key = Some(key);
        self
    }
}
