use crate::config::{Config, NotInterestingTopicError, TopicType};
use crate::controller::{AttributeId, AttributeValue, DeviceController, DeviceId};
use crate::converter::device_to_discovery_payload;
use crate::utils::ResultExtensions;
use async_channel::{bounded, Receiver, Sender};
use rumqttc::{Event, EventLoop, Incoming, Publish, Request, Subscribe};
use serde_json::value::Value::Object;
use simple_error::{bail, simple_error};
use slog::{debug, error, info, trace, warn};
use slog_scope;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tokio::time::Duration;

pub struct DeviceSyncer {
    config: Config,
    controller: Arc<dyn DeviceController>,
    sender: Sender<Request>,
    repoll: Sender<DeviceId>,
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

        if this.config.discovery_topic_prefix.is_some() {
            tokio::task::spawn({
                let this = this.clone();
                async move { this.broadcast_discovery().await }
            });
        }
        this
    }

    async fn do_subscribe(&self) -> Result<(), Box<dyn Error>> {
        let subscribed: Vec<_> = self
            .config
            .mqtt_topic_subscribe_patterns()
            .map(|topic| {
                self.sender.send(Request::Subscribe(Subscribe::new(
                    topic,
                    rumqttc::QoS::AtLeastOnce,
                )))
            })
            .collect();
        for sub in subscribed {
            sub.await?;
        }

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
                self.broadcast_discovery_().await?;
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

    async fn loop_once(self: Arc<Self>, ev: &mut EventLoop) -> Result<(), Box<dyn Error>> {
        let message = match ev.poll().await? {
            Event::Incoming(i) => i,
            Event::Outgoing(_) => return Ok(()),
        };

        trace!(slog_scope::logger(), "mqtt_message"; "message" => ?message);

        return match message {
            Incoming::Connect(_) => Ok(()),
            Incoming::ConnAck(_) => {
                self.do_subscribe().await?;
                Ok(())
            }
            Incoming::Publish(message) => {
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
            Incoming::Disconnect => Ok(()),
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

    async fn poll_device_(&self, device_id: DeviceId) -> Result<(), Box<dyn Error>> {
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

        let mut publish = Publish::new(
            self.config
                .to_topic_string(&TopicType::StatusTopic(device_id))
                .unwrap(),
            rumqttc::QoS::AtLeastOnce,
            payload,
        );
        publish.retain = true;
        self.sender.try_send(Request::Publish(publish))?;

        Ok(())
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
        for task in all_tasks {
            task.await
        }
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
                self.sender
                    .send(Request::Publish(Publish::new(
                        topic,
                        rumqttc::QoS::AtLeastOnce,
                        config,
                    )))
                    .await?;
                Ok(())
            }
            None => {
                warn!(slog_scope::logger(), "unknown_device"; "device_id" => id, "device_info" => ?device);
                Ok(())
            }
        }
    }

    async fn broadcast_discovery_(self: Arc<Self>) -> Result<(), Box<dyn Error>> {
        let devices = self.controller.list().await?;

        let futures = devices
            .into_iter()
            .map(|d| self.clone().broadcast_device_discovery(d.id))
            .collect::<Vec<_>>();
        for f in futures {
            f.await?;
        }
        Ok(())
    }

    async fn broadcast_discovery(self: Arc<Self>) -> () {
        self.broadcast_discovery_()
            .await
            .log_failing_result("broadcast_discovery_failed");
    }
}
