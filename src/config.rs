use crate::config::TopicType::{DiscoveryTopic, SetAttributeTopic, SetJsonTopic, StatusTopic};
use crate::controller::{AttributeId, DeviceId};
use crate::utils::Numberish;
use regex::Regex;
use rumqttc::MqttOptions;
use simple_error::bail;
use std::error::Error;
use std::fmt;
use std::ops::Add;

#[derive(Debug, Clone)]
pub struct Config {
    pub mqtt_options: Option<MqttOptions>,
    pub topic_prefix: Option<String>,
    pub discovery_topic_prefix: Option<String>,
    pub discovery_listen_topic: Option<String>,
    pub resync_interval: u64,
    pub http_port: Option<u16>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum TopicType {
    SetJsonTopic(DeviceId),
    SetAttributeTopic(DeviceId, AttributeId),
    StatusTopic(DeviceId),
    DiscoveryTopic(String, DeviceId),
    DiscoveryListenTopic(),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct NotInterestingTopicError {}

impl fmt::Display for NotInterestingTopicError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "not an interesting topic")
    }
}
impl Error for NotInterestingTopicError {}

lazy_static! {
    static ref SLASHES_ON_END_REGEX: Regex = Regex::new("/+$").unwrap();
    static ref DISCOVERY_SUFFIX_REGEX: Regex =
        Regex::new("(?P<component>[^/]+)/wink_(?P<device_id>[0-9]+)/config").unwrap();
}

impl Config {
    fn normalize_topic_prefix(x: &str) -> String {
        SLASHES_ON_END_REGEX.replace(x, "").into_owned().add("/")
    }

    pub fn new(
        mqtt_options: Option<MqttOptions>,
        topic_prefix: Option<&str>,
        discovery_topic_prefix: Option<&str>,
        discovery_listen_topic: Option<&str>,
        resync_interval: u64,
        http_port: Option<u16>,
    ) -> Config {
        Config {
            mqtt_options: mqtt_options.map(|x| x.clone()),
            topic_prefix: topic_prefix.map(Self::normalize_topic_prefix),
            discovery_topic_prefix: discovery_topic_prefix.map(Self::normalize_topic_prefix),
            discovery_listen_topic: discovery_listen_topic.map(|x| x.to_string()),
            resync_interval,
            http_port,
        }
    }

    pub fn has_mqtt(&self) -> bool {
        self.mqtt_options.is_some() && self.topic_prefix.is_some()
    }

    pub fn is_interesting_topic(&self, topic: &str) -> bool {
        self.topic_prefix.is_some()
            && topic.starts_with(self.topic_prefix.as_ref().unwrap().as_str())
    }

    pub fn is_discovery_topic(&self, topic: &str) -> bool {
        self.discovery_topic_prefix.is_some()
            && topic.starts_with(self.discovery_topic_prefix.as_ref().unwrap().as_str())
    }

    pub fn is_discovery_listen_topic(&self, topic: &str) -> bool {
        self.discovery_listen_topic.is_some()
            && topic == self.discovery_listen_topic.as_ref().unwrap()
    }

    pub fn mqtt_topic_subscribe_patterns(&self) -> impl Iterator<Item = String> {
        let mut result: Vec<String> = Vec::with_capacity(3);
        if let Some(prefix) = self.topic_prefix.as_ref() {
            result.push(format!("{}+/set", prefix));
            result.push(format!("{}+/+/set", prefix));
        }
        if let Some(disco) = self.discovery_listen_topic.as_ref() {
            result.push(disco.clone());
        }
        return result.into_iter();
    }

