use crate::db::DB;
use crate::mqtt::{ClientStatus, MqttError, MqttMessage, MqttSender};
use rumqttd::AdminLink;
use crate::server::ConnectionSet;
use crate::shadow::{Shadow, StateUpdateDocument};
use crate::models::{ShadowName, TenantId};
use futures_util::stream::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::broadcast::Receiver;
use tokio::task::JoinSet;
use tracing::{debug, debug_span, error, info, warn, Instrument};

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
}

impl Default for ProcessorConfig {
    fn default() -> Self {
        ProcessorConfig {
            shadow_topic_prefix: "things/".to_string(),
        }
    }
}

type DeviceId = String;

pub enum TopicType {
    ShadowUpdate(TenantId, DeviceId, ShadowName),
    DataUpdate(TenantId, DeviceId),
    ShadowDelta(TenantId, DeviceId, ShadowName),
    Other,
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

fn get_delta_return_topic(device_id: &str, shadow_name: &ShadowName, topic_prefix: &str) -> String {
    match shadow_name {
        ShadowName::Default => format!("{}{}/shadow/update/delta", topic_prefix, device_id),
        ShadowName::Custom(name) => {
            format!("{}{}/shadow/{}/update/delta", topic_prefix, device_id, name)
        }
    }
}

pub fn send_delta_to_mqtt(
    shadow: &Shadow,
    mqtt_sender: &MqttSender,
    shadow_topic_prefix: &str,
) -> Result<bool, ProcessorError> {
    let return_topic =
        get_delta_return_topic(&shadow.device_id, &shadow.shadow_name, shadow_topic_prefix);
    // Send delta to the device
    let delta_json = shadow.get_delta_response_json()?;
    match delta_json {
        Some(json) => {
            mqtt_sender.publish(return_topic.to_string(), json.into_bytes())?;
            debug!(topic = return_topic, "Delta sent to device");
            Ok(true)
        }
        None => Ok(false),
    }
}

async fn process_update_document(
    update_doc: &StateUpdateDocument,
    state: &ProcessorState,
) -> Result<(), ProcessorError> {
    let shadow = state.db._upsert_shadow(update_doc).await?;
    let delta_sent = send_delta_to_mqtt(
        &shadow,
        &state.mqtt_sender,
        &state.config.shadow_topic_prefix,
    )?;
    info!(
        %update_doc.tenant_id,
        update_doc.device_id, %update_doc.shadow_name, delta_sent, "Processed shadow update"
    );
    Ok(())
}

fn split_device_id(device_id: &str) -> (TenantId, DeviceId) {
    match device_id.split_once('.') {
        Some((tenant_str, device_id)) => (TenantId::from_str(tenant_str), device_id.to_string()),
        None => (TenantId::Default, device_id.to_string()),
    }
}

fn get_topic_type(msg: &MqttMessage, processor_state: &ProcessorState) -> TopicType {
    // check if the topic is a shadow update and strip prefix
    let shadow_topic = match msg
        .topic
        .strip_prefix(processor_state.config.shadow_topic_prefix.as_str())
    {
        Some(t) => t,
        None => return TopicType::Other,
    };

    let parts: Vec<&str> = shadow_topic.split('/').collect();
    // determine the type of message
    // first part is always the device_id
    // second part is always shadow or data -> shadow update or data update
    // return (type, tenant_id, device_id, shadow_name)

    match &parts[..] {
        [device_id, "shadow", "update"] => {
            let (tenant, device) = split_device_id(device_id);
            return TopicType::ShadowUpdate(tenant, device, ShadowName::Default);
        }
        [device_id, "shadow", shadow_name, "update"] => {
            let (tenant, device) = split_device_id(device_id);
            return TopicType::ShadowUpdate(
                tenant,
                device,
                ShadowName::from_str(shadow_name),
            );
        }
        [device_id, "data"] => {
            let (tenant, device) = split_device_id(device_id);
            return TopicType::DataUpdate(tenant, device);
        }
        [device_id, "shadow", "update", "delta"] => {
            let (tenant, device) = split_device_id(device_id);
            return TopicType::ShadowDelta(tenant, device, ShadowName::Default);
        }
        [device_id, "shadow", shadow_name, "update", "delta"] => {
            let (tenant, device) = split_device_id(device_id);
            return TopicType::ShadowDelta(
                tenant,
                device,
                ShadowName::from_str(shadow_name),
            );
        }
        _ => {
            return TopicType::Other;
        }
    }
}

async fn handle_shadow_update(
    tenant_id: &TenantId,
    device_id: &str,
    shadow_name: &ShadowName,
    payload: Vec<u8>,
    state: ProcessorState,
) -> Result<(), ProcessorError> {
    if let Ok(json_str) = String::from_utf8(payload) {
        if let Ok(update_doc) =
            StateUpdateDocument::from_nested_json(&json_str, device_id, shadow_name, tenant_id)
        {
            process_update_document(&update_doc, &state).await?;
        } else {
            return Err(ProcessorError::InvalidShadowUpdate(
                "Failed to parse JSON".to_string(),
            ));
        }
    } else {
        return Err(ProcessorError::InvalidShadowUpdate(
            "Not able to convert payload to utf-8 string".to_string(),
        ));
    }
    Ok(())
}

async fn handle_metric_extraction(
    tenant_id: &TenantId,
    device_id: &str,
    payload: Vec<u8>,
    state: ProcessorState,
) -> Result<(), ProcessorError> {
    let maybe_json = serde_json::from_slice::<serde_json::Value>(&payload);
    let json = match maybe_json {
        Ok(json) => json,
        Err(e) => {
            return Err(ProcessorError::InvalidJson(format!(
                "Failed to parse JSON: {}",
                e
            )));
        }
    };

    // get data config from db
    let maybe_config = state.db.get_data_config(tenant_id, Some(device_id)).await?;
    let metrics = match maybe_config {
        Some(data_config) => data_config.extract_metrics_from_json(json),
        None => return Ok(()),
    };

    let mut counter = 0;
    // store metrics
    // TODO: batch insert for metrics
    for (metric_name, metric_value) in metrics {
        let res = state
            .db
            .put_metric(tenant_id, device_id, &metric_name, metric_value)
            .await;
        match res {
            Ok(_) => {
                counter += 1;
                debug!(metric_name, "Stored metric");
            }
            Err(e) => {
                return Err(ProcessorError::DatabaseError(e));
            }
        }
    }

    info!(%tenant_id, device_id, counter, "Processed metrics");

    Ok(())
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
        _ => {
            warn!(topic = msg.topic, "Unknown topic type");
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
    while let Ok(Some((publish, client_info))) = admin_link.recv().await {
        if let Ok(topic) = std::str::from_utf8(&publish.topic) {
            let msg = MqttMessage {
                topic: topic.to_string(),
                payload: publish.payload.to_vec(),
            };
            
            // NOTE: client_info could be passed to handle_message if needed in the future
            let _ = client_info.client_id;
            
            let state = state.clone();
            tokio::spawn(async move {
                let _ = handle_message(msg, state).await;
            });
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

    let topic_patterns = vec![
        format!("{}+/shadow/update", config.shadow_topic_prefix),
        format!("{}+/shadow/+/update", config.shadow_topic_prefix),
    ];
    processor.subscribe_shadow_updates(topic_patterns).await?;
    Ok((processor, combined_handle))
}

#[cfg(test)]
mod tests;
