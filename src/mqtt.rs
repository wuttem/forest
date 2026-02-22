pub use rumqttd::local::{LinkError, LinkRx, LinkTx};
use rumqttd::meters::MetersLink;
use rumqttd::Meter::Router;
use rumqttd::{alerts::AlertsLink, ConnectionId};
pub use rumqttd::{Alert, AuthHandler, Broker, ClientStatus, Config, Meter, Notification, ClientInfo, AdminLink};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::thread;
use std::{future::Future, sync::atomic::AtomicBool};
use tokio::select;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info_span, warn, info};

pub const DEFAULT_CONFIG: &str = r#"{
  "id": 0,
  "metrics": {
    "alerts": {
      "push_interval": 10
    },
    "meters": {
      "push_interval": 10
    }
  },
  "router": {
    "id": 0,
    "max_connections": 10010,
    "max_outgoing_packet_count": 200,
    "max_segment_count": 10,
    "max_segment_size": 104857600
  },
  "v4": {
    "1": {
      "listen": "127.0.0.1:1883",
      "name": "v4-1",
      "next_connection_delay_ms": 1,
      "connections": {
        "connection_timeout_ms": 30000,
        "dynamic_filters": true,
        "max_inflight_count": 500,
        "max_payload_size": 128000
      }
    }
  },
  "v5": {
    "1": {
      "listen": "127.0.0.1:1884",
      "name": "v5-1",
      "next_connection_delay_ms": 1,
      "connections": {
        "connection_timeout_ms": 30000,
        "max_inflight_count": 500,
        "max_payload_size": 128000
      }
    }
  },
  "ws": {
    "1": {
      "listen": "127.0.0.1:1885",
      "name": "ws-1",
      "next_connection_delay_ms": 1,
      "connections": {
        "connection_timeout_ms": 30000,
        "max_inflight_count": 500,
        "max_payload_size": 128000
      }
    }
  }
}"#;

use thiserror::Error;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MqttConfig {
    pub enable_heartbeat: bool,
    pub enable_ssl: bool,
    pub ssl_ca_path: Option<String>,
    pub ssl_cert_path: Option<String>,
    pub ssl_key_path: Option<String>,
    pub max_connections: usize,
    pub bind_v3: String,
    pub bind_v5: String,
    pub bind_ws: Option<String>,
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            enable_heartbeat: true,
            enable_ssl: false,
            ssl_ca_path: None,
            ssl_cert_path: None,
            ssl_key_path: None,
            max_connections: 10000,
            bind_v3: "127.0.0.1:1883".to_string(),
            bind_v5: "127.0.0.1:1884".to_string(),
            bind_ws: None,
        }
    }
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

fn get_default_config() -> Config {
    let config = serde_json::from_str(DEFAULT_CONFIG).unwrap();
    return config;
}

pub type AsyncMessageCallback = Arc<
    dyn Fn(String, Vec<u8>, MqttSender) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync,
>;

#[derive(Clone)]
pub struct MqttSender {
    connection_id: ConnectionId,
    channel: flume::Sender<MqttCommand>,
    router_tx: flume::Sender<(ConnectionId, rumqttd::Event)>,
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

struct ServerLinks {
    tx_link: Option<LinkTx>,
    rx_link: Option<LinkRx>,
    alerts: Option<AlertsLink>,
    metrics: Option<MetersLink>,
    publish_receiver: flume::Receiver<MqttCommand>,
    publish_sender: MqttSender,
    enable_heartbeat: bool,
    message_sender: flume::Sender<MqttMessage>,
}

pub struct MqttServerMetrics {
    pub messages_forwarded: AtomicU64,
    pub messages_sent: AtomicU64,
    pub messages_dropped: AtomicU64,
}

pub struct MqttServer {
    pub mqtt: MqttSender,
    pub admin: Option<AdminLink>,
    receiver: flume::Receiver<MqttMessage>,
    pub cancel_token: CancellationToken,
    pub metrics: Arc<MqttServerMetrics>,
    connection_monitor_tx: Sender<ClientStatus>,
    pub shutting_down: Arc<AtomicBool>,
}

fn handle_meter(meters: Vec<Meter>) {
    for meter in meters {
        match meter {
            Router(_s, r) => {
                debug!("Router Meter {}: {:?}", r.sequence, r);
            }
            _ => {}
        }
    }
}

fn handle_alert(alerts: Vec<Alert>) {
    for alert in alerts {
        warn!("Alert: {:?}", alert);
    }
}

impl MqttServer {
    pub fn message_receiver(&mut self) -> flume::Receiver<MqttMessage> {
        return self.receiver.clone();
    }

