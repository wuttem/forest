use super::*;
use crate::db::DB;
use crate::mqtt::{config::MqttConfig, start_broker, MqttServer};
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;

fn get_unique_test_config() -> Option<MqttConfig> {
    let mut config = MqttConfig::default();
    config.bind_v3 = "127.0.0.1:0".to_string();
    config.bind_v5 = "127.0.0.1:0".to_string();
    config.bind_ws = None;
    Some(config)
}

async fn setup_db() -> Arc<DB> {
    let _temp_dir = TempDir::new().unwrap();
    let db_id = uuid::Uuid::new_v4().simple();
    Arc::new(
        DB::open_default(&format!(
            "sqlite:file:memdb_{}?mode=memory&cache=shared",
            db_id
        ))
        .await
        .unwrap(),
    )
}

async fn setup_mqtt(db: Arc<DB>) -> MqttServer {
    start_broker(get_unique_test_config(), db).await
}

#[tokio::test]
async fn test_start_processor() {
    let db = setup_db().await;
    let mut mqtt = setup_mqtt(db.clone()).await;
    let sender = mqtt.mqtt.clone();
    let admin = mqtt.admin.take().unwrap();
    let conn_mon_rx = mqtt.connection_monitor_subscribe();
    let connected_clients = Arc::new(ConnectionSet::new());
    let processor_config = ProcessorConfig::default();
    let result = start_processor(
        db,
        sender,
        admin,
        conn_mon_rx,
        connected_clients,
        processor_config,
    )
    .await;
    assert!(result.is_ok(), "start_processor should return Ok");
    let (processor, _handle) = result.unwrap();
    assert!(processor.db.pool.is_some(), "DB should be open");
}

#[tokio::test]
async fn test_time_request() {
    let _ = tracing_subscriber::fmt().with_test_writer().try_init();
    let db = setup_db().await;
    let mut mqtt = setup_mqtt(db.clone()).await;
    let sender = mqtt.mqtt.clone();
    let admin = mqtt.admin.take().unwrap();
    let conn_mon_rx = mqtt.connection_monitor_subscribe();
    let connected_clients = Arc::new(ConnectionSet::new());
    let processor_config = ProcessorConfig::default();

    let receiver = mqtt.message_receiver();
    let (_processor, _handle) = start_processor(
        db,
        sender.clone(),
        admin,
        conn_mon_rx,
        connected_clients,
        processor_config,
    )
    .await
    .unwrap();

    sender
        .subscribe("things/device1/time/response".to_string())
        .await
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let payload = r#"{"device_time": 12345}"#.as_bytes().to_vec();
    sender
        .publish("things/device1/time/request".to_string(), payload)
        .unwrap();

    let mut resp_msg = None;
    for _ in 0..5 {
        let msg = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv_async())
            .await
            .expect("Timeout waiting for message")
            .expect("Channel closed");
        if msg.topic == "things/device1/time/response" {
            resp_msg = Some(msg);
            break;
        }
    }
    let msg = resp_msg.expect("Did not receive response");

    assert_eq!(msg.topic, "things/device1/time/response");
    let resp: serde_json::Value = serde_json::from_slice(&msg.payload).unwrap();
    assert_eq!(resp["device_time"], 12345);
    assert!(resp.get("server_time").is_some());

    // Crucial: shutdown mqtt broker to prevent background thread from hanging test runner
    mqtt.shutdown();
}
