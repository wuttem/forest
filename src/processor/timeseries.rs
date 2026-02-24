use crate::processor::{ProcessorState, ProcessorError};
use crate::models::TenantId;
use tracing::{debug, info};

pub(crate) async fn handle_metric_extraction(
    tenant_id: &TenantId,
    device_id: &str,
    payload: Vec<u8>,
    state: ProcessorState,
) -> Result<(), ProcessorError> {
    let maybe_json = serde_json::from_slice::<serde_json::Value>(&payload);
    let json = match maybe_json {
        Ok(json) => json,
        Err(e) => {
            return Err(ProcessorError::InvalidJson(format!(
                "Failed to parse JSON: {}",
                e
            )));
        }
    };

    // get data config from db
    let maybe_config = state.db.get_data_config(tenant_id, Some(device_id)).await?;
    let metrics = match maybe_config {
        Some(data_config) => data_config.extract_metrics_from_json(json),
        None => return Ok(()),
    };

    let mut counter = 0;
    // store metrics
    // TODO: batch insert for metrics
    for (metric_name, metric_value) in metrics {
        let res = state
            .db
            .put_metric(tenant_id, device_id, &metric_name, metric_value)
            .await;
        match res {
            Ok(_) => {
                counter += 1;
                debug!(metric_name, "Stored metric");
            }
            Err(e) => {
                return Err(ProcessorError::DatabaseError(e));
            }
        }
    }

    info!(%tenant_id, device_id, counter, "Processed metrics");

    Ok(())
}
