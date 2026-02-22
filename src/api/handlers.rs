use std::collections::HashMap;

use crate::api::error::AppError;
use crate::api::AppState;
use crate::api::services::create_device;
use crate::dataconfig::{DataConfig, DataConfigEntry};
use crate::db::DatabaseError;
use crate::processor::send_delta_to_mqtt;
use crate::shadow::{NestedStateDocument, Shadow, StateUpdateDocument};
use crate::models::{DeviceInformation, DeviceMetadata};
use crate::models::{ShadowName, TenantId};
use crate::timeseries::{TimeSeriesConversions, TimeSeriesModel};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone)]
pub struct HomeResponse {
    pub connected_devices: usize,
    pub mqtt_messages_received: u64,
    pub mqtt_messages_sent: u64,
    pub mqtt_messages_dropped: u64,
    pub forest_version: String,
}

pub async fn home_handler(State(state): State<AppState>) -> Result<Json<HomeResponse>, AppError> {
    let devices = state.connected_clients;
    let connected_devices = devices.len();
    let metrics = state.mqtt_metrics.clone();
    let mqtt_received = metrics
        .messages_forwarded
        .load(std::sync::atomic::Ordering::Relaxed);
    let mqtt_sent = metrics
        .messages_sent
        .load(std::sync::atomic::Ordering::Relaxed);
    let mqtt_dropped = metrics
        .messages_dropped
        .load(std::sync::atomic::Ordering::Relaxed);
    let forest_version = env!("CARGO_PKG_VERSION").to_string();
    
    let response = HomeResponse {
        connected_devices,
        mqtt_messages_received: mqtt_received,
        mqtt_messages_sent: mqtt_sent,
        mqtt_messages_dropped: mqtt_dropped,
        forest_version,
    };

    Ok(Json(response))
}

pub async fn health_handler() -> &'static str {
    "OK"
}

