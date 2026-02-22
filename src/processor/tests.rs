use super::*;
use crate::db::DB;
use crate::mqtt::{start_broker, MqttServer};
use tempfile::TempDir;

async fn setup_db() -> Arc<DB> {
    let _temp_dir = TempDir::new().unwrap();
    let db_id = uuid::Uuid::new_v4().simple();
    Arc::new(DB::open_default(&format!("sqlite:file:memdb_{}?mode=memory&cache=shared", db_id)).await.unwrap())
}

async fn setup_mqtt() -> MqttServer {
    start_broker(None).await
}

#[tokio::test]
async fn test_start_processor() {
    let db = setup_db().await;
    let mut mqtt = setup_mqtt().await;
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
    let processor = result.unwrap();
    assert!(processor.db.pool.is_some(), "DB should be open");
}
