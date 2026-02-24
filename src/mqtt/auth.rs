use std::sync::Arc;
use tracing::{error, info, warn};
use crate::db::DB;
use crate::models::{Tenant, TenantId};
use rumqttd::ClientInfo;
use crate::mqtt::server::GLOBAL_DB;

pub(crate) async fn auth(
    client_id: String,
    username: String,
    password: String,
    common_name: String,
    organization: String,
    ca_path: Option<String>,
) -> Result<Option<ClientInfo>, String> {
    info!("authentication request: client_id={} username={} common_name={} organization={} ca_path={:?}", client_id, username, common_name, organization, ca_path);

    let db = match GLOBAL_DB.get() {
        Some(db) => db,
        None => {
            error!("Global DB not initialized for auth handler");
            return Err("Internal Server Error".to_string());
        }
    };

    // Extract device_id (client_id)
    // Find device metadata to get tenant
    // Note: since the device id is usually unique across tenants or formatted as <tenant>-<device>,
    // we might need to assume a way to find it. In forest, device lists are partitioned by tenant.
    // However, if we don't know the tenant, we'd have to scan all, but typically the username might contain the tenant,
    // or we can allow the device metadata to be queried.
    // Wait, let's look at the models. We could require username to be tenant_id:username or similar, but for now
    // we'll fetch the first device matching device_id traversing tenants, OR we can require tenant to be passed as organization.
    // For now, let's assume TenantId::Default or from organization.
    let tenant_str = if !organization.is_empty() {
        &organization
    } else {
        "default"
    };
    let tenant_id = TenantId::from_str(tenant_str);

    // Fetch tenant config
    let tenant = db
        .get_tenant(&tenant_id)
        .await
        .map_err(|e| format!("DB Error: {}", e))?
        .unwrap_or_else(|| Tenant::new(&tenant_id));

    let auth_config = tenant.auth_config;

    // Check certificates
    if !common_name.is_empty() {
        if !auth_config.allow_certificates {
            warn!("Certificates are not allowed for this tenant");
            return Ok(None);
        }
        if client_id != common_name {
            warn!("Client ID does not match certificate common name");
            return Ok(None);
        }
        // Valid cert auth
        return Ok(Some(ClientInfo {
            client_id,
            tenant: Some(tenant_id.to_string()),
            lower_rate: None,
            higher_rate: None,
            message_rates: vec![],
        }));
    }

    // Check passwords
    if !username.is_empty() {
        if !auth_config.allow_passwords {
            warn!("Passwords are not allowed for this tenant");
            return Ok(None);
        }
        let is_valid = db
            .verify_device_password(&tenant_id, &client_id, &username, &password)
            .await
            .map_err(|e| format!("DB Error: {}", e))?;
        if is_valid {
            return Ok(Some(ClientInfo {
                client_id,
                tenant: Some(tenant_id.to_string()),
                lower_rate: None,
                higher_rate: None,
                message_rates: vec![],
            }));
        } else {
            warn!("Invalid username or password");
            return Ok(None);
        }
    }

    warn!("Provide either valid certificate or valid username/password");
    Ok(None)
}
