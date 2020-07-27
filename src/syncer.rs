use mqtt_async_client::client::{Client, SubscribeResult, Subscribe, SubscribeTopic, QoS};
use crate::controller::DeviceController;
use std::error::Error;
use std::io::Read;
use serde_json::value::Value::{Number, Bool, Object};
use slog::{error, warn};
use std::collections::HashMap;
use simple_error::{bail, SimpleError};

pub struct DeviceSyncer<'a> {
    controller: &'a mut dyn DeviceController,
    mqtt_client: &'a mut Client,
    topic_prefix: String,
}

impl<'a> DeviceSyncer<'a> {
    pub fn new(controller: &'a mut dyn DeviceController, mqtt_client: &'a mut Client, topic_prefix: &str) -> DeviceSyncer<'a> {
        return DeviceSyncer{
            controller,
            mqtt_client,
            topic_prefix: topic_prefix.to_string(),
        };
    }

    async fn do_subscribe(&mut self) -> Result<(), Box<dyn Error>> {
        Ok(self.mqtt_client.subscribe(
            Subscribe::new(vec![SubscribeTopic {
                topic_path: format!("{}/+/set", self.topic_prefix),
                qos: QoS::AtLeastOnce
            }])).await?.any_failures()?)
    }

    async fn loop_once(&mut self) -> Result<(), Box<dyn Error>> {
        let result = self.mqtt_client.read_subscriptions().await?;
        let device_id = result.topic()
            .strip_prefix(&self.topic_prefix)
            .and_then(|v| v.strip_prefix("/"))
            .and_then(|v| v.strip_suffix("/set"))
            .ok_or(SimpleError::new("bad topic"))?
            .parse::<u64>()?
            as crate::controller::DeviceId;

        let value = match serde_json::from_slice(result.payload())? {
            Object(map) => map,
            _ => {
                let input = std::str::from_utf8(result.payload()).unwrap();
                bail!("Input to set not a map: {}", input)
            }
        };

        let attribute_names = self.controller.describe(device_id)?
            .attributes
            .into_iter()
            .map(|mut item| (item.description.to_string(), item))
            .collect::<HashMap<_, _>>();

        for (k, v) in value.iter() {
            let attribute_id = match attribute_names.get(k) {
                Some(v) => {
                    if !v.supports_write {
                        error!(slog_scope::logger(), "Attribute {} does not support writes", v.description);
                        continue
                    }
                    v.id
                },
                _ => {
                    error!(slog_scope::logger(), "Bad attribute name: {}", k);
                    continue
                }
            };

            let value = match v {
                Number(n) => format!("{}", n),
                Bool(v) => if *v { "TRUE" } else { "FALSE" }.to_string(),
                serde_json::Value::String(s) => s.clone(),
                v => {
                    error!(slog_scope::logger(), "unknown_setting_for_key"; "key" => k, "value" => format!("{}", v));
                    continue
                }
            };
            self.controller.set(device_id, attribute_id, &value)?
        }

        Ok(())
    }

    pub fn start(&mut self) -> () {
        tokio::spawn(async move {
            loop {
                let result = self.do_subscribe().await;
                if result.is_ok() {
                    break
                } else {
                    warn!(slog_scope::logger(), "could_not_subscribe"; "err" => format!("{}", result.unwrap_err()))
                }
            }

            loop {
                let result = self.loop_once().await;
                if !result.is_ok() {
                    error!(slog_scope::logger(), "loop_error"; "err" => format!("{}", result.unwrap_err()))
                }
            }
        });
    }
}