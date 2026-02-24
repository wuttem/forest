use serde::{Deserialize, Serialize};

pub const DEFAULT_CONFIG: &str = r#"{
  "id": 0,
  "metrics": {
    "alerts": {
      "push_interval": 10
    },
    "meters": {
      "push_interval": 10
    }
  },
  "router": {
    "id": 0,
    "dir": "/tmp/rumqttd",
    "max_connections": 10000,
    "max_outgoing_packet_count": 200,
    "max_segment_size": 104857600,
    "max_segment_count": 10,
    "default_lower_rate": 60.0,
    "default_higher_rate": 6000.0,
    "congestion_threshold": 0.8
  },
  "v4": {
    "1": {
      "listen": "127.0.0.1:1883",
      "name": "v4-1",
      "next_connection_delay_ms": 1,
      "connections": {
        "connection_timeout_ms": 30000,
        "dynamic_filters": true,
        "max_inflight_count": 500,
        "max_payload_size": 128000
      }
    }
  },
  "v5": {
    "1": {
      "listen": "127.0.0.1:1884",
      "name": "v5-1",
      "next_connection_delay_ms": 1,
      "connections": {
        "connection_timeout_ms": 30000,
        "max_inflight_count": 500,
        "max_payload_size": 128000
      }
    }
  },
  "ws": {
    "1": {
      "listen": "127.0.0.1:1885",
      "name": "ws-1",
      "next_connection_delay_ms": 1,
      "connections": {
        "connection_timeout_ms": 30000,
        "max_inflight_count": 500,
        "max_payload_size": 128000
      }
    }
  }
}"#;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MqttConfig {
    pub enable_heartbeat: bool,
    pub enable_ssl: bool,
    pub ssl_ca_path: Option<String>,
    pub ssl_cert_path: Option<String>,
    pub ssl_key_path: Option<String>,
    pub max_connections: usize,
    pub bind_v3: String,
    pub bind_v5: String,
    pub bind_ws: Option<String>,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            enable_heartbeat: true,
            enable_ssl: false,
            ssl_ca_path: None,
            ssl_cert_path: None,
            ssl_key_path: None,
            max_connections: 10000,
            bind_v3: "127.0.0.1:1883".to_string(),
            bind_v5: "127.0.0.1:1884".to_string(),
            bind_ws: None,
        }
    }
}

pub fn get_default_config() -> rumqttd::Config {
    let config = serde_json::from_str(DEFAULT_CONFIG).unwrap();
    return config;
}
