use rumqttc::{Client, MqttOptions, EventLoop, Subscribe, Publish, Request, Incoming};
use crate::controller::{DeviceController, ShortDevice, DeviceId, LongDevice, AttributeId};
use std::error::Error;
use std::io::Read;
use serde_json::value::Value::{Number, Bool, Object};
use slog::{error, warn};
use std::collections::HashMap;
use simple_error::{bail, SimpleError};
use std::sync::{Arc, Mutex, RwLock};
use std::cell::RefCell;

pub struct DeviceSyncer<T> where T : DeviceController {
    topic_prefix: String,
    discovery_prefix: String,
    controller: Mutex<T>,
    event_loop: tokio::sync::RwLock<EventLoop>,
}

impl<T: 'static> DeviceSyncer<T> where T : DeviceController {
    pub async fn new(options: MqttOptions, topic_prefix: &str, discovery_prefix: &str, controller: T) -> Arc<DeviceSyncer<T>> {
        let ev = EventLoop::new(options, 100).await;
        let mut syncer = DeviceSyncer{
            topic_prefix: topic_prefix.to_string(),
            discovery_prefix: discovery_prefix.to_string(),
            event_loop: tokio::sync::RwLock::new(ev),
            controller: Mutex::new(controller),
        };
        let mut ptr = Arc::new(syncer);
        let ptr_clone = ptr.clone();
        tokio::task::spawn(async move {
            Self::run(ptr)
        });
        ptr_clone
    }

    async fn do_subscribe(&self) -> Result<(), Box<dyn Error>> {
        let subscribe = Subscribe::new(
            format!("{}/+/set", &self.topic_prefix),
            rumqttc::QoS::AtLeastOnce);

        self.event_loop.read().await.handle().send(Request::Subscribe(subscribe)).await?;
        Ok(())
    }

    fn report_async_result<X>(type_: &str, r: Result<X, Box<dyn Error>>) {
        if !r.is_ok() {
            warn!(slog_scope::logger(), "async_failure"; "type" => type_, "error" => format!("{}", r.err().unwrap()));
        }
        ();
    }

    async fn process_one(mut this: Arc<Self>, message: Publish) -> Result<(), Box<dyn Error>> {
        if message.topic.starts_with(this.topic_prefix.as_str()) {
            tokio::task::spawn_blocking(async move || {
                Self::report_async_result("set", this.process_one_control_message(message));
            });
            Ok(())
        } else {
            bail!("Unknown message topic: {}", message.topic)
        }
    }

    fn process_one_control_message(&self, message: Publish) -> Result<(), Box<dyn Error>> {
        let device_id = message.topic
            .strip_prefix(self.topic_prefix.as_str())
            .and_then(|v| v.strip_prefix("/"))
            .and_then(|v| v.strip_suffix("/set"))
            .ok_or(SimpleError::new("bad topic"))?
            .parse::<u64>()?
            as crate::controller::DeviceId;

        let value = match serde_json::from_slice(&message.payload)? {
            Object(map) => map,
            _ => {
                let input = std::str::from_utf8(&message.payload).unwrap();
                bail!("Input to set not a map: {}", input)
            }
        };

        let mut controller = self.controller.lock().unwrap();

        let attribute_names = controller.describe(device_id)?
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
            controller.set(device_id, attribute_id, &value)?
        }

        Ok(())
    }

    async fn loop_once(mut this: Arc<Self>) -> Result<(), Box<dyn Error>> {
        let (message, _) = this.event_loop.write().await.poll().await?;

        if message.is_none() {
            return Ok(())
        }

        return match message.unwrap() {
            Incoming::Connected => {
                this.do_subscribe().await?;
                Ok(())
            },
            Incoming::Publish(message) => {
                Self::process_one(this, message).await?;
                Ok(())
            }
            Incoming::PubAck(_) => {
                Ok(())
            }
            Incoming::PubRec(_) => {
                bail!("Unexpected pubrec");
            }
            Incoming::PubRel(_) => {
                bail!("Unexpected pubrel");
            }
            Incoming::PubComp(_) => {
                bail!("Unexpected pubcomp")
            }
            Incoming::Subscribe(_) => {
                bail!("Unexpected subscribe")
            }
            Incoming::SubAck(_) => {
                Ok(())
            }
            Incoming::Unsubscribe(_) => {
                bail!("Unexpected unsubscribe!")
            }
            Incoming::UnsubAck(_) => {
                bail!("Unexpected unsuback!")
            }
            Incoming::PingReq => {
                Ok(())
            }
            Incoming::PingResp => {
                Ok(())
            }
            Incoming::Disconnect => {
                Ok(())
            }
        }
    }

    async fn run(mut this: Arc<Self>) -> () {
        loop {
            let result = Self::loop_once(this.clone()).await;
            if !result.is_ok() {
                warn!(slog_scope::logger(), "loop_encountered_error"; "err" => format!("{}", result.unwrap_err()))
            }
        }
    }
}