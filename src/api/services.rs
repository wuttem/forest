use std::sync::Arc;

use crate::api::error::AppError;
use crate::certs::CertificateManager;
use crate::db::DB;
use crate::models::{DeviceMetadata, TenantId};

pub async fn create_device(
    device_id: &str,
    tenant_id: &TenantId,
    db: Arc<DB>,
    cert_manager: Arc<CertificateManager>,
) -> Result<DeviceMetadata, AppError> {
    // Check if device already exists
    let existing_device = db.get_device_metadata(&tenant_id, &device_id).await?;
    if existing_device.is_some() {
        return Err(AppError::Conflict(format!(
            "Device {} already exists",
            device_id
        )));
    }
    // Generate Device Cert and Key
    let cert_data = cert_manager.create_client_cert(device_id)?;
    let device_metadata =
        DeviceMetadata::new(&device_id, &tenant_id).with_credentials(cert_data.cert, cert_data.key);
    // Save device metadata to DB
    db.put_device_metadata(&device_metadata).await?;
    Ok(device_metadata)
}
