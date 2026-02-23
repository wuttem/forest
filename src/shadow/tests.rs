use super::*;
use serde_json::json;

#[test]
fn test_state_document_update() {
    // Initial state setup
    let mut current = StateDocument {
        reported: json!({
            "device": {
                "name": "livingroom_sensor",
                "readings": {
                    "temperature": 21.5,
                    "humidity": 45,
                    "battery": 98
                },
                "config": {
                    "sample_rate": 300,
                    "alert_threshold": 30
                },
                "tags": ["temperature", "humidity"]
            }
        }),
        desired: json!({}),
        delta: json!(null),
    };

    // Create update document
    let update = StateDocument {
        reported: json!({
            "device": {
                "readings": {
                    "temperature": 23.1,
                    "humidity": null,  // Should be deleted
                    "co2": 800        // New value
                },
                "config": {
                    "sample_rate": 600
                },
                "tags": ["temperature", "co2"]
            }
        }),
        desired: json!({}),
        delta: json!(null),
    };

    // Create metadata state
    let mut metadata = MetadataDocument {
        reported: Value::Null,
        desired: Value::Null,
    };

    // Perform update
    current.update(&update, &mut metadata);

    // Verify final state
    let expected_state = json!({
        "device": {
            "name": "livingroom_sensor",
            "readings": {
                "temperature": 23.1,
                "battery": 98,
                "co2": 800
            },
            "config": {
                "sample_rate": 600,
                "alert_threshold": 30
            },
            "tags": ["temperature", "co2"]
        }
    });

    assert_eq!(current.reported, expected_state);

    // Verify metadata structure matches state structure
    // and contains timestamps for updated values
    if let Value::Object(metadata_obj) = metadata.reported {
        if let Some(device) = metadata_obj.get("device") {
            if let Some(readings) = device.get("readings") {
                assert!(readings.get("temperature").unwrap().is_number());
                assert!(readings.get("co2").unwrap().is_number());
                assert!(readings.get("humidity").is_none());
            }
            if let Some(config) = device.get("config") {
                assert!(config.get("sample_rate").unwrap().is_number());
            }
            if let Some(tags) = device.get("tags") {
                assert!(tags.is_number());
            }
        }
    }
}

#[test]
fn test_shadow_update() {
    // Create initial shadow
    let mut shadow = Shadow::new(
        &"thermostat-123",
        &ShadowName::new("main"),
        &TenantId::new("tenant"),
    );

    // Create valid update with both reported and desired
    let update = StateUpdateDocument {
        device_id: "thermostat-123".to_string(),
        shadow_name: ShadowName::new("main"),
        tenant_id: TenantId::new("tenant"),
        state: {
            StateDocument {
                reported: json!({
                    "temperature": 22.5,
                    "humidity": 45,
                    "mode": "auto"
                }),
                desired: json!({
                    "temperature": 21.0,
                    "mode": "cool"
                }),
                delta: json!(null),
            }
        },
    };

    // Apply update
    assert!(shadow.update(&update).is_ok());

    // Verify reported state
    assert_eq!(
        shadow.state.reported,
        json!({
            "temperature": 22.5,
            "humidity": 45,
            "mode": "auto"
        })
    );

    // Verify desired state
    assert_eq!(
        shadow.state.desired,
        json!({
            "temperature": 21.0,
            "mode": "cool"
        })
    );

    // Verify delta contains differences
    assert_eq!(
        shadow.state.delta,
        json!({
            "temperature": 21.0,
            "mode": "cool"
        })
    );

    // Test invalid device ID
    let invalid_update = StateUpdateDocument {
        device_id: "wrong-id".to_string(),
        shadow_name: ShadowName::new("main"),
        tenant_id: TenantId::new("tenant"),
        state: StateDocument {
            reported: Value::Null,
            desired: Value::Null,
            delta: Value::Null,
        },
    };
    assert!(matches!(
        shadow.update(&invalid_update),
        Err(ShadowError::DeviceIdMismatch)
    ));

    // Test invalid shadow name
    let invalid_update = StateUpdateDocument {
        device_id: "thermostat-123".to_string(),
        shadow_name: ShadowName::new("wrong"),
        tenant_id: TenantId::new("tenant"),
        state: StateDocument {
            reported: Value::Null,
            desired: Value::Null,
            delta: Value::Null,
        },
    };
    assert!(matches!(
        shadow.update(&invalid_update),
        Err(ShadowError::ShadowNameMismatch)
    ));
}

