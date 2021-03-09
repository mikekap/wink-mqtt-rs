use crate::config::{Config, NotInterestingTopicError, TopicType};
use crate::controller::{AttributeId, DeviceController, DeviceId};
use crate::converter::device_to_discovery_payload;
use crate::utils::ResultExtensions;
use async_channel::{bounded, Receiver, Sender};
use futures::future::join_all;
use rumqttc::{Event, EventLoop, Incoming, Publish, Request, Subscribe};
use serde::{Serialize, Serializer};
use serde_json::value::Value::Object;
use simple_error::{bail, simple_error};
use slog::{crit, debug, error, info, trace, warn};
use slog_scope;
use std::collections::{HashMap, VecDeque};
use std::error::Error;
use std::future::Future;
use std::ops::Deref;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::Duration;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaybeJsonString {
    pub byte_contents: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum LoggedMessage {
    OutgoingMessage(String, MaybeJsonString),
    IncomingMessage(String, MaybeJsonString),
    Connected,
    Disconnected,
}

impl MaybeJsonString {
    pub fn new<P: Clone + Into<Vec<u8>>>(bytes: &P) -> MaybeJsonString {
        MaybeJsonString {
            byte_contents: bytes.clone().into(),
        }
    }
}

impl Serialize for MaybeJsonString {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        let str = match std::str::from_utf8(&self.byte_contents) {
            Ok(v) => v,
            Err(_) => return serializer.serialize_bytes(&self.byte_contents),
        };
        match serde_json::from_str(str) {
            Ok(Object(m)) => m.serialize(serializer),
            _ => serializer.serialize_str(str),
        }
    }
}

pub struct DeviceSyncer {
    config: Config,
    controller: Arc<dyn DeviceController>,
    sender: Sender<Request>,
    repoll: Sender<DeviceId>,
    pub last_n_messages: Mutex<VecDeque<LoggedMessage>>,
}

