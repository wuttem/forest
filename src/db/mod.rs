use crate::dataconfig::{DataConfig, DataConfigEntry};
use crate::shadow::{
    Shadow, ShadowError, ShadowSerializationError, StateUpdateDocument,
};
use crate::models::{DeviceMetadata, ShadowName, TenantId, Tenant, DeviceCredential};
use crate::timeseries::{
    MetricTimeSeries, MetricValue, TimeSeriesConversions, TimeseriesSerializationError,
};
use sqlx::{AnyPool, Row, any::AnyPoolOptions, query};
use serde::{Deserialize, Serialize};
use tracing::{warn, info};
use std::sync::Arc;
use thiserror::Error;

const MAX_FUTURE_SECONDS: u64 = 60 * 60 * 24 * 365;

#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("SQLx Error: {0}")]
    SqlxError(#[from] sqlx::Error),
    #[error("TimeseriesSerialization Error: {0}")]
    TimeseriesSerializationError(#[from] TimeseriesSerializationError),
    #[error("DatabaseConnection Error")]
    DatabaseConnectionError,
    #[error("Invalid Key: {0}")]
    InvalidKeyError(String),
    #[error("Bincode Error: {0}")]
    BincodeError(Box<bincode::ErrorKind>),
    #[error("DatabaseValue Error: {0}")]
    DatabaseValueError(String),
    #[error("ShadowSerialization Error: {0}")]
    ShadowSerializationError(#[from] ShadowSerializationError),
    #[error("Shadow Error: {0}")]
    ShadowError(#[from] ShadowError),
    #[error("DatabaseTransaction Error {0}")]
    DatabaseTransactionError(String),
    #[error("NotFound Error {0}")]
    NotFoundError(String),
}

impl From<Box<bincode::ErrorKind>> for DatabaseError {
    fn from(err: Box<bincode::ErrorKind>) -> Self {
        DatabaseError::BincodeError(err)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub path: String, // e.g., "sqlite:./test.db" or "postgres://user:pass@localhost/db"
    pub create_if_missing: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        DatabaseConfig {
            path: String::from("sqlite://.forest.db?mode=rwc"),
            create_if_missing: true,
        }
    }
}

pub struct DB {
    pub path: String,
    pub pool: Option<Arc<AnyPool>>,
}

impl DB {
    pub async fn open_default(path: &str) -> Result<Self, DatabaseError> {
        let mut config = DatabaseConfig::default();
        config.path = path.to_string();
        DB::open(&config).await
    }

    pub async fn open(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        sqlx::any::install_default_drivers();
        let pool = AnyPoolOptions::new()
            .max_connections(5)
            .connect(&config.path)
            .await?;
        
        // Ensure tables exist
        let mut conn = pool.acquire().await?;
        
        let is_postgres = config.path.starts_with("postgres");
        let blob_type = if is_postgres { "BYTEA" } else { "BLOB" };
        let serial_type = if is_postgres { "SERIAL" } else { "INTEGER" };

        // Create table for general Key-Value (similar to rocksdb)
        let kv_query = format!(
            "CREATE TABLE IF NOT EXISTS kv_store (
                key TEXT PRIMARY KEY,
                value {} NOT NULL
            )",
            blob_type
        );
        sqlx::query(&kv_query).execute(&mut *conn).await?;

        // Create table for Timeseries Buckets
        let ts_query = format!(
            "CREATE TABLE IF NOT EXISTS timeseries_buckets (
                id {} PRIMARY KEY,
                key TEXT NOT NULL,
                bucket_timestamp BIGINT NOT NULL,
                data {} NOT NULL,
                UNIQUE(key, bucket_timestamp)
            )",
            serial_type, blob_type
        );
        sqlx::query(&ts_query).execute(&mut *conn).await?;

        // Create table for Shadows
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS shadows (
                tenant_id TEXT NOT NULL,
                device_id TEXT NOT NULL,
                shadow_name TEXT NOT NULL,
                data TEXT NOT NULL,
                PRIMARY KEY (tenant_id, device_id, shadow_name)
            )"
        ).execute(&mut *conn).await?;

        // Create table for Data Configs
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS data_configs (
                tenant_id TEXT NOT NULL,
                device_prefix TEXT NOT NULL,
                config TEXT NOT NULL,
                PRIMARY KEY (tenant_id, device_prefix)
            )"
        ).execute(&mut *conn).await?;

        // Create table for Device Metadata
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS device_metadata (
                tenant_id TEXT NOT NULL,
                device_id TEXT NOT NULL,
                metadata TEXT NOT NULL,
                PRIMARY KEY (tenant_id, device_id)
            )"
        ).execute(&mut *conn).await?;

        // Create table for Tenants
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS tenants (
                tenant_id TEXT NOT NULL,
                data TEXT NOT NULL,
                PRIMARY KEY (tenant_id)
            )"
        ).execute(&mut *conn).await?;

        // Create table for Device Credentials
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS device_credentials (
                tenant_id TEXT NOT NULL,
                device_id TEXT NOT NULL,
                username TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                created_at BIGINT NOT NULL,
                PRIMARY KEY (tenant_id, device_id, username)
            )"
        ).execute(&mut *conn).await?;

        Ok(DB {
            path: config.path.to_owned(),
            pool: Some(Arc::new(pool)),
        })
    }

    pub async fn destroy(path: &str) -> Result<(), DatabaseError> {
        // No direct equivalent in SQLx Any, depends on driver. For SQLite it's deleting the file.
        warn!("Destroy not fully supported through SQLx Any. Path: {}", path);
        Ok(())
    }

    pub async fn put_tenant(&self, tenant: &Tenant) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            let mut tx = pool.begin().await?;
            let t_id = tenant.tenant_id.to_string();
            let data = serde_json::to_string(tenant).map_err(|e| {
                DatabaseError::DatabaseValueError(format!("Failed to serialize tenant: {}", e))
            })?;

            sqlx::query("DELETE FROM tenants WHERE tenant_id = $1")
                .bind(&t_id)
                .execute(&mut *tx).await?;

            sqlx::query("INSERT INTO tenants (tenant_id, data) VALUES ($1, $2)")
                .bind(&t_id)
                .bind(&data)
                .execute(&mut *tx).await?;

            tx.commit().await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn get_tenant(&self, tenant_id: &TenantId) -> Result<Option<Tenant>, DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT data FROM tenants WHERE tenant_id = $1"
            )
            .bind(&t_id)
            .fetch_optional(&**pool).await?;

            match row {
                Some((data,)) => {
                    let tenant = serde_json::from_str(&data).map_err(|e| {
                        DatabaseError::DatabaseValueError(format!("Failed to deserialize tenant: {}", e))
                    })?;
                    Ok(Some(tenant))
                }
                None => Ok(None),
            }
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn add_device_password(&self, credential: &DeviceCredential) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            let mut tx = pool.begin().await?;
            let t_id = credential.tenant_id.to_string();
            let d_id = &credential.device_id;
            let u_name = &credential.username;
            let p_hash = &credential.password_hash;
            let c_at = credential.created_at as i64;

            sqlx::query("DELETE FROM device_credentials WHERE tenant_id = $1 AND device_id = $2 AND username = $3")
                .bind(&t_id)
                .bind(d_id)
                .bind(u_name)
                .execute(&mut *tx).await?;

            sqlx::query("INSERT INTO device_credentials (tenant_id, device_id, username, password_hash, created_at) VALUES ($1, $2, $3, $4, $5)")
                .bind(&t_id)
                .bind(d_id)
                .bind(u_name)
                .bind(p_hash)
                .bind(c_at)
                .execute(&mut *tx).await?;

            tx.commit().await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn verify_device_password(&self, tenant_id: &TenantId, device_id: &str, username: &str, password: &str) -> Result<bool, DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT password_hash FROM device_credentials WHERE tenant_id = $1 AND device_id = $2 AND username = $3"
            )
            .bind(&t_id)
            .bind(device_id)
            .bind(username)
            .fetch_optional(&**pool).await?;
            
            match row {
                Some((hash_str,)) => {
                    // Check against bcrypt hash
                    let valid = match bcrypt::verify(password, &hash_str) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!("Bcrypt verify error: {:?}", e);
                            false
                        }
                    };
                    Ok(valid)
                },
                None => Ok(false),
            }
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn list_device_passwords(&self, tenant_id: &TenantId, device_id: &str) -> Result<Vec<String>, DatabaseError> {
         if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let rows: Vec<(String,)> = sqlx::query_as(
                "SELECT username FROM device_credentials WHERE tenant_id = $1 AND device_id = $2"
            )
            .bind(&t_id)
            .bind(device_id)
            .fetch_all(&**pool).await?;

            let usernames = rows.into_iter().map(|(u,)| u).collect();
            Ok(usernames)
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn set_data(&self, key: &str, data: &[u8]) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            // Using postgres syntax ON CONFLICT with fallback for sqlite.
            // Using a simple Delete + Insert for SQLx Any since UPSERT syntax differs between drivers
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM kv_store WHERE key = $1")
                .bind(key)
                .execute(&mut *tx).await?;
            
            sqlx::query("INSERT INTO kv_store (key, value) VALUES ($1, $2)")
                .bind(key)
                .bind(data)
                .execute(&mut *tx).await?;
            tx.commit().await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn get_data(&self, key: &str) -> Result<Option<Vec<u8>>, DatabaseError> {
        if let Some(pool) = &self.pool {
            let row: Option<(Vec<u8>,)> = sqlx::query_as("SELECT value FROM kv_store WHERE key = $1")
                .bind(key)
                .fetch_optional(&**pool).await?;
            Ok(row.map(|r| r.0))
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn delete_data(&self, key: &str) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            sqlx::query("DELETE FROM kv_store WHERE key = $1")
                .bind(key)
                .execute(&**pool).await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn multi_get_data(&self, keys: &[&str]) -> Result<Vec<Option<Vec<u8>>>, DatabaseError> {
        let mut results = Vec::new();
        for key in keys {
            results.push(self.get_data(key).await?);
        }
        Ok(results)
    }

    pub async fn _put_timeseries(&self, key: &[u8], ts: &MetricTimeSeries) -> Result<(), DatabaseError> {
        let key_str = String::from_utf8_lossy(key).to_string();
        let mut ts_buckets: Vec<(u64, MetricTimeSeries)> = Vec::new();
        for ts_bucket in ts.buckets() {
            if let Some(first_ts) = ts_bucket.first_timestamp() {
                ts_buckets.push((first_ts, ts_bucket));
            }
        }
        self._upsert_timeseries_buckets(&key_str, ts_buckets).await
    }

    pub async fn put_metric(
        &self,
        tenant_id: &TenantId,
        device_id: &str,
        metric_name: &str,
        value: MetricValue,
    ) -> Result<(), DatabaseError> {
        let key = format!("{}#{}#{}", tenant_id, device_id, metric_name).into_bytes();
        let generic_ts = value.as_timeseries(chrono::Utc::now().timestamp() as u64);
        self._put_timeseries(&key, &generic_ts).await
    }

    async fn _upsert_timeseries_buckets(
        &self,
        key_str: &str,
        ts_buckets: Vec<(u64, MetricTimeSeries)>,
    ) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            let mut tx = pool.begin().await?;

            for (bucket_timestamp, new_ts) in &ts_buckets {
                let existing: Option<(Vec<u8>,)> = sqlx::query_as(
                    "SELECT data FROM timeseries_buckets WHERE key = $1 AND bucket_timestamp = $2"
                )
                .bind(key_str)
                .bind(*bucket_timestamp as i64)
                .fetch_optional(&mut *tx).await?;

                let mut final_ts = match existing {
                    Some((data,)) => MetricTimeSeries::from_binary(&data)?,
                    None => MetricTimeSeries::new(),
                };

                final_ts.merge(new_ts);
                let ts_data = final_ts.to_binary()?;

                sqlx::query(
                    "DELETE FROM timeseries_buckets WHERE key = $1 AND bucket_timestamp = $2"
                )
                .bind(key_str)
                .bind(*bucket_timestamp as i64)
                .execute(&mut *tx).await?;

                sqlx::query(
                    "INSERT INTO timeseries_buckets (key, bucket_timestamp, data) VALUES ($1, $2, $3)"
                )
                .bind(key_str)
                .bind(*bucket_timestamp as i64)
                .bind(ts_data)
                .execute(&mut *tx).await?;
            }

            tx.commit().await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn _get_timeseries(
        &self,
        key: &[u8],
        min_ts: u64,
        max_ts: u64,
    ) -> Result<MetricTimeSeries, DatabaseError> {
        let mut merged_ts = MetricTimeSeries::new();
        let key_str = String::from_utf8_lossy(key).to_string();

        if let Some(pool) = &self.pool {
            // Find buckets that migh contain data in the range. 
            // We need to query buckets where timestamp + 3600 (approx max bucket size) is >= min_ts
            // and timestamp <= max_ts. To be safe, we just fetch all for the key and trim.
            // A more optimized query would be: bucket_timestamp >= min_ts - 4000 AND bucket_timestamp <= max_ts
            let min_bucket_ts = min_ts.saturating_sub(4000) as i64;
            let max_bucket_ts = max_ts as i64;

            let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
                "SELECT data FROM timeseries_buckets WHERE key = $1 AND bucket_timestamp >= $2 AND bucket_timestamp <= $3 ORDER BY bucket_timestamp ASC"
            )
            .bind(&key_str)
            .bind(min_bucket_ts)
            .bind(max_bucket_ts)
            .fetch_all(&**pool).await?;

            for (value,) in rows {
                match MetricTimeSeries::from_binary(&value) {
                    Ok(ts) => {
                        merged_ts.merge(&ts);
                    }
                    Err(e) => return Err(DatabaseError::TimeseriesSerializationError(e)),
                }
            }
            merged_ts.trim(min_ts, max_ts);
            Ok(merged_ts)
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn get_metric(
        &self,
        tenant_id: &TenantId,
        device_id: &str,
        metric_name: &str,
        start: u64,
        end: u64,
    ) -> Result<MetricTimeSeries, DatabaseError> {
        let key = format!("{}#{}#{}", tenant_id, device_id, metric_name).into_bytes();
        self._get_timeseries(&key, start, end).await
    }

    pub async fn get_last_metric(
        &self,
        tenant_id: &TenantId,
        device_id: &str,
        metric_name: &str,
        limit: u64,
    ) -> Result<MetricTimeSeries, DatabaseError> {
        let key = format!("{}#{}#{}", tenant_id, device_id, metric_name).into_bytes();
        self._get_timeseries_last(&key, limit).await
    }

    pub async fn _get_timeseries_last(
        &self,
        key_prefix: &[u8],
        limit: u64,
    ) -> Result<MetricTimeSeries, DatabaseError> {
        let mut merged_ts = MetricTimeSeries::new();
        let mut key_str = String::from_utf8_lossy(key_prefix).to_string();
        key_str.push('%');

        if let Some(pool) = &self.pool {
            let rows: Vec<(Vec<u8>,)> = sqlx::query_as(
                "SELECT data FROM timeseries_buckets WHERE key LIKE $1 ORDER BY bucket_timestamp DESC LIMIT 10" // Assuming 10 buckets is enough to satisfy the limit
            )
            .bind(&key_str)
            .fetch_all(&**pool).await?;

            let mut count: u64 = 0;
            for (value,) in rows.into_iter().rev() { // reverse to maintain chronological order from db perspective
                 match MetricTimeSeries::from_binary(&value) {
                    Ok(ts) => {
                        merged_ts.merge(&ts);
                        count += merged_ts.len() as u64;
                    }
                    Err(e) => return Err(DatabaseError::TimeseriesSerializationError(e)),
                }
            }
            merged_ts.keep_last(limit as usize);
            Ok(merged_ts)
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn _upsert_shadow(&self, update: &StateUpdateDocument) -> Result<Shadow, DatabaseError> {
        if let Some(pool) = &self.pool {
            let mut tx = pool.begin().await?;
            let tenant_id = update.tenant_id.to_string();
            let shadow_name = update.shadow_name.as_str().to_string();

            let row: Option<(String,)> = sqlx::query_as(
                "SELECT data FROM shadows WHERE tenant_id = $1 AND device_id = $2 AND shadow_name = $3"
            )
            .bind(&tenant_id)
            .bind(&update.device_id)
            .bind(&shadow_name)
            .fetch_optional(&mut *tx).await?;

            let mut shadow = match row {
                Some((shadow_str,)) => Shadow::from_json(&shadow_str)?,
                None => Shadow::new(&update.device_id, &update.shadow_name, &update.tenant_id),
            };

            shadow.update(update)?;
            let shadow_data = shadow.to_json()?;

            sqlx::query(
                "DELETE FROM shadows WHERE tenant_id = $1 AND device_id = $2 AND shadow_name = $3"
            )
            .bind(&tenant_id)
            .bind(&update.device_id)
            .bind(&shadow_name)
            .execute(&mut *tx).await?;

            sqlx::query(
                "INSERT INTO shadows (tenant_id, device_id, shadow_name, data) VALUES ($1, $2, $3, $4)"
            )
            .bind(&tenant_id)
            .bind(&update.device_id)
            .bind(&shadow_name)
            .bind(&shadow_data)
            .execute(&mut *tx).await?;

            tx.commit().await?;
            Ok(shadow)
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn _get_shadow(
        &self,
        device_id: &str,
        shadow_name: &ShadowName,
        tenant_id: &TenantId,
    ) -> Result<Shadow, DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let s_name = shadow_name.as_str().to_string();

            let row: Option<(String,)> = sqlx::query_as(
                "SELECT data FROM shadows WHERE tenant_id = $1 AND device_id = $2 AND shadow_name = $3"
            )
            .bind(&t_id)
            .bind(device_id)
            .bind(&s_name)
            .fetch_optional(&**pool).await?;

            match row {
                Some((shadow_str,)) => Ok(Shadow::from_json(&shadow_str)?),
                None => Err(DatabaseError::NotFoundError(format!(
                    "Shadow not found for device = {} name = {} tenant = {}",
                    device_id, shadow_name, tenant_id
                ))),
            }
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn _delete_shadow(
        &self,
        device_id: &str,
        shadow_name: &ShadowName,
        tenant_id: &TenantId,
    ) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let s_name = shadow_name.as_str().to_string();
            sqlx::query("DELETE FROM shadows WHERE tenant_id = $1 AND device_id = $2 AND shadow_name = $3")
                .bind(&t_id)
                .bind(device_id)
                .bind(&s_name)
                .execute(&**pool).await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn flush(&self) -> Result<(), DatabaseError> {
        // No explicit flush needed for sqlx Any Pool usually
        Ok(())
    }

    pub async fn cancel_all_background_tasks(&self, _wait: Option<bool>) -> Result<(), DatabaseError> {
        // Not applicable for SQLx
        Ok(())
    }

    pub async fn store_tenant_data_config(
        &self,
        tenant_id: &TenantId,
        config: &DataConfig,
    ) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let config_data = config.to_json();
            let mut tx = pool.begin().await?;

            sqlx::query("DELETE FROM data_configs WHERE tenant_id = $1 AND device_prefix = $2")
                .bind(&t_id)
                .bind("")
                .execute(&mut *tx).await?;

            sqlx::query("INSERT INTO data_configs (tenant_id, device_prefix, config) VALUES ($1, $2, $3)")
                .bind(&t_id)
                .bind("")
                .bind(&config_data)
                .execute(&mut *tx).await?;

            tx.commit().await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn store_device_data_config(
        &self,
        tenant_id: &TenantId,
        device_id_prefix: &str,
        config: &DataConfig,
    ) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let config_data = config.to_json();
            let mut tx = pool.begin().await?;

            sqlx::query("DELETE FROM data_configs WHERE tenant_id = $1 AND device_prefix = $2")
                .bind(&t_id)
                .bind(device_id_prefix)
                .execute(&mut *tx).await?;

            sqlx::query("INSERT INTO data_configs (tenant_id, device_prefix, config) VALUES ($1, $2, $3)")
                .bind(&t_id)
                .bind(device_id_prefix)
                .bind(&config_data)
                .execute(&mut *tx).await?;

            tx.commit().await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn get_data_config(
        &self,
        tenant_id: &TenantId,
        device_id: Option<&str>,
    ) -> Result<Option<DataConfig>, DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            
            // Get tenant config
            let tenant_row: Option<(String,)> = sqlx::query_as(
                "SELECT config FROM data_configs WHERE tenant_id = $1 AND device_prefix = $2"
            )
            .bind(&t_id)
            .bind("")
            .fetch_optional(&**pool).await?;

            let maybe_tenant_cfg = tenant_row.map(|(config_str,)| DataConfig::from_json(&config_str));

            if let Some(d_id) = device_id {
                // Find all matching prefixes
                let mut d_id_like = d_id.to_string();
                let rows: Vec<(String, String)> = sqlx::query_as(
                    "SELECT device_prefix, config FROM data_configs WHERE tenant_id = $1 AND device_prefix != $2"
                )
                .bind(&t_id)
                .bind("") // exclude tenant config
                .fetch_all(&**pool).await?;

                // find best matching prefix
                let mut best_match: Option<(usize, DataConfig)> = None;
                for (prefix, config_str) in rows {
                    if d_id_like.starts_with(&prefix) {
                        let len = prefix.len();
                        if best_match.is_none() || len > best_match.as_ref().unwrap().0 {
                            best_match = Some((len, DataConfig::from_json(&config_str)));
                        }
                    }
                }

                if let Some((_, device_cfg)) = best_match {
                     if let Some(tenant_cfg) = maybe_tenant_cfg {
                        return Ok(Some(tenant_cfg.merge_with(&device_cfg)));
                    } else {
                        return Ok(Some(device_cfg));
                    }
                }
            }
            Ok(maybe_tenant_cfg)
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn delete_data_config(
        &self,
        tenant_id: &TenantId,
        device_id_prefix: Option<&str>,
    ) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let pfx = device_id_prefix.unwrap_or_else(|| "");
            sqlx::query("DELETE FROM data_configs WHERE tenant_id = $1 AND device_prefix = $2")
                .bind(&t_id)
                .bind(pfx)
                .execute(&**pool).await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn list_data_configs(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Vec<DataConfigEntry>, DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let rows: Vec<(String, String)> = sqlx::query_as(
                "SELECT device_prefix, config FROM data_configs WHERE tenant_id = $1"
            )
            .bind(&t_id)
            .fetch_all(&**pool).await?;
            
            let mut configs = Vec::new();
            for (prefix, config_str) in rows {
                let config = DataConfig::from_json(&config_str);
                let device_prefix = if prefix.is_empty() { None } else { Some(prefix) };
                configs.push(DataConfigEntry {
                    tenant_id: tenant_id.clone(),
                    device_prefix,
                    metrics: config.metrics,
                });
            }
            Ok(configs)
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }



    pub async fn put_device_metadata(&self, metadata: &DeviceMetadata) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            let mut tx = pool.begin().await?;
            let t_id = metadata.tenant_id.to_string();
            let d_id = metadata.device_id.clone();
            let data = serde_json::to_string(metadata).map_err(|e| {
                DatabaseError::DatabaseValueError(format!("Failed to serialize device metadata: {}", e))
            })?;

            sqlx::query("DELETE FROM device_metadata WHERE tenant_id = $1 AND device_id = $2")
                .bind(&t_id)
                .bind(&d_id)
                .execute(&mut *tx).await?;

            sqlx::query("INSERT INTO device_metadata (tenant_id, device_id, metadata) VALUES ($1, $2, $3)")
                .bind(&t_id)
                .bind(&d_id)
                .bind(&data)
                .execute(&mut *tx).await?;

            tx.commit().await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn get_device_metadata(
        &self,
        tenant_id: &TenantId,
        device_id: &str,
    ) -> Result<Option<DeviceMetadata>, DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let row: Option<(String,)> = sqlx::query_as(
                "SELECT metadata FROM device_metadata WHERE tenant_id = $1 AND device_id = $2"
            )
            .bind(&t_id)
            .bind(device_id)
            .fetch_optional(&**pool).await?;

            match row {
                Some((metadata_str,)) => {
                    let metadata = serde_json::from_str(&metadata_str).map_err(|e| {
                        DatabaseError::DatabaseValueError(format!("Failed to deserialize device metadata: {}", e))
                    })?;
                    Ok(Some(metadata))
                }
                None => Ok(None),
            }
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn list_devices(&self, tenant_id: &TenantId) -> Result<Vec<DeviceMetadata>, DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            let rows: Vec<(String,)> = sqlx::query_as(
                "SELECT metadata FROM device_metadata WHERE tenant_id = $1"
            )
            .bind(&t_id)
            .fetch_all(&**pool).await?;

            let mut devices = Vec::new();
            for (metadata_str,) in rows {
                 match serde_json::from_str(&metadata_str) {
                    Ok(metadata) => devices.push(metadata),
                    Err(e) => return Err(DatabaseError::DatabaseValueError(format!(
                            "Failed to deserialize device metadata: {}",
                            e
                    )))
                }
            }
            Ok(devices)
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }

    pub async fn delete_device_metadata(
        &self,
        tenant_id: &TenantId,
        device_id: &str,
    ) -> Result<(), DatabaseError> {
        if let Some(pool) = &self.pool {
            let t_id = tenant_id.to_string();
            sqlx::query("DELETE FROM device_metadata WHERE tenant_id = $1 AND device_id = $2")
                .bind(&t_id)
                .bind(device_id)
                .execute(&**pool).await?;
            Ok(())
        } else {
            Err(DatabaseError::DatabaseConnectionError)
        }
    }
}

// Backup logic was rockdsdb specific, removed actual impl

#[cfg(test)]
mod tests;

