pub mod shadow;
pub mod time;
pub mod timeseries;
pub mod topics;

pub use shadow::send_delta_to_mqtt;

use rumqttd::AdminLink;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast::Receiver;
use tokio::task::JoinSet;
use tracing::{debug, debug_span, warn, Instrument};

use crate::db::DB;
use crate::mqtt::{ClientStatus, MqttError, MqttMessage, MqttSender};
use crate::server::ConnectionSet;

use crate::processor::shadow::handle_shadow_update;
use crate::processor::time::handle_time_request;
use crate::processor::timeseries::handle_metric_extraction;
use crate::processor::topics::{get_topic_type, TopicType};

#[derive(Error, Debug)]
pub enum ProcessorError {
    #[error("MQTT error: {0}")]
    Mqtt(#[from] MqttError),
    #[error("Invalid Topic: {0}")]
    InvalidTopic(String),
    #[error("Database error: {0}")]
    DatabaseError(#[from] crate::db::DatabaseError),
    #[error("Shadow error: {0}")]
    ShadowSerializationError(#[from] crate::shadow::ShadowSerializationError),
    #[error("Invalid Shadow Update: {0}")]
    InvalidShadowUpdate(String),
    #[error("Invalid Json: {0}")]
    InvalidJson(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProcessorConfig {
    pub shadow_topic_prefix: String,
    pub telemetry_topics: Vec<String>,
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        ProcessorConfig {
            shadow_topic_prefix: "things/".to_string(),
            telemetry_topics: vec!["things/+/data".to_string()],
        }
    }
}
#[derive(Clone)]
pub struct ProcessorState {
    db: Arc<DB>,
    mqtt_sender: MqttSender,
    config: Arc<ProcessorConfig>,
}

pub struct Processor {
    pub db: Arc<DB>,
    pub mqtt_sender: MqttSender,
}

impl Processor {
    pub async fn subscribe_shadow_updates(
        &mut self,
        topic_patterns: Vec<String>,
    ) -> Result<(), ProcessorError> {
        for pattern in topic_patterns {
            self.mqtt_sender.subscribe(pattern).await?;
        }
        Ok(())
    }
}
async fn handle_message(msg: MqttMessage, state: ProcessorState) {
    let topic_type = get_topic_type(&msg, &state);

    if matches!(topic_type, TopicType::Other) {
        return;
    }

    let mut task_set: JoinSet<Result<(), ProcessorError>> = JoinSet::new();
    let payload = msg.payload;

    match topic_type {
        TopicType::ShadowUpdate(tid, did, sn) => {
            task_set.spawn({
                let state = state.clone();
                let payload = payload.clone();
                let tid = tid.clone();
                let did = did.clone();
                async move { handle_shadow_update(&tid, &did, &sn, payload, state).await }
            });
            task_set.spawn({
                let state = state.clone();
                let payload = payload.clone();
                let tid = tid.clone();
                let did = did.clone();
                async move { handle_metric_extraction(&tid, &did, payload, state).await }
            });
        }
        TopicType::DataUpdate(tid, did) => {
            task_set.spawn({
                let state = state.clone();
                let payload = payload.clone();
                async move { handle_metric_extraction(&tid, &did, payload, state).await }
            });
        }
        TopicType::TimeRequest(tid, did) => {
            task_set.spawn({
                let state = state.clone();
                let payload = payload.clone();
                async move { handle_time_request(&tid, &did, payload, state).await }
            });
        }
        _ => {
            println!("Unknown target {:?}", msg.topic);
        }
    }

    // Wait for all tasks to complete
    while let Some(res) = task_set.join_next().await {
        match res {
            Ok(Err(e)) => {
                warn!(error=?e, "Error processing message");
            }
            Ok(Ok(_)) => {}
            Err(err) => {
                warn!(error=?err, "Error processing message");
            }
        }
    }
}

async fn run_stream_worker(mut admin_link: AdminLink, state: ProcessorState) {
    loop {
        let rs = admin_link.recv().await;
        match rs {
            Ok(Some((publish, client_info))) => {
                if let Ok(topic) = std::str::from_utf8(&publish.topic) {
                    let msg = MqttMessage {
                        topic: topic.to_string(),
                        payload: publish.payload.to_vec(),
                    };

                    let _ = client_info.client_id;
                    let state = state.clone();
                    tokio::spawn(async move {
                        let _ = handle_message(msg, state).await;
                    });
                } else {
                    warn!("publish admin topic could not be decoded!");
                }
            }
            Ok(None) => {
                debug!("admin link closed! Ok(None)");
                break;
            }
            Err(e) => {
                debug!("admin link closed! Err({:?})", e);
                break;
            }
        }
    }
}

async fn connection_monitor(
    mut connection_monitor_rx: Receiver<ClientStatus>,
    clients: Arc<ConnectionSet>,
) {
    while let Ok(status) = connection_monitor_rx.recv().await {
        match status {
            ClientStatus::Connected(client_id) => {
                clients.insert(client_id);
            }
            ClientStatus::Disconnected(client_id) => {
                clients.remove(&client_id);
            }
        }
    }
}

pub async fn start_processor(
    db: Arc<DB>,
    mqtt_sender: MqttSender,
    admin_link: AdminLink,
    connection_monitor_rx: Receiver<ClientStatus>,
    connected_clients: Arc<ConnectionSet>,
    config: ProcessorConfig,
) -> Result<(Processor, tokio::task::JoinHandle<()>), ProcessorError> {
    let mut processor = Processor {
        db: db,
        mqtt_sender: mqtt_sender,
    };

    let config = Arc::new(config);

    //  run stream worker
    let h1 = tokio::spawn({
        let state = ProcessorState {
            db: processor.db.clone(),
            mqtt_sender: processor.mqtt_sender.clone(),
            config: config.clone(),
        };
        async move {
            let _ = run_stream_worker(admin_link, state)
                .instrument(debug_span!("ShadowUpdateWorker"))
                .await;
        }
    });

    // run connection monitor
    let h2 = tokio::spawn({
        async move {
            let _ = connection_monitor(connection_monitor_rx, connected_clients)
                .instrument(debug_span!("ConnectionMonitor"))
                .await;
        }
    });

    let combined_handle = tokio::spawn(async move {
        let _ = tokio::join!(h1, h2);
    });

    let mut topic_patterns = vec![
        format!("{}+/shadow/update", config.shadow_topic_prefix),
        format!("{}+/shadow/+/update", config.shadow_topic_prefix),
        format!("{}+/time/request", config.shadow_topic_prefix),
    ];
    topic_patterns.extend(config.telemetry_topics.clone());
    processor.subscribe_shadow_updates(topic_patterns).await?;
    Ok((processor, combined_handle))
}

#[cfg(test)]
mod tests;
