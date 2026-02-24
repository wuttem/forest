use futures_util::stream::StreamExt;
use rumqttd::local::{LinkRx, LinkTx};
use rumqttd::Meter::Router;
use rumqttd::{alerts::AlertsLink, meters::MetersLink, Alert, Meter, Notification};
use std::sync::Arc;
use tokio::select;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::mqtt::messages::{MqttCommand, MqttError, MqttMessage, MqttSender};
use crate::mqtt::server::MqttServerMetrics;

pub(crate) struct ServerLinks {
    pub(crate) tx_link: Option<LinkTx>,
    pub(crate) rx_link: Option<LinkRx>,
    pub(crate) alerts: Option<AlertsLink>,
    pub(crate) metrics: Option<MetersLink>,
    pub(crate) publish_receiver: flume::Receiver<MqttCommand>,
    pub(crate) publish_sender: MqttSender,
    pub(crate) enable_heartbeat: bool,
    pub(crate) message_sender: flume::Sender<MqttMessage>,
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
async fn mqtt_send_handler(
    mut tx_link: LinkTx,
    publish_receiver: flume::Receiver<MqttCommand>,
    metrics: &Arc<MqttServerMetrics>,
) {
    while let Ok(message) = publish_receiver.recv_async().await {
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

pub(crate) async fn start_event_handlers(
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
