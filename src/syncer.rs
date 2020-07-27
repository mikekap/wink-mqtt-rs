use mqtt_async_client::client::{
    Client
};
use crate::controller::DeviceController;

struct DeviceSyncer {
    controller: Box<dyn DeviceController>,
    mqtt_client: Client,
    topic_prefix: String,
}

impl DeviceSyncer {
    fn new(controller: Box<dyn DeviceController>, mqtt_client: Client, topic_prefix: &str) {
        let mut syncer = DeviceSyncer{
            controller,
            mqtt_client,
            topic_prefix: topic_prefix.to_string(),
        };

        // tokio::task::spawn(async move || {
        //     loop {
        //
        //     }
        // });
    }
}