use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, OnceLock};
use std::thread;
use std::net::SocketAddr;
use tokio::sync::broadcast::{Receiver, Sender};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error};
use rumqttd::{AdminLink, Broker, ClientStatus};

use crate::db::DB;
use crate::mqtt::config::{get_default_config, MqttConfig};
use crate::mqtt::messages::{MqttMessage, MqttSender, MqttCommand};
use crate::mqtt::handlers::{start_event_handlers, ServerLinks};
use crate::mqtt::auth::auth;

pub static GLOBAL_DB: OnceLock<Arc<DB>> = OnceLock::new();

pub struct MqttServerMetrics {
    pub messages_forwarded: AtomicU64,
    pub messages_sent: AtomicU64,
    pub messages_dropped: AtomicU64,
}

pub struct MqttServer {
    pub mqtt: MqttSender,
    pub admin: Option<AdminLink>,
    pub controller: rumqttd::BrokerController,
    receiver: flume::Receiver<MqttMessage>,
    pub cancel_token: CancellationToken,
    pub metrics: Arc<MqttServerMetrics>,
    connection_monitor_tx: Sender<ClientStatus>,
    pub shutting_down: Arc<AtomicBool>,
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

pub async fn start_broker(mqtt_config: Option<MqttConfig>, db: Arc<DB>) -> MqttServer {
    // Initialize the global DB for the auth handler
    let _ = GLOBAL_DB.set(db);

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
    } else {
        let ws = config.ws.as_mut();
        if let Some(ws) = ws {
            ws.remove("1");
        }
    }

    let mut broker = Broker::new(config);

    let (link_tx, link_rx, router_tx, connection_monitor_tx, connection_id) =
        broker.get_broker_links().unwrap();
    let admin_link = broker.admin_link("forest_admin", 200).unwrap();
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
    
    let controller = broker.controller();
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
        controller,
        receiver: message_receiver,
        cancel_token: cancel_token.clone(),
        metrics: metrics,
        connection_monitor_tx: connection_monitor_tx,
        shutting_down: Arc::new(AtomicBool::new(false)),
    };

    return mqtt_server;
}

