use super::*;
use crate::db::{DatabaseConfig, DB};
use crate::models::{AuthConfig, DeviceCredential, Tenant, TenantId};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;
use uuid::Uuid;

async fn setup_db() -> (Arc<DB>, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let mut config = DatabaseConfig::default();
    let db_id = Uuid::new_v4().simple();
    config.path = format!("sqlite:file:memdb_{}?mode=memory&cache=shared", db_id);

    let db = DB::open(&config).await.unwrap();
    (Arc::new(db), temp_dir)
}

fn get_test_config() -> Option<MqttConfig> {
    let mut config = MqttConfig::default();
    config.bind_v3 = "127.0.0.1:7777".to_string();
    config.bind_v5 = "127.0.0.1:7778".to_string();
    Some(config)
}

#[ignore]
#[tokio::test]
async fn test_server_start_stop() {
    let (db, _temp) = setup_db().await;
    let config = get_test_config();
    let mut server = start_broker(config, db).await;

    let shutdown_received = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let shutdown_received_clone = shutdown_received.clone();
    let cancel_token = server.get_cancel_token();

    tokio::spawn(async move {
        cancel_token.cancelled().await;
        shutdown_received_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    sleep(Duration::from_millis(100)).await;
    server.shutdown();

    sleep(Duration::from_millis(100)).await;
    assert!(shutdown_received.load(std::sync::atomic::Ordering::SeqCst));
}

#[ignore]
#[tokio::test]
async fn test_publish_subscribe() {
    let (db, _temp) = setup_db().await;
    let config = get_test_config();
    let mut server = start_broker(config, db).await;

    // Create receiver
    let receiver = server.message_receiver();

    // Subscribe to topic
    server
        .mqtt
        .subscribe("test/topic".to_string())
        .await
        .unwrap();
    sleep(Duration::from_millis(100)).await;

    // Publish message
    let test_payload = b"test message".to_vec();
    server
        .mqtt
        .publish("test/topic".to_string(), test_payload.clone())
        .await
        .unwrap();

    // Receive message
    if let Ok(msg) = receiver.recv_async().await {
        assert_eq!(msg.topic, "test/topic");
        assert_eq!(msg.payload, test_payload);
    }

    server.shutdown();
}

#[tokio::test]
async fn test_auth_handler() {
    let (setup_db_inst, _temp) = setup_db().await;

    // Set global DB manually for testing if not already set by other tests
    let _ = GLOBAL_DB.set(setup_db_inst);
    let db = GLOBAL_DB.get().unwrap().clone();

    let tenant_id = TenantId::new("test_tenant");
    let mut auth_config = AuthConfig::default();
    auth_config.allow_passwords = true;
    auth_config.allow_certificates = true;

    let tenant = Tenant::new(&tenant_id).with_auth_config(auth_config);
    db.put_tenant(&tenant).await.unwrap();

    let password = "secret_password";
    let hash = bcrypt::hash(password, bcrypt::DEFAULT_COST).unwrap();

    let credential = DeviceCredential {
        tenant_id: tenant_id.clone(),
        device_id: "device1".to_string(),
        username: "device1_user".to_string(),
        password_hash: hash,
        created_at: chrono::Utc::now().timestamp() as u64,
    };
    db.add_device_password(&credential).await.unwrap();

    // Test valid password auth
    let result = auth(
        "device1".to_string(),
        "device1_user".to_string(),
        "secret_password".to_string(),
        "".to_string(),
        "test_tenant".to_string(),
        None,
    )
    .await;
    let client_info = result.unwrap().unwrap();
    assert_eq!(client_info.client_id, "device1");
    assert_eq!(client_info.tenant.unwrap(), "test_tenant");

    // Test invalid password auth
    let result = auth(
        "device1".to_string(),
        "device1_user".to_string(),
        "wrong_password".to_string(),
        "".to_string(),
        "test_tenant".to_string(),
        None,
    )
    .await;
    assert!(result.unwrap().is_none());

    // Test invalid username
    let result = auth(
        "device1".to_string(),
        "wrong_username".to_string(),
        "secret_password".to_string(),
        "".to_string(),
        "test_tenant".to_string(),
        None,
    )
    .await;
    assert!(result.unwrap().is_none());

    // Test valid cert auth
    let result = auth(
        "device_cert_1".to_string(),
        "".to_string(),
        "".to_string(),
        "device_cert_1".to_string(),
        "test_tenant".to_string(),
        None,
    )
    .await;
    let client_info = result.unwrap().unwrap();
    assert_eq!(client_info.client_id, "device_cert_1");

    // Test mismatched cert
    let result = auth(
        "device_cert_1".to_string(),
        "".to_string(),
        "".to_string(),
        "device_cert_2".to_string(),
        "test_tenant".to_string(),
        None,
    )
    .await;
    assert!(result.unwrap().is_none());

    // Test passwords disabled
    let mut tenant2 = Tenant::new(&TenantId::new("no_password_tenant"));
    tenant2.auth_config.allow_passwords = false;
    db.put_tenant(&tenant2).await.unwrap();

    let result = auth(
        "device1".to_string(),
        "user1".to_string(),
        "pass".to_string(),
        "".to_string(),
        "no_password_tenant".to_string(),
        None,
    )
    .await;
    assert!(result.unwrap().is_none());
}
