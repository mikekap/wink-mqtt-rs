use crate::controller::{LongDevice, AttributeType};
use serde_json::json;

pub struct AutodiscoveryMessage {
    pub component: &'static str,
    pub discovery_info: serde_json::Value,
}

pub fn device_to_discovery_payload(topic_prefix: &str, device: &LongDevice) -> Option<AutodiscoveryMessage> {
    if device.attribute("Up_Down").is_some() && device.attribute("Level").is_some() {
        return Some(dimmer_to_discovery_payload(topic_prefix, device))
    }
    return None
}

fn dimmer_to_discovery_payload(topic_prefix: &str, device: &LongDevice) -> AutodiscoveryMessage {
    let level = device.attribute("Level").unwrap();
    let scale = match level.attribute_type {
        AttributeType::UInt8 => 255,
        AttributeType::Bool => 1,
    };

    AutodiscoveryMessage {
        component: "light",
        discovery_info: json!({
            "platform": "mqtt",
            "name": device.name,
            "state_topic": format!("{}{}/status", topic_prefix, device.id),
            "command_topic": format!("{}{}/{}/set", topic_prefix, device.id, level.id),
            "on_command_type": "brightness",
            "payload_off": "0",
            "brightness_state_topic": format!("{}{}/status", topic_prefix, device.id),
            "brightness_command_topic": format!("{}{}/{}/set", topic_prefix, device.id, level.id),
            "brightness_value_template": "{{value_json.Level}}",
            "brightness_scale": scale,
        })
    }
}
