use forest::config::ForestConfig;
use forest::server::start_server;
use reqwest::Client;
use serde_json::json;
use tokio::time::sleep;
use std::time::Duration;
use uuid::Uuid;
use forest::models::{Tenant, TenantId, AuthConfig};
use std::fs;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_full_user_management_flow() {
    let db_id = Uuid::new_v4().simple();
    
    // 1. Setup custom config
    let mut config = ForestConfig::default();
    config.bind_api = "127.0.0.1:9191".to_string();
    config.mqtt.bind_v3 = "127.0.0.1:9192".to_string();
    config.mqtt.bind_v5 = "127.0.0.1:9193".to_string();
    config.cert_dir = format!("/tmp/forest_certs_{}", db_id);
    fs::create_dir_all(&config.cert_dir).unwrap();
    config.database.path = format!("sqlite:file:memdb_{}?mode=memory&cache=shared", db_id);
    
    // 2. Start server
    let (cancel_token, handle) = start_server(&config).await;

    // Wait a brief moment for server to come up
    sleep(Duration::from_millis(500)).await;

    // 3. Test API Flow using reqwest
    let client = Client::new();
    let api_url = "http://127.0.0.1:9191";

    // Create Tenant
    let mut auth_config = AuthConfig::default();
    auth_config.allow_passwords = true;
    let tenant_payload = json!({
        "tenant_id": "test_tenant",
        "auth_config": auth_config,
        "created_at": chrono::Utc::now().timestamp()
    });

    let res = client.post(&format!("{}/tenants", api_url))
        .json(&tenant_payload)
        .send()
        .await
        .expect("Failed to create tenant");

    assert_eq!(res.status().as_u16(), 200, "Expected 200 OK for tenant creation");

    // Add Password credential
    let password_payload = json!({
        "username": "device_user",
        "password_plaintext": "secret_pass"
    });

    let res = client.post(&format!("{}/test_tenant/devices/device_id_1/passwords", api_url))
        .json(&password_payload)
        .send()
        .await
        .expect("Failed to add password");

    assert_eq!(res.status().as_u16(), 200, "Expected 200 OK for password creation");

    // Fetch Passwords to verify
    let res = client.get(&format!("{}/test_tenant/devices/device_id_1/passwords", api_url))
        .send()
        .await
        .expect("Failed to fetch passwords");

    assert_eq!(res.status().as_u16(), 200, "Expected 200 OK for fetching passwords");
    let passwords: Vec<String> = res.json().await.unwrap();
    assert_eq!(passwords.len(), 1);
    assert_eq!(passwords[0], "device_user");

    // 4. Teardown
    cancel_token.cancel();
    
    // Wait for shutdown (handle typically finishes within a few ms of cancellation)
    let _ = tokio::time::timeout(Duration::from_secs(2), handle).await;
}
