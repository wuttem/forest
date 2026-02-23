use crate::models::TenantId;
use crate::timeseries::{LatLong, MetricValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum DataType {
    Float,
    Int,
    LocationObject,
    LocationTuple,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MetricConfig {
    pub json_pointer: String,
    pub name: String,
    pub data_type: DataType,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DataConfig {
    pub metrics: Vec<MetricConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DataConfigEntry {
    pub tenant_id: TenantId,
    pub device_prefix: Option<String>,
    pub metrics: Vec<MetricConfig>,
}

impl DataConfig {
    // Example merge logic: device config overwrites any tenant metrics with the same name
    pub fn merge_with(&self, other: &DataConfig) -> DataConfig {
        let mut merged = self.metrics.clone();
        for om in &other.metrics {
            if let Some(existing) = merged.iter_mut().find(|m| m.name == om.name) {
                *existing = om.clone();
            } else {
                merged.push(om.clone());
            }
        }
        DataConfig { metrics: merged }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap()
    }

    pub fn from_json(json: &str) -> DataConfig {
        serde_json::from_str(json).unwrap()
    }

    pub fn extract_metrics_from_json(&self, json_value: Value) -> Vec<(String, MetricValue)> {
        let mut metrics = Vec::new();
        for metric in &self.metrics {
            if let Some(value) = json_value.pointer(&metric.json_pointer) {
                // handle data types
                let value: Option<MetricValue> = match metric.data_type {
                    DataType::Float => value.as_f64().map(MetricValue::Float),
                    DataType::Int => {
                        // handle both i64 and f64 as int
                        let int = value.as_i64().or(value.as_f64().map(|f| f as i64));
                        int.map(MetricValue::Int)
                    }
                    DataType::LocationObject => {
                        let lat = value["lat"].as_f64();
                        let long = value["long"].as_f64();
                        if let (Some(lat), Some(long)) = (lat, long) {
                            Some(MetricValue::Location(LatLong::new(lat, long)))
                        } else {
                            None
                        }
                    }
                    DataType::LocationTuple => {
                        let lat = value[0].as_f64();
                        let long = value[1].as_f64();
                        if let (Some(lat), Some(long)) = (lat, long) {
                            Some(MetricValue::Location(LatLong::new(lat, long)))
                        } else {
                            None
                        }
                    }
                };
                if let Some(value) = value {
                    metrics.push((metric.name.clone(), value));
                }
            }
        }
        metrics
    }
}