impl<'a> DeviceSyncer {
    pub fn new(config: &Config, controller: Arc<dyn DeviceController>) -> Arc<DeviceSyncer> {
        let mut options = config.mqtt_options.as_ref().unwrap().clone();
        info!(slog_scope::logger(), "opening_client"; "host" => options.broker_address().0, "port" => options.broker_address().1, "client_id" => &options.client_id());
        options.set_clean_session(true);
        let ev = EventLoop::new(options, 100);
        let (repoll_sender, repoll_rx) = bounded(10);
        let syncer = DeviceSyncer {
            config: config.clone(),
            controller,
            sender: ev.handle(),
            repoll: repoll_sender,
            last_n_messages: Mutex::new(VecDeque::with_capacity(10)),
        };
        let this = Arc::new(syncer);
        trace!(slog_scope::logger(), "start_thread");
        tokio::task::spawn({
            let this = this.clone();
            async move { this.run_mqtt(ev).await }
        });

        tokio::task::spawn({
            let this = this.clone();
            async move {
                this.clone()
                    .run_poller(this.clone().config.resync_interval, repoll_rx)
                    .await
            }
        });
        this
    }

    async fn start_broadcast_discovery_broadcast(self: Arc<Self>) {
        if self.config.discovery_topic_prefix.is_some() {
            tokio::task::spawn({
                let this = self.clone();
                async move { this.broadcast_discovery().await }
            });
        }
    }

    async fn do_subscribe(&self) -> Result<(), Box<dyn Error>> {
        join_all(self.config.mqtt_topic_subscribe_patterns().map(|topic| {
            self.sender.send(Request::Subscribe(Subscribe::new(
                topic,
                rumqttc::QoS::AtLeastOnce,
            )))
        }))
        .await
        .into_iter()
        .collect::<Result<Vec<()>, rumqttc::SendError<rumqttc::Request>>>()?;

        self.repoll.send(0).await?;

        Ok(())
    }

    async fn process_one(self: Arc<Self>, message: Publish) -> Result<(), Box<dyn Error>> {
        let topic = {
            let result = self.config.parse_mqtt_topic(&message.topic);

            if result
                .as_ref()
                .err()
                .and_then(|x| x.downcast_ref::<NotInterestingTopicError>())
                .is_some()
            {
                return Ok(());
            }
            result?
        };

        match topic {
            TopicType::SetJsonTopic(device_id) => {
                self.set_device_attributes_json(device_id, &message.payload)
                    .await?;
            }
            TopicType::SetAttributeTopic(device_id, attribute_id) => {
                self.set_device_attribute_by_id(device_id, attribute_id, &message.payload)
                    .await?;
            }
            TopicType::DiscoveryListenTopic() => {
                self.broadcast_discovery().await;
            }
            TopicType::StatusTopic(_) | TopicType::DiscoveryTopic(_, _) => {
                // Don't need to do anything here; we really shouldn't get here though...
                warn!(slog_scope::logger(), "unexpected_topic_seen"; "topic" => message.topic);
            }
        }

        Ok(())
    }

    async fn set_device_attribute_by_id(
        &self,
        device_id: DeviceId,
        attribute_id: AttributeId,
        payload: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        let (device_name, attribute) = {
            let info = self.controller.describe(device_id).await?;
            (
                info.name,
                info.attributes
                    .into_iter()
                    .find(|x| x.id == attribute_id)
                    .ok_or_else(|| {
                        simple_error!(
                            "Couldn't find attribute with id {} on device {}",
                            attribute_id,
                            device_id
                        )
                    })?,
            )
        };
        if !attribute.supports_write {
            bail!("Attribute {} does not support write", attribute.description);
        };

        let payload_str = std::str::from_utf8(payload)?;
        let value = attribute.attribute_type.parse(payload_str)?;

        self.controller.set(device_id, attribute_id, &value).await?;
        info!(slog_scope::logger(), "set"; "device_id" => device_id, "device" => &device_name, "attribute" => &attribute.description, "value" => ?value);

        self.repoll.try_send(device_id)?;

        Ok(())
    }

    async fn set_device_attributes_json(
        &self,
        device_id: DeviceId,
        payload: &[u8],
    ) -> Result<(), Box<dyn Error>> {
        let input = std::str::from_utf8(&payload)?;
        debug!(slog_scope::logger(), "json_message"; "device_id" => device_id, "payload" => &input);

        let value = match serde_json::from_str(input)? {
            Object(map) => map,
            _ => bail!("Input to set not a map: {}", input),
        };

        let controller = &self.controller;

        let (device_name, attribute_names) = {
            let info = controller.describe(device_id).await?;
            (
                info.name,
                info.attributes
                    .into_iter()
                    .map(|item| (item.description.to_string(), item))
                    .collect::<HashMap<_, _>>(),
            )
        };

        for (k, v) in value.iter() {
            let attribute = match attribute_names.get(k) {
                Some(v) => {
                    if !v.supports_write {
                        error!(
                            slog_scope::logger(),
                            "read_only_attribute"; "attribute" => &v.description
                        );
                        continue;
                    }
                    v
                }
                _ => {
                    error!(slog_scope::logger(), "not_found_attribute"; "name" => &k);
                    continue;
                }
            };

            let value = match attribute.attribute_type.parse_json(v) {
                Ok(v) => v,
                Err(e) => {
                    error!(slog_scope::logger(), "bad_setting_for_attribute"; "attribute" => &attribute.description, "value" => %v, "error" => ?e);
                    continue;
                }
            };

            info!(slog_scope::logger(), "set"; "device_id" => device_id, "device" => &device_name, "attribute" => k, "value" => ?value);
            controller.set(device_id, attribute.id, &value).await?
        }

        self.repoll.try_send(device_id)?;

        Ok(())
    }

    async fn log_message(self: Arc<Self>, message: LoggedMessage) {
        let mut msgs = self.last_n_messages.lock().await;
        if msgs.len() == 10 {
            msgs.pop_front();
        };
        msgs.push_back(message)
    }

    async fn loop_once(self: Arc<Self>, ev: &mut EventLoop) -> Result<(), Box<dyn Error>> {
        let message = match ev.poll().await? {
            Event::Incoming(i) => i,
            Event::Outgoing(_) => return Ok(()),
        };

        trace!(slog_scope::logger(), "mqtt_message"; "message" => ?message);

        return match message {
            Incoming::Connect(_) => Ok(()),
            Incoming::ConnAck(_) => {
                self.clone().log_message(LoggedMessage::Connected).await;
                self.clone().do_subscribe().await?;
                self.start_broadcast_discovery_broadcast().await;
                Ok(())
            }
            Incoming::Publish(message) => {
                self.clone()
                    .log_message(LoggedMessage::IncomingMessage(
                        message.topic.clone(),
                        MaybeJsonString::new(&message.payload.deref()),
                    ))
                    .await;
                let this = self.clone();
                tokio::task::spawn(async move {
                    this.process_one(message)
                        .await
                        .log_failing_result("process_message_failed");
                });
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
            Incoming::Disconnect => {
                self.clone().log_message(LoggedMessage::Disconnected).await;
                Ok(())
            }
        };
    }

    async fn run_mqtt(self: Arc<Self>, mut ev: EventLoop) -> () {
        loop {
            let should_delay = {
                let result = self.clone().loop_once(&mut ev).await;
                match result {
                    Ok(_) => false,
                    Err(e) => {
                        warn!(slog_scope::logger(), "loop_encountered_error"; "err" => ?e);
                        true
                    }
                }
            };
            if should_delay {
                tokio::time::delay_for(Duration::from_millis(200)).await
            };
        }
    }

    async fn poll_device_(self: Arc<Self>, device_id: DeviceId) -> Result<(), Box<dyn Error>> {
        let device_info = { self.controller.describe(device_id).await? };
        let attributes = device_info
            .attributes
            .into_iter()
            .map(|x| {
                (
                    x.description,
                    x.setting_value.or(&x.current_value).to_json(),
                )
            })
            .collect::<serde_json::Map<_, _>>();

        let payload = serde_json::Value::Object(attributes).to_string();
        trace!(slog_scope::logger(), "poll_device_status"; "device_id" => device_id, "payload" => &payload);

        let topic = self
            .config
            .to_topic_string(&TopicType::StatusTopic(device_id))
            .unwrap();
        let logged_message =
            LoggedMessage::OutgoingMessage(topic.clone(), MaybeJsonString::new(&payload));
        let mut publish = Publish::new(topic, rumqttc::QoS::AtLeastOnce, payload);
        publish.retain = true;
        match self.sender.try_send(Request::Publish(publish)) {
            Ok(_) => {
                self.log_message(logged_message).await;
                Ok(())
            }
            Err(e) => {
                crit!(slog_scope::logger(), "sending_failed_crashing_to_maybe_reconnect"; "error" => ?e);
                panic!(e)
            }
        }
    }

    async fn poll_device(self: Arc<Self>, device_id: DeviceId) -> () {
        self.poll_device_(device_id)
            .await
            .log_failing_result("poll_device_failed");
    }

    async fn poll_all_(self: Arc<Self>) -> Result<(), Box<dyn Error>> {
        let all_devices = self.clone().controller.list().await?;
        let all_tasks = all_devices
            .into_iter()
            .map(|x| self.clone().poll_device(x.id))
            .collect::<Vec<_>>();
        join_all(all_tasks).await;
        Ok(())
    }

    async fn poll_all(self: Arc<Self>) -> () {
        self.poll_all_().await.log_failing_result("poll_all_failed");
    }

    async fn run_poller(self: Arc<Self>, resync_interval: u64, rx: Receiver<DeviceId>) -> () {
        info!(slog_scope::logger(), "poller_starting"; "resync_interval" => resync_interval);
        tokio::task::spawn({
            let that = self.clone();
            async move {
                let mut timer = tokio::time::interval(Duration::from_millis(resync_interval));
                loop {
                    timer.tick().await;
                    let _ = that.repoll.send(0).await;
                }
            }
        });
        loop {
            let device_id = rx.recv().await.unwrap();
            trace!(slog_scope::logger(), "requested_repoll"; "device_id" => device_id);
            if device_id == 0 {
                self.clone().poll_all().await;
            } else {
                self.clone().poll_device(device_id).await;
            }
        }
    }

    async fn broadcast_device_discovery(
        self: Arc<Self>,
        id: DeviceId,
    ) -> Result<(), Box<dyn Error>> {
        debug!(slog_scope::logger(), "broadcast_discovery"; "id" => id);

        let device = self.clone().controller.describe(id).await?;

        match device_to_discovery_payload(&self.config, &device) {
            Some(v) => {
                let topic = self
                    .config
                    .to_topic_string(&TopicType::DiscoveryTopic(v.component.into(), device.id))
                    .ok_or_else(|| simple_error!("No discovery topic for device {}", device.id))?;
                let config = v.discovery_info.to_string();
                info!(slog_scope::logger(), "discovered_device"; "id" => id, "name" => &device.name);
                debug!(slog_scope::logger(), "broadcast_discovery_result"; "id" => id, "topic" => &topic, "config" => &config);
                let log_message =
                    LoggedMessage::OutgoingMessage(topic.clone(), MaybeJsonString::new(&config));
                self.sender
                    .send(Request::Publish(Publish::new(
                        topic,
                        rumqttc::QoS::AtLeastOnce,
                        config,
                    )))
                    .await?;
                self.log_message(log_message).await;
                Ok(())
            }
            None => {
                warn!(slog_scope::logger(), "unknown_device"; "device_id" => id, "device_info" => ?device);
                Ok(())
            }
        }
    }

    async fn broadcast_device_discovery_quiet(self: Arc<Self>, id: DeviceId) {
        self.broadcast_device_discovery(id)
            .await
            .log_failing_result("broadcast_device_discovery_failed");
    }

    async fn broadcast_discovery(self: Arc<Self>) -> () {
        let devices = match self.controller.list().await {
            Ok(v) => v,
            Err(e) => {
                error!(slog_scope::logger(), "failed_to_list_devices"; "error" => ?e);
                return ();
            }
        };

        let futures = devices
            .into_iter()
            .map(|d| self.clone().broadcast_device_discovery_quiet(d.id))
            .collect::<Vec<_>>();
        join_all(futures).await;
    }
}
