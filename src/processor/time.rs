use crate::models::TenantId;
use crate::processor::{ProcessorError, ProcessorState};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
struct TimeRequestPayload {
    pub device_time: Option<u64>,
}

#[derive(Serialize, Debug)]
struct TimeResponsePayload {
    pub server_time: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_time: Option<u64>,
}

pub(crate) async fn handle_time_request(
    _tenant_id: &TenantId,
    device_id: &str,
    payload: Vec<u8>,
    state: ProcessorState,
) -> Result<(), ProcessorError> {
    let mut device_time_req = None;

    if !payload.is_empty() {
        if let Ok(json_str) = String::from_utf8(payload) {
            if let Ok(req) = serde_json::from_str::<TimeRequestPayload>(&json_str) {
                device_time_req = req.device_time;
            }
        }
    }

    let server_time = chrono::Utc::now().timestamp_millis() as u64;
    let resp = TimeResponsePayload {
        server_time,
        device_time: device_time_req,
    };

    let response_json =
        serde_json::to_string(&resp).map_err(|e| ProcessorError::InvalidJson(e.to_string()))?;

    let return_topic = format!(
        "{}{}/time/response",
        state.config.shadow_topic_prefix, device_id
    );

    state
        .mqtt_sender
        .publish(return_topic, response_json.into_bytes())
        .await?;

    Ok(())
}
