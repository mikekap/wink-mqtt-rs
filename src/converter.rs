use crate::controller::{AttributeType, LongDevice};
use serde_json::json;
use simple_error::bail;
use std::error::Error;

use crate::utils::ResultExtensions;

pub struct AutodiscoveryMessage {
    pub component: &'static str,
    pub discovery_info: serde_json::Value,
}

pub fn device_to_discovery_payload(
    topic_prefix: &str,
    device: &LongDevice,
) -> Option<AutodiscoveryMessage> {
    if device.attribute("Level").is_some() {
        return dimmer_to_discovery_payload(topic_prefix, device)
            .log_failing_result("dimmer_discovery_failed");
    }
    if device.attribute("On_Off").is_some() {
        return switch_to_discovery_payload(topic_prefix, device)
            .log_failing_result("switch_discovery_failed");
    }
    return None;
}

fn switch_to_discovery_payload(
    topic_prefix: &str,
    device: &LongDevice,
) -> Result<AutodiscoveryMessage, Box<dyn Error>> {
    let on_off = device.attribute("On_Off").unwrap();

    let (payload_on, payload_off) = match on_off.attribute_type {
        AttributeType::UInt8 => ("0", "255"),
        AttributeType::UInt16 => ("0", "65535"),
        AttributeType::UInt32 => ("0", "4294967295"),
        AttributeType::Bool => ("TRUE", "FALSE"),
        AttributeType::String => ("ON", "OFF"),
    };

    Ok(AutodiscoveryMessage {
        component: "switch",
        discovery_info: json!({
            "platform": "mqtt",
            "unique_id": format!("{}/{}", topic_prefix, device.id),
            "name": device.name,
            "state_topic": format!("{}{}/status", topic_prefix, device.id),
            "value_template": "{{ value_json.On_Off | upper }}",
            "command_topic": format!("{}{}/{}/set", topic_prefix, device.id, on_off.id),
            "payload_on": payload_on,
            "payload_off": payload_off,
        }),
    })
}

fn dimmer_to_discovery_payload(
    topic_prefix: &str,
    device: &LongDevice,
) -> Result<AutodiscoveryMessage, Box<dyn Error>> {
    let level = device.attribute("Level").unwrap();
    let scale: u32 = match level.attribute_type {
        AttributeType::UInt8 => u8::max_value() as u32,
        AttributeType::UInt16 => u16::max_value() as u32,
        AttributeType::UInt32 => u32::max_value(),
        AttributeType::Bool => 1,
        AttributeType::String => {
            bail!("A string level type! Please report with `aprontest -l` output!")
        }
    };

    Ok(AutodiscoveryMessage {
        component: "light",
        discovery_info: json!({
            "platform": "mqtt",
            "unique_id": format!("{}/{}", topic_prefix, device.id),
            "name": device.name,
            "state_topic": format!("{}{}/status", topic_prefix, device.id),
            "state_value_template": "{% if value_json.Level > 0 %}1{% else %}0{% endif %}",
            "command_topic": format!("{}{}/{}/set", topic_prefix, device.id, level.id),
            "on_command_type": "brightness",
            "payload_off": "0",
            "payload_on": "1",
            "brightness_state_topic": format!("{}{}/status", topic_prefix, device.id),
            "brightness_command_topic": format!("{}{}/{}/set", topic_prefix, device.id, level.id),
            "brightness_value_template": "{{value_json.Level}}",
            "brightness_scale": scale,
        }),
    })
}
