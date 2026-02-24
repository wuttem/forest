use crate::api::handlers::*;
use crate::api::AppState;
use axum::{
    routing::{get, post, put},
    Router,
};

pub fn get_routes(state: AppState) -> Router {
    Router::new()
        .route("/", get(home_handler))
        .route("/health", get(health_handler))
        .route("/time", get(time_handler))
        .route(
            "/{tenant_id}/things/{device_id}/shadow",
            get(get_shadow_handler)
                .post(update_shadow_handler)
                .delete(delete_shadow_handler),
        )
        .route(
            "/{tenant_id}/data/{device_id}/{metric}",
            get(get_timeseries_handler),
        )
        .route(
            "/{tenant_id}/data/{device_id}",
            post(post_telemetry_handler),
        )
        .route(
            "/{tenant_id}/data/{device_id}/{metric}/last",
            get(get_last_timeseries_handler),
        )
        .route(
            "/{tenant_id}/dataconfig",
            put(store_tenant_config_handler)
                .get(get_tenant_config_handler)
                .delete(delete_config_handler),
        )
        .route(
            "/{tenant_id}/dataconfig/device/{device_prefix}",
            put(store_device_config_handler)
                .get(get_config_handler)
                .delete(delete_config_handler),
        )
        .route("/{tenant_id}/dataconfig/all", get(list_configs_handler))
        .route("/{tenant_id}/connected", get(list_connections_handler))
        .route("/{tenant_id}/devices", get(list_devices_handler))
        .route(
            "/{tenant_id}/devices/{device_id}",
            get(get_device_info_handler)
                .post(post_device_metadata_handler)
                .delete(delete_device_metadata_handler),
        )
        .route(
            "/{tenant_id}/devices/{device_id}/metadata",
            get(get_device_metadata_handler),
        )
        .route("/tenants", post(create_tenant_handler))
        .route("/tenants/{tenant_id}", get(get_tenant_handler))
        .route(
            "/{tenant_id}/devices/{device_id}/passwords",
            get(get_device_passwords_handler).post(add_device_password_handler),
        )
        .route(
            "/cacert/server",
            get(get_server_ca_handler).post(generate_server_ca_handler),
        )
        .route(
            "/tenants/{tenant_id}/cacert",
            get(get_tenant_ca_handler).post(upload_tenant_ca_handler),
        )
        .route(
            "/tenants/{tenant_id}/cacert/generate",
            post(generate_tenant_ca_handler),
        )
        .route(
            "/tenants/{tenant_id}/devices/{device_id}/client_cert/generate",
            post(generate_client_cert_handler),
        )
        .with_state(state)
}
