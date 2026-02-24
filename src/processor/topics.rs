use crate::models::{ShadowName, TenantId};
use crate::mqtt::MqttMessage;
use crate::processor::{ProcessorState, ProcessorConfig};

type DeviceId = String;

#[derive(Debug)]
pub enum TopicType {
    ShadowUpdate(TenantId, DeviceId, ShadowName),
    DataUpdate(TenantId, DeviceId),
    ShadowDelta(TenantId, DeviceId, ShadowName),
    Other,
}
fn split_device_id(device_id: &str) -> (TenantId, DeviceId) {
    match device_id.split_once('.') {
        Some((tenant_str, device_id)) => (TenantId::from_str(tenant_str), device_id.to_string()),
        None => (TenantId::Default, device_id.to_string()),
    }
}
pub(crate) fn get_topic_type(msg: &MqttMessage, processor_state: &ProcessorState) -> TopicType {
    // Check if it matches any telemetry topics
    for pattern in &processor_state.config.telemetry_topics {
        let pattern_parts: Vec<&str> = pattern.split('/').collect();
        let topic_parts: Vec<&str> = msg.topic.split('/').collect();

        if pattern_parts.len() == topic_parts.len() {
            let mut matches = true;
            let mut extracted_device_id = None;
            for (p, t) in pattern_parts.iter().zip(topic_parts.iter()) {
                if *p == "+" {
                    if extracted_device_id.is_none() {
                        extracted_device_id = Some(t.to_string());
                    }
                } else if p != t {
                    matches = false;
                    break;
                }
            }
            if matches {
                if let Some(device_id_str) = extracted_device_id {
                    let (tenant, device) = split_device_id(&device_id_str);
                    return TopicType::DataUpdate(tenant, device);
                }
            }
        }
    }

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
            return TopicType::ShadowUpdate(tenant, device, ShadowName::from_str(shadow_name));
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
            return TopicType::ShadowDelta(tenant, device, ShadowName::from_str(shadow_name));
        }
        _ => {
            return TopicType::Other;
        }
    }
}
