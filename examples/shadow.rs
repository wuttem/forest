extern crate forest;

use forest::models::{ShadowName, TenantId};
use forest::shadow::*;
use serde_json::json;

fn main() {
    // Create initial shadow
    let mut shadow = Shadow::new(&"thermostat-123", &ShadowName::Default, &TenantId::Default);
    let update = StateUpdateDocument {
        device_id: "thermostat-123".to_string(),
        shadow_name: ShadowName::Default,
        tenant_id: TenantId::Default,
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
    shadow.update(&update).unwrap();
    // print json serialization of shadow
    println!("{}", shadow.to_json_pretty().unwrap());
}
