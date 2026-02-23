use config::{Config, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::db::DatabaseConfig;
use crate::mqtt::MqttConfig;
use crate::processor::ProcessorConfig;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForestConfig {
    pub mqtt: MqttConfig,
    pub processor: ProcessorConfig,
    pub database: DatabaseConfig,
    pub bind_api: String,
    pub tenant_id: Option<String>,
    pub cert_dir: String,
    pub server_name: String,
    pub host_names: Vec<String>,
}

impl Default for ForestConfig {
    fn default() -> Self {
        Self {
            mqtt: MqttConfig::default(),
            processor: ProcessorConfig::default(),
            database: DatabaseConfig::default(),
            bind_api: String::from("127.0.0.1:8807"),
            tenant_id: None,
            cert_dir: "/etc/forest/certs".to_string(),
            server_name: String::from("localhost"),
            host_names: vec![String::from("localhost"), String::from("127.0.0.1")],
        }
    }
}

impl ForestConfig {
    pub fn new(config_path: Option<&Path>) -> Result<Self, ConfigError> {
        // Stupid way to set the defaults
        let default_config = ForestConfig::default();

        let mut builder = Config::builder()
            // Start with default values
            .set_default("mqtt.bind_v3", default_config.mqtt.bind_v3)?
            .set_default("mqtt.bind_v5", default_config.mqtt.bind_v5)?
            .set_default(
                "mqtt.enable_heartbeat",
                default_config.mqtt.enable_heartbeat,
            )?
            .set_default("mqtt.enable_ssl", default_config.mqtt.enable_ssl)?
            .set_default(
                "mqtt.max_connections",
                default_config.mqtt.max_connections as u64,
            )?
            .set_default(
                "processor.shadow_topic_prefix",
                default_config.processor.shadow_topic_prefix,
            )?
            .set_default(
                "processor.telemetry_topics",
                default_config.processor.telemetry_topics,
            )?
            .set_default(
                "database.create_if_missing",
                default_config.database.create_if_missing,
            )?
            .set_default("database.path", default_config.database.path)?
            .set_default(
                "database.timeseries_path",
                default_config.database.timeseries_path,
            )?
            .set_default("bind_api", default_config.bind_api)?
            .set_default("tenant_id", default_config.tenant_id)?
            // .set_default("cert_dir", default_config.cert_dir)?
            .set_default("server_name", default_config.server_name)?
            .set_default("host_names", default_config.host_names)?
            // Add in settings from environment variables (with prefix "FOREST_")
            .add_source(Environment::with_prefix("FOREST").separator("__"));

        // If a config file was provided, add it as a source
        if let Some(path) = config_path {
            builder = builder.add_source(File::from(path));
        }

        // Build the config
        let raw_config = builder.build()?;

        // Try to deserialize the config into our ForestConfig struct
        let mut config: Result<Self, ConfigError> = raw_config.try_deserialize();

        // If we have ssl_cert_dir and dont have ssl paths for mqtt, we need to set them
        if let Ok(ref mut forest_config) = config {
            let cert_dir = forest_config.cert_dir.clone();
            let cert_dir = cert_dir.trim_end_matches('/');

            if forest_config.mqtt.ssl_cert_path.is_none() {
                forest_config.mqtt.ssl_cert_path = Some(format!("{}/server.pem", cert_dir));
            }
            if forest_config.mqtt.ssl_key_path.is_none() {
                forest_config.mqtt.ssl_key_path = Some(format!("{}/server-key.pem", cert_dir));
            }
            if forest_config.mqtt.ssl_ca_path.is_none() {
                forest_config.mqtt.ssl_ca_path = Some(format!("{}/cacerts", cert_dir));
            }
        }

        config
    }
}
