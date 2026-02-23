use reqwest::Client;
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Setup the HTTP client
    let client = Client::builder().timeout(Duration::from_secs(10)).build()?;

    let base_url = "http://localhost:8807";
    let tenant_id = "default";
    let device_id = "sensor_1";

    println!(
        "1. Configuring Data Ingestion rules for device: {}...",
        device_id
    );

    // 2. We define a DataConfig that tells the server how to extract metrics from our JSON payload.
    // In this example, we expect JSON like: `{ "temperature": 23.5, "humidity": 45, "gps": [13.4, 52.5] }`
    let data_config = json!({
        "metrics": [
            {
                "name": "temperature",
                "json_pointer": "/temperature",
                "data_type": "Float"
            },
            {
                "name": "humidity",
                "json_pointer": "/humidity",
                "data_type": "Int"
            },
            {
                "name": "location",
                "json_pointer": "/gps",
                "data_type": "LocationTuple"
            }
        ]
    });

    let config_url = format!("{}/{}/dataconfig/device/{}", base_url, tenant_id, device_id);
    let config_resp = client.put(&config_url).json(&data_config).send().await?;

    if config_resp.status().is_success() {
        println!("✅ Configured data ingestion rules.");
    } else {
        println!(
            "❌ Failed to configure rules: {:?}",
            config_resp.text().await?
        );
        return Ok(());
    }

    println!("\n2. Sending telemetry data...");

    // 3. Send a telemetry payload matching our rules
    let telemetry_payload = json!({
        "temperature": 24.1,
        "humidity": 42,
        "gps": [52.5200, 13.4050],
        "ignored_field": "This will not be stored because it isn't in our DataConfig"
    });

    let ingest_url = format!("{}/{}/data/{}", base_url, tenant_id, device_id);
    let ingest_resp = client
        .post(&ingest_url)
        .json(&telemetry_payload)
        .send()
        .await?;

    if ingest_resp.status().is_success() {
        println!("✅ Telemetry data ingested successfully!");
    } else {
        println!(
            "❌ Failed to ingest telemetry data: {:?}",
            ingest_resp.text().await?
        );
    }

    println!("\n3. Verifying telemetry data...");

    // 4. Verify we can get the data back
    let get_url = format!(
        "{}/{}/data/{}/temperature/last?limit=5",
        base_url, tenant_id, device_id
    );
    let get_resp = client.get(&get_url).send().await?;

    if get_resp.status().is_success() {
        let metrics: serde_json::Value = get_resp.json().await?;
        println!(
            "✅ Retrieved metrics: {}",
            serde_json::to_string_pretty(&metrics)?
        );
    } else {
        println!(
            "❌ Failed to retrieve telemetry data: {:?}",
            get_resp.text().await?
        );
    }

    Ok(())
}
