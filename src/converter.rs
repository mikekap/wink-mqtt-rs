use crate::controller::{LongDevice};
use serde_json::json;

pub struct AutodiscoveryMessage {
    pub component: &'static str,
    pub discovery_info: serde_json::Value,
}

pub fn device_to_discovery_payload(topic_prefix: &str, device: &LongDevice) -> Option<AutodiscoveryMessage> {
    if device.attributes.iter().any(|x| x.description == "Up_Down") {
        return Some(dimmer_to_discovery_payload(topic_prefix, device))
    }
    return None
}

fn dimmer_to_discovery_payload(topic_prefix: &str, device: &LongDevice) -> AutodiscoveryMessage {
    AutodiscoveryMessage {
        component: "light",
        discovery_info: json!({
            "platform": "mqtt",
            "name": device.name,
            "state_topic": format!("{}{}/status", topic_prefix, device.id),
            "command_topic": format!("{}{}/set", topic_prefix, device.id),
            "on_command_type": "brightness",
            "payload_off": "{\"Level\": 0}",
            "brightness_state_topic": format!("{}{}/status", topic_prefix, device.id),
            "brightness_command_topic": format!("{}{}/set", topic_prefix, device.id),
            "brightness_value_template": "{\"Level\": {{value_json.brightness}}}",
        })
    }
}
