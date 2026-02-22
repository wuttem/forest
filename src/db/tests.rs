use super::*;
use crate::dataconfig::{DataConfig, DataType, MetricConfig};
use crate::shadow::StateDocument;
use crate::timeseries::FloatTimeSeries;
use serde_json::{json, Value};
use tempfile::TempDir;
use uuid::Uuid;

async fn setup_db() -> (DB, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let mut config = DatabaseConfig::default();
    let db_id = Uuid::new_v4().simple();
    config.path = format!("sqlite:file:memdb_{}?mode=memory&cache=shared", db_id);

    let db = DB::open(&config).await.unwrap();
    (db, temp_dir)
}

#[tokio::test]
async fn test_put_get_multiple_buckets() {
    let (db, _temp) = setup_db().await;
    let mut ts = FloatTimeSeries::new();
    // Two hours of data
    ts.add_point(1710511200, 1.0); // Hour 1
    ts.add_point(1710511200 + 3600, 2.0); // Hour 2

    let key = b"test2";
    db._put_timeseries(key, &MetricTimeSeries::from(&ts))
        .await
        .unwrap();

    // Query first hour only
    let result = db
        ._get_timeseries(key, 1710511200, 1710511200 + 3599)
        .await
        .unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(
        *result.get_value_for_timestamp(1710511200).unwrap(),
        MetricValue::Float(1.0)
    );
}

#[tokio::test]
async fn test_no_db_connection_ts() {
    let db = DB {
        path: String::from("path"),
        pool: None,
    };
    let ts = FloatTimeSeries::new();

    assert!(matches!(
        db._put_timeseries(b"key", &MetricTimeSeries::from(&ts)).await,
        Err(DatabaseError::DatabaseConnectionError)
    ));

    assert!(matches!(
        db._get_timeseries(b"key", 0, 1).await,
        Err(DatabaseError::DatabaseConnectionError)
    ));
}

#[tokio::test]
async fn test_empty_range() {
    let (db, _temp) = setup_db().await;
    let result = db._get_timeseries(b"nonexistent", 0, 1).await.unwrap();
    assert_eq!(result.len(), 0);
}

// Test `test_ts_key_ordering` removed as it is RocksDB specific //

#[tokio::test]
async fn test_get_timeseries_last() {
    let (db, _temp) = setup_db().await;

    // Create test data with multiple timestamps
    let mut ts1 = FloatTimeSeries::new();
    ts1.add_point(1710511200, 1.0); // 14:00
    ts1.add_point(1710511200 + 1800, 2.0); // 14:30

    let mut ts2 = FloatTimeSeries::new();
    ts2.add_point(1710514800, 3.0); // 15:00
    ts2.add_point(1710514800 + 1800, 4.0); // 15:30

    let key = b"test_last";

    // Store both timeseries
    db._put_timeseries(key, &MetricTimeSeries::from(&ts1))
        .await
        .unwrap();
    db._put_timeseries(key, &MetricTimeSeries::from(&ts2))
        .await
        .unwrap();

    // Test getting last point
    let result = db._get_timeseries_last(key, 1).await.unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(
        *result.get_value_for_timestamp(1710514800 + 1800).unwrap(),
        MetricValue::Float(4.0)
    );

    // Test getting more points than available
    let result = db._get_timeseries_last(key, 10).await.unwrap();
    assert_eq!(result.len(), 4);

    // Test empty key
    let result = db._get_timeseries_last(b"nonexistent", 1).await.unwrap();
    assert_eq!(result.len(), 0);

    // Test no DB connection
    let db_no_conn = DB {
        path: "path".to_string(),
        pool: None,
    };
    assert!(matches!(
        db_no_conn._get_timeseries_last(key, 1).await,
        Err(DatabaseError::DatabaseConnectionError)
    ));
}

