use crate::controller::{AttributeType, LongDevice};
use serde_json::json;
use simple_error::{bail, simple_error};
use std::error::Error;

use crate::config::{Config, TopicType};
use crate::utils::ResultExtensions;

pub struct AutodiscoveryMessage {
    pub component: &'static str,
    pub discovery_info: serde_json::Value,
}

pub fn device_to_discovery_payload(
    config: &Config,
    device: &LongDevice,
) -> Option<AutodiscoveryMessage> {
    if device.attribute("Level").is_some() {
        return dimmer_to_discovery_payload(&config, device)
            .log_failing_result("dimmer_discovery_failed");
    }
    if device.attribute("On_Off").is_some() {
        return switch_to_discovery_payload(&config, device)
            .log_failing_result("switch_discovery_failed");
    }
    return None;
}

fn switch_to_discovery_payload(
    config: &Config,
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

    let unique_id = format!(
        "{}/{}",
        config
            .topic_prefix
            .as_ref()
            .ok_or_else(|| simple_error!("No topic prefix defined"))?,
        device.id
    );
    let state_topic = config
        .to_topic_string(&TopicType::StatusTopic(device.id))
        .unwrap();
    let command_topic = config
        .to_topic_string(&TopicType::SetAttributeTopic(device.id, on_off.id))
        .unwrap();

    Ok(AutodiscoveryMessage {
        component: "switch",
        discovery_info: json!({
            "platform": "mqtt",
            "unique_id": unique_id,
            "name": device.name,
            "state_topic": state_topic,
            "value_template": "{{ value_json.On_Off | upper }}",
            "command_topic": command_topic,
            "payload_on": payload_on,
            "payload_off": payload_off,
        }),
    })
}

fn dimmer_to_discovery_payload(
    config: &Config,
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

    let unique_id = format!(
        "{}/{}",
        config
            .topic_prefix
            .as_ref()
            .ok_or_else(|| simple_error!("No topic prefix defined"))?,
        device.id
    );
    let state_topic = config
        .to_topic_string(&TopicType::StatusTopic(device.id))
        .unwrap();
    let command_topic = config
        .to_topic_string(&TopicType::SetAttributeTopic(device.id, level.id))
        .unwrap();

    Ok(AutodiscoveryMessage {
        component: "light",
        discovery_info: json!({
            "platform": "mqtt",
            "unique_id": unique_id,
            "name": device.name,
            "state_topic": state_topic,
            "state_value_template": "{% if value_json.Level > 0 %}1{% else %}0{% endif %}",
            "command_topic": command_topic,
            "on_command_type": "brightness",
            "payload_off": "0",
            "payload_on": "1",
            "brightness_state_topic": state_topic,
            "brightness_command_topic": command_topic,
            "brightness_value_template": "{{value_json.Level}}",
            "brightness_scale": scale,
        }),
    })
}
