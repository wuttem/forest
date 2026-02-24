use rumqttd::local::LinkError;
use rumqttd::ConnectionId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tracing::{error, warn};

#[derive(Clone)]
pub struct MqttMessage {
    pub topic: String,
    pub payload: Vec<u8>,
}

pub enum MqttCommand {
    Publish(MqttMessage),
    Subscribe(String),
    Unsubscribe(String),
}
#[derive(Error, Debug)]
pub enum MqttError {
    #[error("Mqtt Link Error: {0}")]
    LinkError(#[from] LinkError),
    #[error("Mqtt Send Error: {0}")]
    SendError(#[from] flume::SendError<MqttCommand>),
    #[error("Mqtt Task Exit Error: {0}")]
    TaskExitError(String),
    #[error("Mqtt Unsupported: {0}")]
    UnsupportedError(String),
}

pub type AsyncMessageCallback = Arc<
    dyn Fn(String, Vec<u8>, MqttSender) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

#[derive(Clone)]
pub struct MqttSender {
    pub(crate) connection_id: ConnectionId,
    pub(crate) channel: flume::Sender<MqttCommand>,
    pub(crate) router_tx: flume::Sender<(ConnectionId, rumqttd::Event)>,
}

impl MqttSender {
    pub fn publish(&self, topic: String, payload: Vec<u8>) -> Result<(), MqttError> {
        self.channel.send(MqttCommand::Publish(MqttMessage {
            topic: topic,
            payload: payload,
        }))?;
        Ok(())
    }

    pub async fn subscribe(&self, topic: String) -> Result<(), MqttError> {
        self.channel.send(MqttCommand::Subscribe(topic))?;
        Ok(())
    }

    pub async fn unsubscribe(&self, _topic: String) -> Result<(), MqttError> {
        warn!("Unsubscribe not supported");
        Ok(())
        // self.channel.send(
        //     MqttCommand::Unsubscribe(topic)
        // ).await?;
        // Ok(())
    }

    pub fn print_status(&self) {
        let event = rumqttd::Event::PrintStatus(rumqttd::Print::Subscriptions);
        let message = (self.connection_id, event);
        if self.router_tx.send(message).is_err() {
            error!("Error sending status request");
        }
    }
}