#[tokio::test]
async fn test_upsert_shadow() {
    let (db, _temp) = setup_db().await;

    // Create initial update
    let update1 = StateUpdateDocument {
        device_id: "thermostat-01".to_string(),
        shadow_name: ShadowName::Default,
        tenant_id: TenantId::Default,
        state: StateDocument {
            reported: json!({
                "temperature": 22.5,
                "humidity": 45
            }),
            desired: Value::Null,
            delta: Value::Null,
        },
    };

    // Test initial insert
    db._upsert_shadow(&update1).await.unwrap();

    let shadow = db._get_shadow("thermostat-01", &ShadowName::Default, &TenantId::Default).await.unwrap();

    assert_eq!(shadow.device_id, "thermostat-01");
    assert_eq!(shadow.get_reported_value()["temperature"], 22.5);

    // Test update existing shadow
    let update2 = StateUpdateDocument {
        device_id: "thermostat-01".to_string(),
        shadow_name: ShadowName::Default,
        tenant_id: TenantId::Default,
        state: StateDocument {
            reported: Value::Null,
            desired: json!({
                "temperature": 21.0,
            }),
            delta: Value::Null,
        },
    };

    db._upsert_shadow(&update2).await.unwrap();

    // Verify shadow was updated
    let shadow = db._get_shadow("thermostat-01", &ShadowName::Default, &TenantId::Default).await.unwrap();
    let desired = shadow.get_desired_value();
    let reported = shadow.get_reported_value();
    assert_eq!(desired["temperature"], 21.0);
    assert_eq!(reported["temperature"], 22.5);
    let s = shadow.get_delta_value();
    assert_eq!(s["temperature"], 21.0);

    // Make another update to reset delta
    let update3 = StateUpdateDocument {
        device_id: "thermostat-01".to_string(),
        shadow_name: ShadowName::Default,
        tenant_id: TenantId::Default,
        state: StateDocument {
            reported: json!({
                "temperature": 21.0
            }),
            desired: Value::Null,
            delta: Value::Null,
        },
    };
    db._upsert_shadow(&update3).await.unwrap();

    let shadow = db._get_shadow("thermostat-01", &ShadowName::Default, &TenantId::Default).await.unwrap();
    let desired = shadow.get_desired_value();
    let reported = shadow.get_reported_value();
    assert_eq!(desired["temperature"], 21.0);
    assert_eq!(reported["temperature"], 21.0);
    assert_eq!(*shadow.get_delta_value(), Value::Null);

    let store_shadow = db
        ._get_shadow("thermostat-01", &ShadowName::Default, &TenantId::Default)
        .await
        .unwrap();
    let desired = shadow.get_desired_value();
    let reported = shadow.get_reported_value();
    assert_eq!(store_shadow.device_id, "thermostat-01");
    assert_eq!(reported["temperature"], 21.0);
    assert_eq!(desired["temperature"], 21.0);
    assert_eq!(*store_shadow.get_delta_value(), Value::Null);
}

#[tokio::test]
async fn test_store_and_get_tenant_data_config() {
    let (db, _temp) = setup_db().await;

    let config = DataConfig {
        metrics: vec![
            MetricConfig {
                json_pointer: "/temperature".to_string(),
                name: "temperature".to_string(),
                data_type: DataType::Float,
            },
            MetricConfig {
                json_pointer: "/temperature".to_string(),
                name: "humidity".to_string(),
                data_type: DataType::Int,
            },
        ],
    };

    db.store_tenant_data_config(&TenantId::Default, &config).await.unwrap();
    let actual = db.get_data_config(&TenantId::Default, None).await.unwrap().unwrap();
    assert_eq!(actual.metrics.len(), 2);
}

