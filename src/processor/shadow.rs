use crate::processor::{ProcessorState, ProcessorError};
use crate::mqtt::MqttSender;
use crate::shadow::{Shadow, StateUpdateDocument};
use crate::models::{ShadowName, TenantId};
use tracing::{debug, info};

pub(crate) fn get_delta_return_topic(device_id: &str, shadow_name: &ShadowName, topic_prefix: &str) -> String {
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
pub(crate) async fn process_update_document(
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
pub(crate) async fn handle_shadow_update(
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
