use forest::config::ForestConfig;
use forest::server::start_server;

#[tokio::main]
async fn main() {
    let mut config = ForestConfig::default();

    // Create temporary directories to avoid permission issues during testing
    let cert_dir = "/tmp/forest_certs".to_string();
    std::fs::create_dir_all(&cert_dir).unwrap();
    config.cert_dir = cert_dir;

    // Use an in-memory database
    config.database.path = "sqlite:file:memdb_example?mode=memory&cache=shared".to_string();

    // Enable authentication and SSL required pathways as examples
    config.mqtt.enable_ssl = false; // set to true if testing with local certificates
    config.mqtt.bind_v3 = "127.0.0.1:1883".to_string();

    println!("Starting Forest Platform Auth/Admin Example...");
    println!("MQTT Broker listening on {}", config.mqtt.bind_v3);

    // Start the server with the setup
    let (cancel_token, server_handle) = start_server(&config).await;

    // Wait for the server to finish
    cancel_token.cancelled().await;
    let _ = server_handle.await;
}
