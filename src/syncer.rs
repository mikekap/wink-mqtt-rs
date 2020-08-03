use crate::controller::{DeviceController, DeviceId, AttributeValue, ShortDevice, LongDevice, AttributeId, AttributeType};
use crate::converter::device_to_discovery_payload;
use async_channel::{Sender, bounded, Receiver};
use rumqttc::{EventLoop, Incoming, MqttOptions, Publish, Request, Subscribe};
use serde_json::value::Value::{Bool, Number, Object};
use simple_error::{bail, SimpleError};
use slog::{error, info, warn, trace, debug};
use slog_scope;
use std::collections::HashMap;
use std::error::Error;
use std::sync::{Arc, Mutex};
use tokio::time::Duration;
use std::convert::TryFrom;

pub struct DeviceSyncer<T>
where
    T: DeviceController,
{
    topic_prefix: String,
    discovery_prefix: Option<String>,
    controller: Mutex<T>,
    sender: Sender<Request>,
    repoll: Sender<DeviceId>,
}

impl<T: 'static> DeviceSyncer<T>
where
    T: DeviceController,
{
    pub async fn new(
        mut options: MqttOptions,
        topic_prefix: &str,
        discovery_prefix: Option<&str>,
        controller: T,
    ) -> Arc<DeviceSyncer<T>> {
        info!(slog_scope::logger(), "opening_client"; "host" => options.broker_address().0, "port" => options.broker_address().1);
        options.set_clean_session(true);
        let ev = EventLoop::new(options, 100).await;
        let (repoll_sender, repoll_rx) = bounded(10);
        let syncer = DeviceSyncer {
            topic_prefix: topic_prefix.to_string(),
            discovery_prefix: discovery_prefix.map(|x| x.to_string()),
            controller: Mutex::new(controller),
            sender: ev.handle(),
            repoll: repoll_sender,
        };
        let ptr = Arc::new(syncer);
        let ptr_2 = ptr.clone();
        let ptr_3 = ptr.clone();
        let ptr_clone = ptr.clone();
        trace!(slog_scope::logger(), "start_thread");
        tokio::task::spawn(async move { Self::run_mqtt(ptr, ev).await });
        tokio::task::spawn(async move { Self::run_poller(ptr_2, repoll_rx).await });
        if ptr_clone.discovery_prefix.is_some() {
            tokio::task::spawn(async move { Self::broadcast_discovery(ptr_3).await });
        }
        ptr_clone
    }

    async fn do_subscribe(&self) -> Result<(), Box<dyn Error>> {
        self.sender.send(Request::Subscribe(Subscribe::new(
            format!("{}+/set", &self.topic_prefix),
            rumqttc::QoS::AtLeastOnce,
        ))).await?;

        self.sender.send(Request::Subscribe(Subscribe::new(
            format!("{}+/+/set", &self.topic_prefix),
            rumqttc::QoS::AtLeastOnce,
        ))).await?;
        Ok(())
    }

    fn report_async_result<X, E : std::fmt::Display>(type_: &str, r: Result<X, E>) {
        if !r.is_ok() {
            warn!(slog_scope::logger(), "async_failure"; "type" => type_, "error" => format!("{}", r.err().unwrap()));
        }
    }

    async fn process_one(this: Arc<Self>, message: Publish) -> Result<(), Box<dyn Error>> {
        if message.topic.starts_with(this.topic_prefix.as_str()) {
            tokio::task::spawn_blocking(move || {
                Self::report_async_result("set", this.process_one_control_message(message))
            });
            Ok(())
        } else {
            bail!("Unknown message topic: {}", message.topic)
        }
    }

    fn process_one_control_message(&self, message: Publish) -> Result<(), Box<dyn Error>> {
        let path_components = message
            .topic
            .strip_prefix(&self.topic_prefix)
            .and_then(|v| v.strip_suffix("/set"))
            .ok_or(SimpleError::new(format!("bad topic: {}", message.topic)))?
            .split("/")
            .collect::<Vec<_>>();

        let device_id = path_components.first()
            .ok_or(SimpleError::new(format!("Bad topic: {}", message.topic)))?
            .parse::<u64>()? as crate::controller::DeviceId;
        if let [_, rest] = path_components[..] {
            let attribute_id = rest.parse::<u64>()? as AttributeId;
            self.set_device_attribute_by_id(device_id, attribute_id, &message.payload)?;
        } else {
            self.set_device_attributes_json(device_id, &message.payload)?;
        };

        Ok(())
    }

    fn set_device_attribute_by_id(&self, device_id: DeviceId, attribute_id: AttributeId, payload: &[u8]) -> Result<(), Box<dyn Error>> {
        let (device_name, attribute) = {
            let info = self.controller.lock().unwrap().describe(device_id)?;
            (info.name, info.attributes
                .into_iter()
                .find(|x| x.id == attribute_id)
                .ok_or(SimpleError::new(format!("Couldn't find attribute with id {} on device {}", attribute_id, device_id)))?)
        };
        if !attribute.supports_write {
            bail!("Attribute {} does not support write", attribute.description);
        };

        let payload_str = std::str::from_utf8(payload)?.trim();
        let value = match attribute.attribute_type {
            AttributeType::UInt8 => AttributeValue::UInt8(payload_str.parse::<u8>()?),
            AttributeType::Bool => AttributeValue::Bool(match payload_str.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => bail!("Bad boolean value: {}", payload_str),
            })
        };

        self.controller.lock().unwrap().set(device_id, attribute_id, &value)?;
        info!(slog_scope::logger(), "set"; "device_id" => device_id, "device" => &device_name, "attribute" => &attribute.description, "value" => format!("{:?}", value));

        Ok(())
    }

    fn set_device_attributes_json(&self, device_id: DeviceId, payload: &[u8]) -> Result<(), Box<dyn Error>> {
        let input = std::str::from_utf8(&payload)?;
        debug!(slog_scope::logger(), "json_message"; "device_id" => device_id, "payload" => &input);

        let value = match serde_json::from_str(input)? {
            Object(map) => map,
            _ => {
                bail!("Input to set not a map: {}", input)
            }
        };

        let mut controller = self.controller.lock().unwrap();

        let (device_name, attribute_names) = {
            let info = controller.describe(device_id)?;
            (
                info.name,
                info.attributes
                    .into_iter()
                    .map(|item| (item.description.to_string(), item))
                    .collect::<HashMap<_, _>>(),
            )
        };

        for (k, v) in value.iter() {
            let attribute_id = match attribute_names.get(k) {
                Some(v) => {
                    if !v.supports_write {
                        error!(
                            slog_scope::logger(),
                            "Attribute {} does not support writes", v.description
                        );
                        continue;
                    }
                    v.id
                }
                _ => {
                    error!(slog_scope::logger(), "Bad attribute name: {}", k);
                    continue;
                }
            };

            let value = match v {
                Number(n) => AttributeValue::UInt8(n.as_u64().and_then(|x| u8::try_from(x).ok()).unwrap_or(if n.as_i64().unwrap_or(0) < 0 { 0 } else { 255 })),
                Bool(v) => AttributeValue::Bool(*v),
                // serde_json::Value::String(s) => s.clone(),
                v => {
                    error!(slog_scope::logger(), "unknown_setting_for_key"; "key" => k, "value" => format!("{}", v));
                    continue;
                }
            };
            info!(slog_scope::logger(), "set"; "device_id" => device_id, "device" => &device_name, "attribute" => k, "value" => format!("{:?}", value));
            controller.set(device_id, attribute_id, &value)?
        }

        self.repoll.try_send(device_id)?;

        Ok(())
    }

    async fn loop_once(this: Arc<Self>, ev: &mut EventLoop) -> Result<(), Box<dyn Error>> {
        let (message, _) = ev.poll().await?;

        if message.is_none() {
            return Ok(());
        }

        let message = message.unwrap();
        trace!(slog_scope::logger(), "mqtt_message"; "message" => format!("{:?}", &message));

        return match message {
            Incoming::Connected => {
                this.do_subscribe().await?;
                Ok(())
            }
            Incoming::Publish(message) => {
                Self::process_one(this, message).await?;
                Ok(())
            }
            Incoming::PubAck(_) => Ok(()),
            Incoming::PubRec(_) => {
                bail!("Unexpected pubrec");
            }
            Incoming::PubRel(_) => {
                bail!("Unexpected pubrel");
            }
            Incoming::PubComp(_) => bail!("Unexpected pubcomp"),
            Incoming::Subscribe(_) => bail!("Unexpected subscribe"),
            Incoming::SubAck(_) => Ok(()),
            Incoming::Unsubscribe(_) => bail!("Unexpected unsubscribe!"),
            Incoming::UnsubAck(_) => bail!("Unexpected unsuback!"),
            Incoming::PingReq => Ok(()),
            Incoming::PingResp => Ok(()),
            Incoming::Disconnect => Ok(()),
        };
    }

    async fn run_mqtt(this: Arc<Self>, mut ev: EventLoop) -> () {
        loop {
            let result = Self::loop_once(this.clone(), &mut ev).await;
            if !result.is_ok() {
                warn!(slog_scope::logger(), "loop_encountered_error"; "err" => format!("{}", result.unwrap_err()))
            }
        }
    }

    fn poll_device_(&self, device_id: DeviceId) -> Result<(), Box<dyn Error>> {
        let device_info = {
            self.controller.lock().unwrap().describe(device_id)?
        };
        let attributes = device_info.attributes
            .into_iter()
            .map(|x| (x.description, match x.setting_value {
                AttributeValue::NoValue => serde_json::Value::Null,
                AttributeValue::Bool(b) => serde_json::Value::Bool(b),
                AttributeValue::UInt8(i) => serde_json::Value::Number(serde_json::Number::from(i)),
            }))
            .collect::<serde_json::Map<_, _>>();

        let payload = serde_json::Value::Object(attributes).to_string();
        trace!(slog_scope::logger(), "poll_device_status"; "device_id" => device_id, "payload" => &payload);

        self.sender.try_send(Request::Publish(Publish::new(
            format!("{}{}/status", self.topic_prefix, device_id),
            rumqttc::QoS::AtLeastOnce,
            payload,
        )))?;

        Ok(())
    }

    async fn poll_device(this: Arc<Self>, device_id: DeviceId) -> () {
        let _ = tokio::task::spawn_blocking(move || {
            Self::report_async_result("poll_device", this.poll_device_(device_id))
        }).await;
    }

    async fn poll_all_(this: Arc<Self>) -> Result<(), Box<dyn Error>> {
        let that = this.clone();
        let all_devices = tokio::task::spawn_blocking(move || -> Result<_, SimpleError> {
            match that.controller.lock().unwrap().list() {
                Ok(v) => Ok(v),
                Err(e) => bail!("{}", e)
            }
        }).await??;

        let all_tasks = all_devices
            .into_iter()
            .map(|x| {
                Self::poll_device(this.clone(), x.id)
            })
            .collect::<Vec<_>>();
        for task in all_tasks {
            task.await
        };
        Ok(())
    }

    async fn poll_all(this: Arc<Self>) -> () {
        Self::report_async_result("poll_all", Self::poll_all_(this).await)
    }

    async fn run_poller(this: Arc<Self>, rx: Receiver<DeviceId>) -> () {
        let mut timer = tokio::time::interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                _ = timer.tick() => {
                    Self::poll_all(this.clone()).await;
                },
                device_id = rx.recv() => {
                    Self::poll_device(this.clone(), device_id.unwrap()).await;
                }
            };
        }
    }

    async fn broadcast_device_discovery(this: Arc<Self>, id: DeviceId) -> Result<(), Box<dyn Error>> {
        debug!(slog_scope::logger(), "broadcast_discovery"; "id" => id);

        let that = this.clone();
        let device = tokio::task::spawn_blocking(move || -> Result<LongDevice, SimpleError> {
            match that.controller.lock().unwrap().describe(id) {
                Ok(v) => Ok(v),
                Err(v) => bail!("{}", v),
            }
        }).await??;

        match device_to_discovery_payload(&this.topic_prefix, &device) {
            Some(v) => {
                let topic = format!("{}{}/wink/{}/config", this.discovery_prefix.as_ref().unwrap(), v.component, device.id);
                let config = v.discovery_info.to_string();
                debug!(slog_scope::logger(), "broadcast_discovery_result"; "id" => id, "topic" => &topic, "config" => &config);
                this.sender.send(Request::Publish(Publish::new(
                    topic,
                    rumqttc::QoS::AtLeastOnce,
                    config,
                ))).await?;
                Ok(())
            },
            None => {
                warn!(slog_scope::logger(), "unknown_device"; "device_id" => id, "device_info" => format!("{:?}", device));
                Ok(())
            }
        }
    }

    async fn broadcast_discovery_(this: Arc<Self>) -> Result<(), Box<dyn Error>> {
        let that = this.clone();
        let devices = tokio::task::spawn_blocking(move || -> Result<Vec<ShortDevice>, SimpleError> {
            match that.controller.lock().unwrap().list() {
                Ok(v) => Ok(v),
                Err(v) => bail!("{}", v),
            }
        }).await??;

        let futures = devices
            .into_iter()
            .map(|d| Self::broadcast_device_discovery(this.clone(), d.id))
            .collect::<Vec<_>>();
        for f in futures {
            f.await?;
        };
        Ok(())
    }

    async fn broadcast_discovery(this: Arc<Self>) -> () {
        Self::report_async_result("broadcast_discovery", Self::broadcast_discovery_(this).await)
    }
}