    pub fn parse_mqtt_topic(&self, topic: &str) -> Result<TopicType, Box<dyn Error>> {
        if self.is_discovery_listen_topic(topic) {
            Ok(TopicType::DiscoveryListenTopic())
        } else if self.is_discovery_topic(topic) {
            let suffix = topic
                .strip_prefix(self.discovery_topic_prefix.as_ref().unwrap())
                .unwrap();
            let parsed = match DISCOVERY_SUFFIX_REGEX.captures(suffix) {
                Some(caps) => caps,
                None => {
                    bail!("Invalid discovery topic: {}", topic)
                }
            };

            Ok(DiscoveryTopic(
                parsed.name("component").unwrap().as_str().into(),
                parsed
                    .name("device_id")
                    .unwrap()
                    .as_str()
                    .parse_numberish()?,
            ))
        } else if self.is_interesting_topic(topic) {
            let path_components = topic
                .strip_prefix(self.topic_prefix.as_ref().unwrap())
                .unwrap()
                .split("/")
                .collect::<Vec<_>>();
            if path_components.is_empty() {
                bail!("Invalid topic: {}", topic)
            }

            if path_components.last().unwrap() == &"set"
                && path_components.len() >= 2
                && path_components.len() <= 3
            {
                let device_id =
                    path_components.first().unwrap().parse::<u64>()? as crate::controller::DeviceId;

                if let [_, rest, _] = path_components[..] {
                    let attribute_id = rest.parse::<u64>()? as AttributeId;
                    Ok(SetAttributeTopic(device_id, attribute_id))
                } else {
                    Ok(SetJsonTopic(device_id))
                }
            } else if path_components.last().unwrap() == &"status" && path_components.len() == 2 {
                let device_id =
                    path_components.first().unwrap().parse::<u64>()? as crate::controller::DeviceId;

                Ok(StatusTopic(device_id))
            } else {
                bail!("Bad internal topic: {}; {:?}", topic, path_components)
            }
        } else {
            Err(NotInterestingTopicError {}.into())
        }
    }
    pub fn to_topic_string(&self, topic: &TopicType) -> Option<String> {
        match topic {
            SetJsonTopic(device_id) => self
                .topic_prefix
                .as_ref()
                .map(|prefix| format!("{}{}/set", prefix, device_id)),
            SetAttributeTopic(device_id, attribute_id) => self
                .topic_prefix
                .as_ref()
                .map(|prefix| format!("{}{}/{}/set", prefix, device_id, attribute_id)),
            StatusTopic(device_id) => self
                .topic_prefix
                .as_ref()
                .map(|prefix| format!("{}{}/status", prefix, device_id)),
            DiscoveryTopic(device_type, device_id) => self
                .discovery_topic_prefix
                .as_ref()
                .map(|prefix| format!("{}{}/wink_{}/config", prefix, device_type, device_id)),
            TopicType::DiscoveryListenTopic() => self.discovery_listen_topic.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    lazy_static! {
        static ref TEST_CASES: Vec<TopicType> = [
            SetJsonTopic(1),
            SetAttributeTopic(1, 3),
            StatusTopic(1),
            DiscoveryTopic("light".to_string(), 1),
            TopicType::DiscoveryListenTopic(),
        ]
        .to_vec();
    }

    #[test]
    fn empty_config() {
        let config = Config::new(None, None, None, None, 10, None);

        for case in TEST_CASES.iter() {
            assert_eq!(None, config.to_topic_string(case))
        }

        assert_ne!(
            None,
            config
                .parse_mqtt_topic("somewhere/out/there")
                .unwrap_err()
                .downcast_ref::<NotInterestingTopicError>()
        )
    }

    #[test]
    fn full_config() {
        let config = Config::new(
            Some(&MqttOptions::new("a", "localhost", 123)),
            Some("topic/prefix/"),
            Some("discovery/topic/prefix/"),
            Some("fire/discovery"),
            10,
            None,
        );

        for case in TEST_CASES.iter() {
            let topic = config.to_topic_string(case).unwrap();
            assert_eq!(*case, config.parse_mqtt_topic(&topic).unwrap());
            assert!(topic.find("//").is_none());
        }
    }
}