#[tokio::test]
async fn test_store_and_get_device_data_config() {
    let (db, _temp) = setup_db().await;

    let tenant_config = DataConfig {
        metrics: vec![MetricConfig {
            json_pointer: "/temperature".to_string(),
            name: "temperature".to_string(),
            data_type: DataType::Float,
        }],
    };
    db.store_tenant_data_config(&TenantId::new("tenant2"), &tenant_config)
        .await
        .unwrap();

    let base = db
        .get_data_config(&TenantId::new("tenant2"), Some("deviceA1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(base.metrics.len(), 1);
    assert_eq!(base.metrics[0].name, "temperature");
    assert_eq!(base.metrics[0].data_type, DataType::Float);

    let device_config = DataConfig {
        metrics: vec![MetricConfig {
            json_pointer: "/temperature".to_string(),
            name: "temperature".to_string(),
            data_type: DataType::Int, // override
        }],
    };
    db.store_device_data_config(&TenantId::new("tenant2"), "deviceA", &device_config)
        .await
        .unwrap();

    // Should merge tenant config + device config
    let merged = db
        .get_data_config(&TenantId::new("tenant2"), Some("deviceA1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(merged.metrics.len(), 1);
    assert_eq!(merged.metrics[0].name, "temperature");
    assert_eq!(merged.metrics[0].data_type, DataType::Int);

    let device_config = DataConfig {
        metrics: vec![MetricConfig {
            json_pointer: "/temp3".to_string(),
            name: "temp2".to_string(),
            data_type: DataType::Float,
        }],
    };
    db.store_device_data_config(&TenantId::new("tenant2"), "deviceA1", &device_config)
        .await
        .unwrap();

    let merged = db
        .get_data_config(&TenantId::new("tenant2"), Some("deviceA1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(merged.metrics.len(), 2);
    assert_eq!(merged.metrics[0].name, "temperature");
    assert_eq!(merged.metrics[0].data_type, DataType::Float);
    assert_eq!(merged.metrics[1].name, "temp2");
    assert_eq!(merged.metrics[1].data_type, DataType::Float);
}

#[tokio::test]
async fn test_delete_data_config() {
    let (db, _temp) = setup_db().await;

    // Setup test data
    let tenant_config = DataConfig {
        metrics: vec![MetricConfig {
            json_pointer: "/temperature".to_string(),
            name: "temperature".to_string(),
            data_type: DataType::Float,
        }],
    };
    let device_config = DataConfig {
        metrics: vec![MetricConfig {
            json_pointer: "/humidity".to_string(),
            name: "humidity".to_string(),
            data_type: DataType::Int,
        }],
    };

    // Store configs
    db.store_tenant_data_config(&TenantId::new("tenant1"), &tenant_config)
        .await
        .unwrap();
    db.store_device_data_config(&TenantId::new("tenant1"), "device1", &device_config)
        .await
        .unwrap();

    // Delete device config
    db.delete_data_config(&TenantId::new("tenant1"), Some("device1")).await.unwrap();

    // Verify device config is gone but tenant config remains
    let result = db
        .get_data_config(&TenantId::new("tenant1"), Some("device1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.metrics.len(), 1);
    assert_eq!(result.metrics[0].name, "temperature");

    // Delete tenant config
    db.delete_data_config(&TenantId::new("tenant1"), None).await.unwrap();

    // Verify tenant config is gone
    let result = db.get_data_config(&TenantId::new("tenant1"), None).await.unwrap();
    assert!(matches!(result, None));
}

#[tokio::test]
async fn test_list_data_configs() {
    let (db, _temp) = setup_db().await;

    // Setup test data
    let tenant_config = DataConfig {
        metrics: vec![MetricConfig {
            json_pointer: "/temperature".to_string(),
            name: "temperature".to_string(),
            data_type: DataType::Float,
        }],
    };
    let device1_config = DataConfig {
        metrics: vec![MetricConfig {
            json_pointer: "/humidity".to_string(),
            name: "humidity".to_string(),
            data_type: DataType::Int,
        }],
    };
    let device2_config = DataConfig {
        metrics: vec![MetricConfig {
            json_pointer: "/pressure".to_string(),
            name: "pressure".to_string(),
            data_type: DataType::Float,
        }],
    };

    // Store configs
    db.store_tenant_data_config(&TenantId::new("tenant1"), &tenant_config)
        .await
        .unwrap();
    db.store_device_data_config(&TenantId::new("tenant1"), "device1", &device1_config)
        .await
        .unwrap();
    db.store_device_data_config(&TenantId::new("tenant1"), "device2", &device2_config)
        .await
        .unwrap();

    // List configs
    let configs = db.list_data_configs(&TenantId::new("tenant1")).await.unwrap();

    // Verify number of configs
    assert_eq!(configs.len(), 3);

    // Verify tenant config
    let tenant_key = TenantId::new("tenant1");
    let tenant_entry = configs
        .iter()
        .find(|entry| entry.tenant_id == tenant_key)
        .unwrap();
    assert_eq!(tenant_entry.metrics[0].name, "temperature");

    // Verify device configs
    let device1_key = Some("device1".to_string());
    let device1_entry = configs
        .iter()
        .find(|entry| entry.device_prefix == device1_key)
        .unwrap();
    assert_eq!(device1_entry.metrics[0].name, "humidity");

    let device2_key = Some("device2".to_string());
    let device2_entry = configs
        .iter()
        .find(|entry| entry.device_prefix == device2_key)
        .unwrap();
    assert_eq!(device2_entry.metrics[0].name, "pressure");

    // Verify empty list for non-existent tenant
    let empty_configs = db.list_data_configs(&TenantId::new("tenant2")).await.unwrap();
    assert_eq!(empty_configs.len(), 0);
}