pub async fn get_shadow_handler(
    Path((_tenant_id, device_id)): Path<(String, String)>,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Shadow>, AppError> {
    let db = state.db.clone();
    let maybe_shadow_name = params.get("name");
    let shadow_name = match maybe_shadow_name {
        Some(name) => ShadowName::from_str(name),
        None => ShadowName::Default,
    };
    match db._get_shadow(&device_id, &shadow_name, &TenantId::Default).await {
        Ok(doc) => Ok(Json(doc)),
        Err(DatabaseError::NotFoundError(_)) => Err(AppError::NotFound(format!(
            "Shadow ({}) not found for device: {}",
            shadow_name.as_str(),
            device_id
        ))),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

pub async fn update_shadow_handler(
    Path((_tenant_id, device_id)): Path<(String, String)>,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    Json(nested_update_doc): Json<NestedStateDocument>,
) -> Result<Json<Shadow>, AppError> {
    let tenant_id = TenantId::Default;
    let maybe_shadow_name = params.get("name");
    let shadow_name = match maybe_shadow_name {
        Some(name) => ShadowName::from_str(name),
        None => ShadowName::Default,
    };
    let update_doc = StateUpdateDocument::from_nested_state(
        nested_update_doc,
        &device_id,
        &shadow_name,
        &tenant_id,
    );
    // Upsert shadow
    let shadow = match state.db._upsert_shadow(&update_doc).await {
        Ok(updated) => updated,
        Err(e) => return Err(AppError::DatabaseError(e)),
    };

    //  Send delta to device if we have a mqtt sender
    if params.get("send_delta").is_some() {
        if let Some(mqtt_sender) = &state.mqtt_sender {
            let _delta_sent = send_delta_to_mqtt(&shadow, mqtt_sender, &state.shadow_topic_prefix);
        }
    }

    Ok(Json(shadow))
}

pub async fn delete_shadow_handler(
    Path((_tenant_id, device_id)): Path<(String, String)>,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<()>, AppError> {
    let tenant_id = TenantId::Default;
    let maybe_shadow_name = params.get("name");
    let shadow_name = match maybe_shadow_name {
        Some(name) => ShadowName::from_str(name),
        None => ShadowName::Default,
    };
    match state.db._delete_shadow(&device_id, &shadow_name, &tenant_id).await {
        Ok(_) => Ok(Json(())),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

#[derive(Deserialize)]
pub struct TimeseriesQuery {
    pub start: u64,
    pub end: u64,
}

pub async fn get_timeseries_handler(
    Path((_tenant_id, device_id, metric)): Path<(String, String, String)>,
    State(state): State<AppState>,
    Query(range): Query<TimeseriesQuery>,
) -> Result<Json<TimeSeriesModel>, AppError> {
    let db = &state.db;
    let tenant_id = TenantId::Default;
    let timeseries = match db.get_metric(&tenant_id, &device_id, &metric, range.start, range.end).await {
        Ok(ts) => ts,
        Err(DatabaseError::NotFoundError(_)) => {
            return Err(AppError::NotFound(format!(
                "No timeseries found for {} / {}",
                device_id, metric
            )));
        }
        Err(e) => return Err(AppError::DatabaseError(e)),
    };
    Ok(Json(timeseries.to_model(&device_id, &metric)))
}

#[derive(Deserialize)]
pub struct LastValuesQuery {
    pub limit: Option<u64>,
}

pub async fn get_last_timeseries_handler(
    Path((_tenant_id, device_id, metric)): Path<(String, String, String)>,
    State(state): State<AppState>,
    Query(query): Query<LastValuesQuery>,
) -> Result<Json<TimeSeriesModel>, AppError> {
    let db = &state.db;
    let tenant_id = TenantId::Default;
    let limit = query.limit.unwrap_or(1);

    let timeseries = match db.get_last_metric(&tenant_id, &device_id, &metric, limit).await {
        Ok(ts) => ts,
        Err(DatabaseError::NotFoundError(_)) => {
            return Err(AppError::NotFound(format!(
                "No timeseries found for {} / {}",
                device_id, metric
            )));
        }
        Err(e) => return Err(AppError::DatabaseError(e)),
    };

    Ok(Json(timeseries.to_model(&device_id, &metric)))
}

pub async fn store_device_config_handler(
    Path((tenant_id, device_prefix)): Path<(String, String)>,
    State(state): State<AppState>,
    Json(config): Json<DataConfig>,
) -> Result<Json<DataConfig>, AppError> {
    let db = &state.db;
    let tenant_id = TenantId::from_str(&tenant_id);
    match db.store_device_data_config(&tenant_id, &device_prefix, &config).await {
        Ok(_) => Ok(Json(config)),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

pub async fn store_tenant_config_handler(
    Path(tenant_id): Path<String>,
    State(state): State<AppState>,
    Json(config): Json<DataConfig>,
) -> Result<Json<DataConfig>, AppError> {
    let db = &state.db;
    let tenant_id = TenantId::from_str(&tenant_id);
    match db.store_tenant_data_config(&tenant_id, &config).await {
        Ok(_) => Ok(Json(config)),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

pub async fn get_tenant_config_handler(
    Path(tenant_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<DataConfig>, AppError> {
    let db = &state.db;
    let tenant_id = TenantId::from_str(&tenant_id);
    match db.get_data_config(&tenant_id, None).await {
        Ok(Some(config)) => Ok(Json(config)),
        Ok(None) => Err(AppError::NotFound(format!(
            "No config found for tenant: {}",
            tenant_id
        ))),
        Err(DatabaseError::NotFoundError(msg)) => Err(AppError::NotFound(msg)),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

pub async fn get_config_handler(
    Path((tenant_id, device_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<DataConfig>, AppError> {
    let db = &state.db;
    let tenant_id = TenantId::from_str(&tenant_id);
    match db.get_data_config(&tenant_id, Some(&device_id)).await {
        Ok(Some(config)) => Ok(Json(config)),
        Ok(None) => Err(AppError::NotFound(format!(
            "No config found for tenant: {} and device: {:?}",
            tenant_id, device_id
        ))),
        Err(DatabaseError::NotFoundError(msg)) => Err(AppError::NotFound(msg)),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

pub async fn delete_config_handler(
    Path((tenant_id, device_prefix)): Path<(String, Option<String>)>,
    State(state): State<AppState>,
) -> Result<Json<()>, AppError> {
    let db = &state.db;
    let tenant_id = TenantId::from_str(&tenant_id);
    match db.delete_data_config(&tenant_id, device_prefix.as_deref()).await {
        Ok(_) => Ok(Json(())),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

pub async fn list_configs_handler(
    Path(tenant_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<DataConfigEntry>>, AppError> {
    let db = &state.db;
    let tenant_id = TenantId::from_str(&tenant_id);
    match db.list_data_configs(&tenant_id).await {
        Ok(configs) => Ok(Json(configs)),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

pub async fn list_connections_handler(
    Path(_tenant_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<String>>, AppError> {
    let devices = state.connected_clients;
    let device_iter = devices.iter();
    let connections = device_iter.map(|x| (*x).to_owned()).collect();
    Ok(Json(connections))
}



#[derive(Deserialize)]
pub struct PutDeviceBody {
    key: Option<String>,
}

pub async fn post_device_metadata_handler(
    Path((tenant_id, device_id)): Path<(String, String)>,
    State(state): State<AppState>,
    Json(device_info): Json<PutDeviceBody>,
) -> Result<Json<DeviceMetadata>, AppError> {
    // Ensure the path parameters match the body
    let tenant_id = TenantId::from_str(&tenant_id);
    let db = state.db.clone();
    let cert_manager = state.cert_manager.clone();

    if let Some(_key) = &device_info.key {
        // A device key was provided in the request body
        // This could be used for custom authentication or identification
        tracing::warn!("Device key provided");
        // You might want to store this key in the device metadata
        // or use it for certificate generation
    }
    let metadata = create_device(&device_id, &tenant_id, db, cert_manager).await?;

    match state.db.put_device_metadata(&metadata).await {
        Ok(_) => Ok(Json(metadata)),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

// Handler to get detailed device information
pub async fn get_device_info_handler(
    Path((tenant_id, device_id)): Path<(String, String)>,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<DeviceInformation>, AppError> {
    let tenant_id = TenantId::from_str(&tenant_id);
    
    // Get device metadata
    let metadata = match state.db.get_device_metadata(&tenant_id, &device_id).await {
        Ok(Some(metadata)) => metadata,
        Ok(None) => return Err(AppError::NotFound(format!(
            "Device metadata not found for tenant: {} and device: {}",
            tenant_id, device_id
        ))),
        Err(e) => return Err(AppError::DatabaseError(e)),
    };
    
    // Check connection status
    let connected = state.connected_clients.contains(&device_id);
    
    // Get shadow last update time if requested
    let mut last_shadow_update = None;

    // Get shadow name from query params or use default
    let maybe_shadow_name = params.get("name");
    let shadow_name = match maybe_shadow_name {
        Some(name) => ShadowName::from_str(name),
        None => ShadowName::Default,
    };
    
    // Try to get the shadow to extract last_updated
    match state.db._get_shadow(&device_id, &shadow_name, &tenant_id).await {
        Ok(shadow) => {
            last_shadow_update = Some(shadow.get_last_updated());
        },
        Err(DatabaseError::NotFoundError(_)) => {
            // No shadow found, leave last_shadow_update as None
        },
        Err(e) => return Err(AppError::DatabaseError(e)),
    }
    
    // Construct the DeviceInformation response
    let device_info = DeviceInformation {
        device_id: metadata.device_id,
        tenant_id: metadata.tenant_id,
        certificate: metadata.certificate,
        connected,
        last_shadow_update,
    };
    
    Ok(Json(device_info))
}

// Handler to get device metadata
pub async fn get_device_metadata_handler(
    Path((tenant_id, device_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<DeviceMetadata>, AppError> {
    let tenant_id = TenantId::from_str(&tenant_id);
    
    match state.db.get_device_metadata(&tenant_id, &device_id).await {
        Ok(Some(metadata)) => Ok(Json(metadata)),
        Ok(None) => Err(AppError::NotFound(format!(
            "Device metadata not found for tenant: {} and device: {}",
            tenant_id, device_id
        ))),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}


// Handler to list all devices for a tenant
pub async fn list_devices_handler(
    Path(tenant_id): Path<String>,
    State(state): State<AppState>,
) -> Result<Json<Vec<String>>, AppError> {
    let tenant_id = TenantId::from_str(&tenant_id);
    
    match state.db.list_devices(&tenant_id).await {
        Ok(devices) =>{
            // Return a list of device IDs
            // Convert the DeviceMetadata objects to just their device IDs
            let device_ids = devices.into_iter()
                .map(|metadata| metadata.device_id)
                .collect();
            Ok(Json(device_ids))
        },
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}

// Handler to delete device metadata
pub async fn delete_device_metadata_handler(
    Path((tenant_id, device_id)): Path<(String, String)>,
    State(state): State<AppState>,
) -> Result<Json<()>, AppError> {
    let tenant_id = TenantId::from_str(&tenant_id);
    match state.db.delete_device_metadata(&tenant_id, &device_id).await {
        Ok(_) => Ok(Json(())),
        Err(e) => Err(AppError::DatabaseError(e)),
    }
}