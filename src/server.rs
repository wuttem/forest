use tokio_util::sync::CancellationToken;
use tracing::warn;

use crate::api::start_api_server;
use crate::config::ForestConfig;
use crate::db::DB;
use crate::mqtt::start_broker;
use crate::processor::start_processor;

use std::path::PathBuf;
use std::sync::Arc;

pub type ConnectionSet = dashmap::DashSet<String>;

pub async fn start_server(config: &ForestConfig) -> CancellationToken {
    let db_path = PathBuf::from(&config.database.path);

    let maybe_db = DB::open_default(db_path.to_str().unwrap()).await;
    let db = {
        match maybe_db {
            Ok(db) => Arc::new(db),
            Err(e) => {
                panic!("Failed to open DB: {:?}", e);
            }
        }
    };

    let connected_clients = Arc::new(ConnectionSet::new());

    let mut mqtt_broker = start_broker(Some(config.mqtt.clone())).await;
    let _broker_cancel_token = mqtt_broker.cancel_token.clone();
    let mqtt_sender = mqtt_broker.mqtt.clone();
    let mqtt_admin = mqtt_broker.admin.take().unwrap(); // Move admin out of MqttServer
    let processor_db = db.clone();
    let connection_monitor_rx = mqtt_broker.connection_monitor_subscribe();
    let maybe_processor = start_processor(
        processor_db,
        mqtt_sender,
        mqtt_admin,
        connection_monitor_rx,
        connected_clients.clone(),
        config.processor.clone(),
    )
    .await;
    let _processor = {
        match maybe_processor {
            Ok(processor) => processor,
            Err(e) => {
                panic!("Failed to start processor: {:?}", e);
            }
        }
    };

    let api_db = db.clone();
    let mqtt_sender = mqtt_broker.mqtt.clone();
    let mqtt_metrics = mqtt_broker.metrics.clone();
    let _api_server_cancel_token = start_api_server(
        &config.bind_api,
        api_db,
        Some(mqtt_sender),
        mqtt_metrics,
        connected_clients,
        &config,
    )
    .await;

    let server_cancel_token = _broker_cancel_token.clone();

    tokio::spawn(async move {
        tokio::select! {
            _ = _broker_cancel_token.cancelled() => {
                warn!("Broker cancelled");
                _api_server_cancel_token.cancel();
            }
            _ = _api_server_cancel_token.cancelled() => {
                warn!("API server cancelled");
                _broker_cancel_token.cancel();
            }
        }
    });

    server_cancel_token
}
