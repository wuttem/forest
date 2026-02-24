extern crate forest;

use std::future::Future;
use std::pin::Pin;

use forest::db::DB;
use forest::mqtt::MqttSender;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let db = Arc::new(
        DB::open_default("sqlite:file:memdb_mqtt_example?mode=memory&cache=shared")
            .await
            .unwrap(),
    );
    let mut mqtt_server = forest::mqtt::start_broker(None, db).await;

    // Example: create a message channel and receive messages
    let receiver = mqtt_server.message_receiver();
    let back_channel = mqtt_server.mqtt.clone();
    tokio::spawn(async move {
        while let Ok(message) = receiver.recv_async().await {
            on_message(message.topic, message.payload, back_channel.clone()).await;
        }
    });

    // Example: Subscribe to a topic
    let publish_channel = mqtt_server.mqtt.clone();
    publish_channel
        .subscribe("things/#".to_string())
        .await
        .unwrap();

    tokio::spawn(async move {
        loop {
            publish_channel
                .publish(
                    "things/testdevice0/shadow/update".to_string(),
                    "Ping!".as_bytes().to_vec(),
                )
                .await
                .unwrap();
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
        }
    });

    let _ = tokio::signal::ctrl_c().await;
    mqtt_server.shutdown();
}

pub fn on_message(
    topic: String,
    _payload: Vec<u8>,
    mqtt_sender: MqttSender,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        println!("Received message on topic: {}", topic);
        println!("Payload: {:?}", _payload);
        // Example: Send a message back to the device
        let return_topic = topic.replace("shadow/update", "shadow/update/accepted");
        let return_payload = "Hello, World!".as_bytes().to_vec();
        mqtt_sender.publish(return_topic, return_payload).await.unwrap();
        println!("Sent response to device");
    })
}