    pub fn shutdown(&mut self) {
        self.shutting_down
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.cancel_token.cancel();
    }

    pub fn get_cancel_token(&self) -> CancellationToken {
        return self.cancel_token.clone();
    }

    pub fn connection_monitor_subscribe(&self) -> Receiver<ClientStatus> {
        return self.connection_monitor_tx.subscribe();
    }
}

async fn mqtt_send_handler(
    mut tx_link: LinkTx,
    publish_receiver: flume::Receiver<MqttCommand>,
    metrics: &Arc<MqttServerMetrics>,
) {
    while let Ok(message) = publish_receiver.recv() {
        match message {
            MqttCommand::Publish(message) => {
                let r = tx_link.publish(message.topic, message.payload);
                if let Err(e) = r {
                    error!(error=?e, "Error publishing message");
                } else {
                    metrics
                        .messages_sent
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            MqttCommand::Subscribe(topic) => {
                let r = tx_link.subscribe(&topic);
                if let Err(e) = r {
                    error!(error=?e, "Error subscribing to topic");
                }
            }
            MqttCommand::Unsubscribe(_topic) => {
                error!("Unsubscribe not supported");
            }
        }
    }
    info!("mqtt_send_handler stopped");
}

async fn mqtt_message_handler(
    mut rx_link: LinkRx,
    message_forward: flume::Sender<MqttMessage>,
    metrics: &Arc<MqttServerMetrics>,
) {
    while let Ok(next_notification) = rx_link.next().await {
        if let Some(notification) = next_notification {
            match notification {
                Notification::Forward(forward) => {
                    if let Ok(topic) = std::str::from_utf8(&forward.publish.topic) {
                        let payload = forward.publish.payload.to_vec();
                        let res = message_forward.try_send(MqttMessage {
                            topic: topic.to_string(),
                            payload: payload.clone(),
                        });
                        if let Err(_) = res {
                            metrics
                                .messages_dropped
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            warn!("Message Dropped");
                            // TODO - figure out how to buffer messages
                        } else {
                            metrics
                                .messages_forwarded
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                }
                _ => continue,
            }
        }
    }
    info!("mqtt_message_handler stopped");
}

async fn alert_handler(alerts: AlertsLink) {
    while let Ok(alert) = alerts.next().await {
        handle_alert(alert);
    }
    info!("alert_handler stopped");
}

async fn meter_handler(metrics: MetersLink) {
    while let Ok(metric) = metrics.next().await {
        handle_meter(metric);
    }
    info!("meter_handler stopped");
}

async fn heartbeat_task(publish_channel: MqttSender) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let now = chrono::Utc::now().timestamp() as u64;
        let payload = format!("{{\"ts\":{}}}", now).into_bytes();
        if let Err(e) = publish_channel.publish("public/heartbeat".to_string(), payload) {
            error!(error=?e, "Error sending heartbeat");
            break;
        } else {
            debug!("Heartbeat sent");
        }
    }
    info!("heartbeat_task stopped");
}

async fn start_event_handlers(
    mut links: ServerLinks,
    metrics: &Arc<MqttServerMetrics>,
    token: CancellationToken,
) -> Result<(), MqttError> {
    let enable_heartbeat = links.enable_heartbeat;

    let mut set = JoinSet::new();

    let _rx_handle = {
        let rx_link = std::mem::replace(&mut links.rx_link, None).expect("No rx_link available");
        let metric_clone = metrics.clone();
        set.spawn(async move {
            let message_forward = links.message_sender;
            mqtt_message_handler(rx_link, message_forward, &metric_clone).await;
        })
    };

    let _publish_handle = {
        let tx_link = std::mem::replace(&mut links.tx_link, None).expect("No tx_link available");
        let metric_clone = metrics.clone();
        set.spawn(async move {
            mqtt_send_handler(tx_link, links.publish_receiver, &metric_clone).await;
        })
    };

    let _alerts_handle = {
        let alerts = std::mem::replace(&mut links.alerts, None).expect("No alerts link available");
        set.spawn(async move {
            alert_handler(alerts).await;
        })
    };

    let _metrics_handle = {
        let metrics =
            std::mem::replace(&mut links.metrics, None).expect("No metrics link available");
        set.spawn(async move {
            meter_handler(metrics).await;
        })
    };

    let _heartbeat_handle = if enable_heartbeat {
        let publish_channel = links.publish_sender.clone();
        Some(set.spawn(async move {
            heartbeat_task(publish_channel).await;
        }))
    } else {
        None
    };

    // Monitor tasks - panic if any completes
    loop {
        select! {
            _ = set.join_next() => {
                if token.is_cancelled() {
                    return Ok(())
                } else {
                    token.cancel();
                }
                return Err(MqttError::TaskExitError("Critical background task completed unexpectedly".to_string()));
            }
            _ = token.cancelled() => {
                return Ok(())
            }
        }
    }
}

async fn auth(
    client_id: String,
    username: String,
    _password: String,
    common_name: String,
    organization: String,
    ca_path: Option<String>,
) -> Result<Option<ClientInfo>, String> {
    let _span = info_span!("authentication", client_id = %client_id, username = %username, common_name = %common_name, organization = %organization, ca_path = ?ca_path).entered();
    // we can do auth on username and password or on common_name (from client certificate)
    // if we have a common_name we need to check that it matches the client_id
    if !common_name.is_empty() && client_id != common_name {
        warn!("Client ID does not match certificate common name");
        return Ok(None);
    }

    Ok(Some(ClientInfo {
        client_id,
        tenant: None, // Or however you determine tenant ID (e.g. from organization)
    }))
}

pub async fn start_broker(mqtt_config: Option<MqttConfig>) -> MqttServer {
    let mut config = get_default_config();

    let mqtt_config = match mqtt_config {
        Some(c) => c,
        None => MqttConfig::default(),
    };

    let server_v3 = config.v4.as_mut().and_then(|v4| v4.get_mut("1")).unwrap();
    let server_v5 = config.v5.as_mut().and_then(|v5| v5.get_mut("1")).unwrap();

    //  Apply mqtt_config to config
    config.router.max_connections = mqtt_config.max_connections;
    if mqtt_config.enable_ssl {
        // check that we have all the required paths
        if mqtt_config.ssl_ca_path.is_none()
            || mqtt_config.ssl_cert_path.is_none()
            || mqtt_config.ssl_key_path.is_none()
        {
            error!("Missing required SSL paths");
            panic!("Missing required SSL paths");
        }
        server_v3.tls = Some(rumqttd::TlsConfig::Rustls {
            capath: mqtt_config.ssl_ca_path.to_owned(),
            certpath: mqtt_config.ssl_cert_path.to_owned().unwrap(),
            keypath: mqtt_config.ssl_key_path.to_owned().unwrap(),
        });
        server_v5.tls = Some(rumqttd::TlsConfig::Rustls {
            capath: mqtt_config.ssl_ca_path.to_owned(),
            certpath: mqtt_config.ssl_cert_path.to_owned().unwrap(),
            keypath: mqtt_config.ssl_key_path.to_owned().unwrap(),
        });
    }

    let v3_socket_addr: SocketAddr = mqtt_config
        .bind_v3
        .parse()
        .expect("Invalid v3_listen address");
    server_v3.listen = v3_socket_addr;
    let v5_socket_addr: SocketAddr = mqtt_config
        .bind_v5
        .parse()
        .expect("Invalid v5_listen address");
    server_v5.listen = v5_socket_addr;

    server_v3.set_auth_handler(auth);
    server_v5.set_auth_handler(auth);

    //  Enable or disable websockets
    if let Some(ws) = mqtt_config.bind_ws {
        let ws_socket_addr: SocketAddr = ws.parse().expect("Invalid ws_listen address");
        let ws_server = config.ws.as_mut().and_then(|ws| ws.get_mut("1")).unwrap();
        ws_server.listen = ws_socket_addr;
        if mqtt_config.enable_ssl {
            ws_server.tls = Some(rumqttd::TlsConfig::Rustls {
                capath: mqtt_config.ssl_ca_path.to_owned(),
                certpath: mqtt_config.ssl_cert_path.to_owned().unwrap(),
                keypath: mqtt_config.ssl_key_path.to_owned().unwrap(),
            });
        }
        ws_server.set_auth_handler(auth);
    }
    else {
        let ws = config.ws.as_mut();
        if let Some(ws) = ws {
            ws.remove("1");
        }
    }

    let mut broker = Broker::new(config);

    let (link_tx, link_rx, router_tx, connection_monitor_tx, connection_id) =
        broker.get_broker_links().unwrap();
    let admin_link = broker.admin_link("forest_admin").unwrap();
    let alerts = broker.alerts().unwrap();
    let metrics = broker.meters().unwrap();
    let (tx, rx) = flume::bounded::<MqttCommand>(400);

    let sender = MqttSender {
        channel: tx,
        connection_id: connection_id,
        router_tx: router_tx,
    };

    let (message_sender, message_receiver) = flume::bounded(200);

    let enable_heartbeat = mqtt_config.enable_heartbeat;
    let links = ServerLinks {
        tx_link: Some(link_tx),
        rx_link: Some(link_rx),
        alerts: Some(alerts),
        metrics: Some(metrics),
        publish_sender: sender.clone(),
        publish_receiver: rx,
        enable_heartbeat: enable_heartbeat,
        message_sender: message_sender,
    };

    // We use this cancel token to signal the broker to shutdown
    let cancel_token = CancellationToken::new();
    // Oneshot Shutdown signal
    // let (main_sd_s, main_sd_r) = tokio::sync::oneshot::channel::<usize>();
    let main_cancel_token = cancel_token.clone();
    let _main_thread_handle = thread::spawn(move || {
        broker.start().unwrap();
        main_cancel_token.cancel();
        // let _ = main_sd_s.send(0);
    });

    // Do this to subscribe to all topics
    // sender.subscribe("#".to_string()).await.unwrap();

    // Create Metrics
    let metrics = Arc::new(MqttServerMetrics {
        messages_forwarded: AtomicU64::new(0),
        messages_sent: AtomicU64::new(0),
        messages_dropped: AtomicU64::new(0),
    });

    // onshot channel for shutdown signal
    // let (background_sd_s, background_sd_r) = tokio::sync::oneshot::channel::<usize>();

    let background_cancel_token = cancel_token.clone();
    let _metrics_ref = metrics.clone();
    tokio::spawn(async move {
        let res = start_event_handlers(links, &_metrics_ref, background_cancel_token).await;
        match res {
            Ok(_) => {
                debug!("MQTT Event Handlers Stopped");
                // let _ = background_sd_s.send(0);
            }
            Err(e) => {
                error!(error=?e, "MQTT Event Handlers Exited Unexpectedly");
                // let _ = background_sd_s.send(1);
            }
        }
    });

    let mqtt_server = MqttServer {
        mqtt: sender.clone(),
        admin: Some(admin_link),
        receiver: message_receiver,
        cancel_token: cancel_token.clone(),
        metrics: metrics,
        connection_monitor_tx: connection_monitor_tx,
        shutting_down: Arc::new(AtomicBool::new(false)),
    };

    return mqtt_server;
}

#[cfg(test)]
mod tests;