#[test]
fn test_shadow_serialization() {
    // Create shadow with realistic data
    let mut shadow = Shadow::new(
        &"garage-sensor-01",
        &ShadowName::new("main"),
        &TenantId::new("tenant"),
    );

    // Update with initial state
    let update = StateUpdateDocument {
        device_id: "garage-sensor-01".to_string(),
        shadow_name: ShadowName::new("main"),
        tenant_id: TenantId::new("tenant"),
        state: StateDocument {
            reported: json!({
                "sensors": {
                    "temperature": 24.5,
                    "humidity": 65,
                    "door": "closed"
                },
                "system": {
                    "firmware": "v1.2.3",
                    "uptime": 3600
                }
            }),
            desired: json!({
                "sensors": {
                    "temperature": 22.0,
                    "door": "open"
                }
            }),
            delta: json!(null),
        },
    };

    shadow.update(&update).unwrap();

    // Serialize to JSON string
    let serialized = serde_json::to_string_pretty(&shadow).unwrap();

    // Deserialize back to shadow
    let deserialized: Shadow = serde_json::from_str(&serialized).unwrap();

    // Verify identity preserved
    assert_eq!(deserialized.device_id, "garage-sensor-01");
    assert_eq!(deserialized.shadow_name, ShadowName::new("main"));

    // Verify reported state
    assert_eq!(
        deserialized
            .state
            .reported
            .pointer("/sensors/temperature")
            .unwrap(),
        24.5
    );
    assert_eq!(
        deserialized
            .state
            .reported
            .pointer("/system/firmware")
            .unwrap(),
        "v1.2.3"
    );

    // Verify desired state
    assert_eq!(
        deserialized
            .state
            .desired
            .pointer("/sensors/temperature")
            .unwrap(),
        22.0
    );

    // Verify delta calculation preserved
    assert!(deserialized
        .state
        .delta
        .pointer("/sensors/temperature")
        .is_some());
    assert!(deserialized.state.delta.pointer("/sensors/door").is_some());

    // Verify metadata timestamps exist
    assert!(deserialized
        .metadata
        .reported
        .pointer("/sensors/temperature")
        .unwrap()
        .is_number());

    // Verify complete round-trip equality
    assert_eq!(
        shadow
            .state
            .reported
            .pointer("/sensors/temperature")
            .unwrap(),
        deserialized
            .state
            .reported
            .pointer("/sensors/temperature")
            .unwrap()
    );
}

#[test]
fn test_shadow_name_creation() {
    // Test new()
    assert_eq!(ShadowName::new("default"), ShadowName::Default);
    assert_eq!(ShadowName::new("DEFAULT"), ShadowName::Default);
    assert_eq!(
        ShadowName::new("custom"),
        ShadowName::Custom("custom".to_string())
    );

    // Test from_str()
    assert_eq!(ShadowName::from_str("default"), ShadowName::Default);
    assert_eq!(ShadowName::from_str("DEFAULT"), ShadowName::Default);
    assert_eq!(
        ShadowName::from_str("custom"),
        ShadowName::Custom("custom".to_string())
    );
}

#[test]
fn test_shadow_name_serialization() {
    // Test serializing Default variant
    let default = ShadowName::Default;
    let json = serde_json::to_string(&default).unwrap();
    assert_eq!(json, r#""default""#);

    // Test serializing Custom variant
    let custom = ShadowName::Custom("my-custom-shadow".to_string());
    let json = serde_json::to_string(&custom).unwrap();
    assert_eq!(json, r#""my-custom-shadow""#);
}

#[test]
fn test_shadow_name_deserialization() {
    // Test deserializing to Default variant
    let default: ShadowName = serde_json::from_str(r#""default""#).unwrap();
    assert_eq!(default, ShadowName::Default);

    // Test case-insensitive "default"
    let default_upper: ShadowName = serde_json::from_str(r#""DEFAULT""#).unwrap();
    assert_eq!(default_upper, ShadowName::Default);

    // Test deserializing to Custom variant
    let custom: ShadowName = serde_json::from_str(r#""my-custom-shadow""#).unwrap();
    assert_eq!(custom, ShadowName::Custom("my-custom-shadow".to_string()));
}

#[test]
fn test_shadow_name_in_struct() {
    // Test serialization within a struct
    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct TestStruct {
        name: ShadowName,
    }

    let test = TestStruct {
        name: ShadowName::Default,
    };
    let json = serde_json::to_string(&test).unwrap();
    assert_eq!(json, r#"{"name":"default"}"#);

    // Test deserialization within a struct
    let test: TestStruct = serde_json::from_str(r#"{"name":"custom-name"}"#).unwrap();
    assert_eq!(test.name, ShadowName::Custom("custom-name".to_string()));
}
