use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use sqlx::Executor;
use sqlx::QueryBuilder;
use sqlx::Row;
use sqlx::migrate::Migrator;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use thiserror::Error;
use tracing::info;
use wdapm_core::{
    AccountHealthReport, AdminAuditLogInsert, AdminAuditLogRecord, AlertEventRecord, AlertRuleKind,
    AlertRuleRecord, EgressProxy, EgressProxyKind, EgressProxyStatus, EgressProxySummary,
    ProviderAccount, ProviderAccountStatus, ProviderAccountSummary, ProviderAccountUpdate,
    ProviderAsyncJobInsert, ProviderAsyncJobRecord, ProviderAsyncJobState, ProviderAsyncJobUpdate,
    ProviderError, ProviderId, ProviderRequestReport, RequestLogInsert, RequestLogRecord,
};

mod alerts;
mod platform_keys;

static MIGRATOR: Migrator = sqlx::migrate!("../../db/migrations");

#[derive(Clone, Default)]
pub struct RequestLogFilter {
    pub provider: Option<String>,
    pub status_min: Option<i64>,
    pub status_max: Option<i64>,
    pub latency_min: Option<i64>,
    pub latency_max: Option<i64>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub api_key_name: Option<String>,
    pub limit: Option<i64>,
}

pub struct PlatformApiKeyInsert<'a> {
    pub id: &'a str,
    pub tenant_id: &'a str,
    pub name: &'a str,
    pub key_hash: &'a str,
    pub key_prefix: &'a str,
    pub plaintext_key: &'a str,
    pub quota: i64,
}

#[derive(Clone, Debug)]
pub struct SqliteTuning {
    pub max_connections: u32,
    pub busy_timeout: Duration,
    pub cache_size_kib: i64,
    pub mmap_size_bytes: i64,
    pub journal_size_limit_bytes: i64,
}

impl Default for SqliteTuning {
    fn default() -> Self {
        Self {
            max_connections: 8,
            busy_timeout: Duration::from_secs(15),
            cache_size_kib: 16 * 1024,
            mmap_size_bytes: 256 * 1024 * 1024,
            journal_size_limit_bytes: 64 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct SqliteCheckpointStats {
    pub busy: i64,
    pub log_frames: i64,
    pub checkpointed_frames: i64,
}

impl SqliteCheckpointStats {
    pub fn wal_bytes(&self, page_size: i64) -> i64 {
        self.log_frames.saturating_mul(page_size)
    }
}

#[derive(Clone, Debug)]
pub struct ArchivedRequestLog {
    pub id: String,
    pub tenant_id: Option<String>,
    pub platform_api_key_id: Option<String>,
    pub provider_id: String,
    pub provider_account_id: Option<String>,
    pub egress_proxy_id: Option<String>,
    pub method: String,
    pub route: String,
    pub upstream_url: String,
    pub status_code: Option<i64>,
    pub latency_ms: Option<i64>,
    pub failure_kind: Option<String>,
    pub failure_message: Option<String>,
    pub request_headers: String,
    pub response_headers: String,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Default)]
pub struct RequestLogArchiveBatch {
    pub archived_rows: usize,
    pub deleted_rows: usize,
    pub months: Vec<String>,
}

#[derive(Clone)]
pub struct StorageService {
    pool: SqlitePool,
    master_key: [u8; 32],
}

impl StorageService {
    pub async fn connect_with_keys_and_tuning(
        database_url: &str,
        master_key: [u8; 32],
        tuning: SqliteTuning,
    ) -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::from_str(database_url)
            .map_err(|error| StorageError::InvalidDatabaseUrl(error.to_string()))?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .busy_timeout(tuning.busy_timeout)
            .synchronous(SqliteSynchronous::Normal);
        let connection_tuning = tuning.clone();
        let pool = SqlitePoolOptions::new()
            .max_connections(tuning.max_connections)
            .after_connect(move |connection, _meta| {
                let tuning = connection_tuning.clone();
                Box::pin(async move {
                    apply_sqlite_connection_tuning(connection, &tuning).await?;
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;

        MIGRATOR.run(&pool).await?;
        sqlx::query("pragma optimize=0x10002")
            .execute(&pool)
            .await?;

        info!(
            database_url,
            max_connections = tuning.max_connections,
            busy_timeout_ms = tuning.busy_timeout.as_millis(),
            cache_size_kib = tuning.cache_size_kib,
            mmap_size_bytes = tuning.mmap_size_bytes,
            journal_size_limit_bytes = tuning.journal_size_limit_bytes,
            "sqlite storage initialized"
        );
        Ok(Self { pool, master_key })
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn ensure_tenant(
        &self,
        tenant_id: &str,
        tenant_name: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            insert or ignore into tenants (id, name, updated_at)
            values (?, ?, current_timestamp)
            "#,
        )
        .bind(tenant_id)
        .bind(tenant_name)
        .execute(&self.pool)
        .await?;

        info!(tenant_id, tenant_name, "tenant ensured");
        Ok(())
    }

    pub async fn find_provider_account(
        &self,
        account_id: &str,
    ) -> Result<Option<ProviderAccount>, StorageError> {
        let row = sqlx::query(
            r#"
            select
                accounts.id,
                accounts.provider_id,
                accounts.name,
                accounts.base_url,
                accounts.config,
                accounts.enabled,
                accounts.status,
                accounts.last_error,
                accounts.cooldown_until,
                accounts.last_used_at,
                accounts.consecutive_failures,
                accounts.last_status_code,
                accounts.weight,
                accounts.last_failure_at,
                credentials.encrypted_api_key
            from provider_accounts accounts
            inner join provider_account_credentials credentials
                on credentials.provider_account_id = accounts.id
            where accounts.id = ?
            "#,
        )
        .bind(account_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(|row| map_provider_account(row, &self.master_key))
            .transpose()
    }

    pub async fn list_provider_accounts(
        &self,
        provider: Option<ProviderId>,
    ) -> Result<Vec<ProviderAccountSummary>, StorageError> {
        let rows = if let Some(provider) = provider {
            sqlx::query(
                r#"
                select
                    id,
                    provider_id,
                    name,
                    base_url,
                    config,
                    enabled,
                    status,
                    last_error,
                    cooldown_until,
                    last_used_at,
                    consecutive_failures,
                    last_status_code,
                    weight,
                    created_at,
                    updated_at
                from provider_accounts
                where provider_id = ?
                order by provider_id, name, id
                "#,
            )
            .bind(provider.as_str())
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                r#"
                select
                    id,
                    provider_id,
                    name,
                    base_url,
                    config,
                    enabled,
                    status,
                    last_error,
                    cooldown_until,
                    last_used_at,
                    consecutive_failures,
                    last_status_code,
                    weight,
                    created_at,
                    updated_at
                from provider_accounts
                order by provider_id, name, id
                "#,
            )
            .fetch_all(&self.pool)
            .await?
        };

        rows.into_iter().map(map_provider_account_summary).collect()
    }

    pub async fn list_routable_provider_accounts(
        &self,
        provider: ProviderId,
    ) -> Result<Vec<ProviderAccount>, StorageError> {
        let rows = sqlx::query(
            r#"
            select
                accounts.id,
                accounts.provider_id,
                accounts.name,
                accounts.base_url,
                accounts.config,
                accounts.enabled,
                accounts.status,
                accounts.last_error,
                accounts.cooldown_until,
                accounts.last_used_at,
                accounts.consecutive_failures,
                accounts.last_status_code,
                accounts.weight,
                accounts.last_failure_at,
                credentials.encrypted_api_key
            from provider_accounts accounts
            inner join provider_account_credentials credentials
                on credentials.provider_account_id = accounts.id
            where accounts.provider_id = ?
              and accounts.enabled = 1
              and accounts.status != 'disabled'
              and (accounts.cooldown_until is null or accounts.cooldown_until <= current_timestamp)
            order by
                coalesce(accounts.last_used_at, accounts.created_at),
                accounts.name,
                accounts.id
            "#,
        )
        .bind(provider.as_str())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| map_provider_account(row, &self.master_key))
            .collect()
    }

    pub async fn create_provider_account(
        &self,
        account: &ProviderAccount,
    ) -> Result<(), StorageError> {
        let encrypted = encrypt_credential(&account.api_key, &self.master_key);
        let mut transaction = self.pool.begin().await?;

        sqlx::query(
            r#"
            insert into provider_accounts (
                id,
                provider_id,
                name,
                enabled,
                base_url,
                config,
                status,
                last_error,
                cooldown_until,
                last_used_at,
                consecutive_failures,
                last_status_code,
                weight,
                updated_at
            ) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&account.id)
        .bind(account.provider.as_str())
        .bind(&account.name)
        .bind(bool_to_int(account.enabled))
        .bind(account.base_url.as_deref())
        .bind(provider_account_config_json(
            account.reader_base_url.as_deref(),
            account.search_base_url.as_deref(),
        )?)
        .bind(account.status.as_str())
        .bind(account.last_error.as_deref())
        .bind(account.cooldown_until.as_deref())
        .bind(account.last_used_at.as_deref())
        .bind(account.consecutive_failures)
        .bind(account.last_status_code)
        .bind(account.weight)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            insert into provider_account_credentials (
                provider_account_id,
                encrypted_api_key
            ) values (?, ?)
            "#,
        )
        .bind(&account.id)
        .bind(&encrypted)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        info!(
            provider = %account.provider,
            provider_account_id = account.id,
            "provider account created"
        );
        Ok(())
    }

    pub async fn update_provider_account(
        &self,
        account_id: &str,
        update: ProviderAccountUpdate,
    ) -> Result<bool, StorageError> {
        let Some(existing) = self.find_provider_account(account_id).await? else {
            return Ok(false);
        };

        let next_name = update.name.unwrap_or(existing.name);
        let next_api_key = update.api_key.unwrap_or(existing.api_key);
        let encrypted_api_key = encrypt_credential(&next_api_key, &self.master_key);
        let next_base_url = update.base_url.unwrap_or(existing.base_url);
        let next_reader_base_url = update.reader_base_url.unwrap_or(existing.reader_base_url);
        let next_search_base_url = update.search_base_url.unwrap_or(existing.search_base_url);
        let next_enabled = update.enabled.unwrap_or(existing.enabled);
        let next_status = match update.enabled {
            Some(true) => ProviderAccountStatus::Active,
            Some(false) => ProviderAccountStatus::Disabled,
            None => existing.status,
        };
        let next_cooldown_until = match update.enabled {
            Some(_) => None,
            None => existing.cooldown_until,
        };
        let mut transaction = self.pool.begin().await?;

        sqlx::query(
            r#"
            update provider_accounts
            set
                name = ?,
                enabled = ?,
                base_url = ?,
                config = ?,
                status = ?,
                cooldown_until = ?,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(&next_name)
        .bind(bool_to_int(next_enabled))
        .bind(next_base_url.as_deref())
        .bind(provider_account_config_json(
            next_reader_base_url.as_deref(),
            next_search_base_url.as_deref(),
        )?)
        .bind(next_status.as_str())
        .bind(next_cooldown_until.as_deref())
        .bind(account_id)
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            r#"
            update provider_account_credentials
            set
                encrypted_api_key = ?,
                updated_at = current_timestamp
            where provider_account_id = ?
            "#,
        )
        .bind(&encrypted_api_key)
        .bind(account_id)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;

        info!(provider_account_id = account_id, "provider account updated");
        Ok(true)
    }

    pub async fn set_provider_account_enabled(
        &self,
        account_id: &str,
        enabled: bool,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            update provider_accounts
            set
                enabled = ?,
                status = case when ? = 1 then 'active' else 'disabled' end,
                cooldown_until = null,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(bool_to_int(enabled))
        .bind(bool_to_int(enabled))
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        let updated = result.rows_affected() > 0;

        if updated {
            info!(
                provider_account_id = account_id,
                enabled, "provider account toggled"
            );
        }

        Ok(updated)
    }

    pub async fn delete_provider_account(&self, account_id: &str) -> Result<bool, StorageError> {
        let result = sqlx::query("delete from provider_accounts where id = ?")
            .bind(account_id)
            .execute(&self.pool)
            .await?;

        let deleted = result.rows_affected() > 0;

        if deleted {
            info!(provider_account_id = account_id, "provider account deleted");
        }

        Ok(deleted)
    }

    pub async fn record_provider_account_selection(
        &self,
        account_id: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            update provider_accounts
            set
                last_used_at = current_timestamp,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        info!(
            provider_account_id = account_id,
            "provider account selected"
        );
        Ok(())
    }

    pub async fn record_provider_account_success(
        &self,
        account_id: &str,
        status_code: Option<i64>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            update provider_accounts
            set
                status = 'active',
                cooldown_until = null,
                last_error = null,
                consecutive_failures = 0,
                last_failure_at = null,
                last_status_code = ?,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(status_code)
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        info!(
            provider_account_id = account_id,
            status_code, "provider account recovered"
        );
        Ok(())
    }

    pub async fn record_provider_account_cooldown(
        &self,
        account_id: &str,
        cooldown_seconds: i64,
        status_code: Option<i64>,
        reason: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            update provider_accounts
            set
                status = 'cooldown',
                cooldown_until = datetime('now', '+' || ? || ' seconds'),
                last_error = ?,
                consecutive_failures = consecutive_failures + 1,
                last_failure_at = current_timestamp,
                last_status_code = ?,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(cooldown_seconds)
        .bind(reason)
        .bind(status_code)
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        info!(
            provider_account_id = account_id,
            status_code, reason, cooldown_seconds, "provider account cooled down"
        );
        Ok(())
    }

    pub async fn record_provider_account_disabled(
        &self,
        account_id: &str,
        status_code: Option<i64>,
        reason: Option<&str>,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            update provider_accounts
            set
                enabled = 0,
                status = 'disabled',
                cooldown_until = null,
                last_error = ?,
                consecutive_failures = consecutive_failures + 1,
                last_failure_at = current_timestamp,
                last_status_code = ?,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(reason)
        .bind(status_code)
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        info!(
            provider_account_id = account_id,
            status_code, reason, "provider account disabled"
        );
        Ok(())
    }

    pub async fn record_provider_account_failure(
        &self,
        account_id: &str,
        reason: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            update provider_accounts
            set
                last_error = ?,
                consecutive_failures = consecutive_failures + 1,
                last_failure_at = current_timestamp,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(reason)
        .bind(account_id)
        .execute(&self.pool)
        .await?;

        info!(
            provider_account_id = account_id,
            reason, "provider account failed"
        );
        Ok(())
    }

    pub async fn find_egress_proxy(
        &self,
        proxy_id: &str,
    ) -> Result<Option<EgressProxy>, StorageError> {
        let row = sqlx::query(
            r#"
            select
                id,
                name,
                kind,
                proxy_url,
                region,
                enabled,
                status,
                last_error,
                cooldown_until,
                last_used_at,
                consecutive_failures
            from egress_proxies
            where id = ?
            "#,
        )
        .bind(proxy_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(map_egress_proxy).transpose()
    }

    pub async fn list_egress_proxies(&self) -> Result<Vec<EgressProxySummary>, StorageError> {
        let rows = sqlx::query(
            r#"
            select
                id,
                name,
                kind,
                proxy_url,
                region,
                enabled,
                status,
                last_error,
                cooldown_until,
                last_used_at,
                consecutive_failures,
                created_at,
                updated_at
            from egress_proxies
            order by name, id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(map_egress_proxy_summary).collect()
    }

    pub async fn list_bound_egress_proxies(
        &self,
        account_id: &str,
    ) -> Result<Vec<EgressProxy>, StorageError> {
        let rows = sqlx::query(
            r#"
            select
                proxies.id,
                proxies.name,
                proxies.kind,
                proxies.proxy_url,
                proxies.region,
                proxies.enabled,
                proxies.status,
                proxies.last_error,
                proxies.cooldown_until,
                proxies.last_used_at,
                proxies.consecutive_failures
            from egress_proxies proxies
            inner join account_proxy_bindings bindings
                on bindings.egress_proxy_id = proxies.id
            where bindings.provider_account_id = ?
            order by bindings.created_at, proxies.name, proxies.id
            "#,
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(map_egress_proxy).collect()
    }

    pub async fn list_routable_bound_egress_proxies(
        &self,
        account_id: &str,
    ) -> Result<Vec<EgressProxy>, StorageError> {
        let rows = sqlx::query(
            r#"
            select
                proxies.id,
                proxies.name,
                proxies.kind,
                proxies.proxy_url,
                proxies.region,
                proxies.enabled,
                proxies.status,
                proxies.last_error,
                proxies.cooldown_until,
                proxies.last_used_at,
                proxies.consecutive_failures
            from egress_proxies proxies
            inner join account_proxy_bindings bindings
                on bindings.egress_proxy_id = proxies.id
            where bindings.provider_account_id = ?
              and proxies.enabled = 1
              and proxies.status != 'disabled'
              and (proxies.cooldown_until is null or proxies.cooldown_until <= current_timestamp)
            order by
                coalesce(proxies.last_used_at, proxies.created_at),
                bindings.created_at,
                proxies.name,
                proxies.id
            "#,
        )
        .bind(account_id)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(map_egress_proxy).collect()
    }

    pub async fn create_egress_proxy(&self, proxy: &EgressProxy) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            insert into egress_proxies (
                id,
                name,
                proxy_url,
                region,
                enabled,
                kind,
                status,
                last_error,
                cooldown_until,
                last_used_at,
                consecutive_failures,
                updated_at
            ) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(&proxy.id)
        .bind(&proxy.name)
        .bind(&proxy.proxy_url)
        .bind(proxy.region.as_deref())
        .bind(bool_to_int(proxy.enabled))
        .bind(proxy.kind.as_str())
        .bind(proxy.status.as_str())
        .bind(proxy.last_error.as_deref())
        .bind(proxy.cooldown_until.as_deref())
        .bind(proxy.last_used_at.as_deref())
        .bind(proxy.consecutive_failures)
        .execute(&self.pool)
        .await?;

        info!(egress_proxy_id = %proxy.id, kind = proxy.kind.as_str(), "egress proxy created");
        Ok(())
    }

    pub async fn update_egress_proxy(
        &self,
        proxy_id: &str,
        name: Option<String>,
        proxy_url: Option<String>,
        region: Option<Option<String>>,
        enabled: Option<bool>,
    ) -> Result<bool, StorageError> {
        let Some(existing) = self.find_egress_proxy(proxy_id).await? else {
            return Ok(false);
        };

        let next_name = name.unwrap_or(existing.name);
        let next_proxy_url = proxy_url.unwrap_or(existing.proxy_url);
        let next_region = region.unwrap_or(existing.region);
        let next_enabled = enabled.unwrap_or(existing.enabled);
        let next_kind = EgressProxyKind::from_proxy_url(&next_proxy_url)?;
        let next_status = match enabled {
            Some(true) => EgressProxyStatus::Active,
            Some(false) => EgressProxyStatus::Disabled,
            None => existing.status,
        };
        let next_cooldown_until = match enabled {
            Some(_) => None,
            None => existing.cooldown_until,
        };

        sqlx::query(
            r#"
            update egress_proxies
            set
                name = ?,
                proxy_url = ?,
                region = ?,
                enabled = ?,
                kind = ?,
                status = ?,
                cooldown_until = ?,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(&next_name)
        .bind(&next_proxy_url)
        .bind(next_region.as_deref())
        .bind(bool_to_int(next_enabled))
        .bind(next_kind.as_str())
        .bind(next_status.as_str())
        .bind(next_cooldown_until.as_deref())
        .bind(proxy_id)
        .execute(&self.pool)
        .await?;

        info!(
            egress_proxy_id = proxy_id,
            kind = next_kind.as_str(),
            "egress proxy updated"
        );
        Ok(true)
    }

    pub async fn bind_account_proxy(
        &self,
        account_id: &str,
        proxy_id: &str,
    ) -> Result<(), StorageError> {
        let binding_id = format!("{account_id}:{proxy_id}");
        sqlx::query(
            r#"
            insert or ignore into account_proxy_bindings (id, provider_account_id, egress_proxy_id)
            values (?, ?, ?)
            "#,
        )
        .bind(&binding_id)
        .bind(account_id)
        .bind(proxy_id)
        .execute(&self.pool)
        .await?;

        info!(
            provider_account_id = account_id,
            egress_proxy_id = proxy_id,
            "account proxy bound"
        );
        Ok(())
    }

    pub async fn record_egress_proxy_selection(&self, proxy_id: &str) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            update egress_proxies
            set
                last_used_at = current_timestamp,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(proxy_id)
        .execute(&self.pool)
        .await?;

        info!(egress_proxy_id = proxy_id, "egress proxy selected");
        Ok(())
    }

    pub async fn record_egress_proxy_success(&self, proxy_id: &str) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            update egress_proxies
            set
                status = 'active',
                cooldown_until = null,
                last_error = null,
                consecutive_failures = 0,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(proxy_id)
        .execute(&self.pool)
        .await?;

        info!(egress_proxy_id = proxy_id, "egress proxy recovered");
        Ok(())
    }

    pub async fn record_egress_proxy_cooldown(
        &self,
        proxy_id: &str,
        cooldown_seconds: i64,
        reason: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            update egress_proxies
            set
                status = 'cooldown',
                cooldown_until = datetime('now', '+' || ? || ' seconds'),
                last_error = ?,
                consecutive_failures = consecutive_failures + 1,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(cooldown_seconds)
        .bind(reason)
        .bind(proxy_id)
        .execute(&self.pool)
        .await?;

        info!(
            egress_proxy_id = proxy_id,
            reason, cooldown_seconds, "egress proxy cooled down"
        );
        Ok(())
    }

    pub async fn record_egress_proxy_failure(
        &self,
        proxy_id: &str,
        reason: &str,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            update egress_proxies
            set
                last_error = ?,
                consecutive_failures = consecutive_failures + 1,
                updated_at = current_timestamp
            where id = ?
            "#,
        )
        .bind(reason)
        .bind(proxy_id)
        .execute(&self.pool)
        .await?;

        info!(egress_proxy_id = proxy_id, reason, "egress proxy failed");
        Ok(())
    }

    pub async fn list_request_logs(
        &self,
        limit: i64,
    ) -> Result<Vec<RequestLogRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            select
                id,
                tenant_id,
                platform_api_key_id,
                provider_id,
                provider_account_id,
                egress_proxy_id,
                method,
                route,
                upstream_url,
                status_code,
                latency_ms,
                failure_kind,
                failure_message,
                created_at
            from request_logs
            order by created_at desc, id desc
            limit ?
            "#,
        )
        .bind(normalize_limit(limit))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(map_request_log_record).collect()
    }

    pub async fn list_request_logs_filtered(
        &self,
        filter: &RequestLogFilter,
    ) -> Result<Vec<RequestLogRecord>, StorageError> {
        let mut qb: QueryBuilder<'_, sqlx::Sqlite> = QueryBuilder::new(
            r#"select
                rl.id,
                rl.tenant_id,
                rl.platform_api_key_id,
                rl.provider_id,
                rl.provider_account_id,
                rl.egress_proxy_id,
                rl.method,
                rl.route,
                rl.upstream_url,
                rl.status_code,
                rl.latency_ms,
                rl.failure_kind,
                rl.failure_message,
                rl.created_at
            from request_logs rl"#,
        );

        if filter.api_key_name.is_some() {
            qb.push(" join platform_api_keys pak on pak.id = rl.platform_api_key_id");
        }

        let mut is_first = true;
        let and = |qb: &mut QueryBuilder<'_, sqlx::Sqlite>, first: &mut bool| {
            if *first {
                qb.push(" where ");
                *first = false;
            } else {
                qb.push(" and ");
            }
        };

        if let Some(p) = &filter.provider {
            and(&mut qb, &mut is_first);
            qb.push("rl.provider_id = ");
            qb.push_bind(p.clone());
        }
        if let Some(v) = filter.status_min {
            and(&mut qb, &mut is_first);
            qb.push("rl.status_code >= ");
            qb.push_bind(v);
        }
        if let Some(v) = filter.status_max {
            and(&mut qb, &mut is_first);
            qb.push("rl.status_code <= ");
            qb.push_bind(v);
        }
        if let Some(v) = filter.latency_min {
            and(&mut qb, &mut is_first);
            qb.push("rl.latency_ms >= ");
            qb.push_bind(v);
        }
        if let Some(v) = filter.latency_max {
            and(&mut qb, &mut is_first);
            qb.push("rl.latency_ms <= ");
            qb.push_bind(v);
        }
        if let Some(v) = &filter.since {
            and(&mut qb, &mut is_first);
            qb.push("rl.created_at >= ");
            qb.push_bind(v.clone());
        }
        if let Some(v) = &filter.until {
            and(&mut qb, &mut is_first);
            qb.push("rl.created_at <= ");
            qb.push_bind(v.clone());
        }
        if let Some(v) = &filter.api_key_name {
            and(&mut qb, &mut is_first);
            qb.push("pak.name like '%' || ");
            qb.push_bind(v.clone());
            qb.push(" || '%'");
        }

        qb.push(" order by rl.created_at desc, rl.id desc limit ");
        qb.push_bind(normalize_limit(filter.limit.unwrap_or(100)));

        let rows = qb.build().fetch_all(&self.pool).await?;

        rows.into_iter().map(map_request_log_record).collect()
    }

    pub async fn list_provider_async_jobs(
        &self,
        provider: Option<ProviderId>,
        state: Option<ProviderAsyncJobState>,
        limit: i64,
    ) -> Result<Vec<ProviderAsyncJobRecord>, StorageError> {
        let rows = match (provider, state) {
            (Some(provider), Some(state)) => {
                sqlx::query(
                    r#"
                    select
                        id,
                        tenant_id,
                        request_log_id,
                        provider_id,
                        provider_account_id,
                        egress_proxy_id,
                        route,
                        upstream_job_id,
                        state,
                        last_status_code,
                        last_error,
                        poll_attempts,
                        next_poll_at,
                        settled_at,
                        metadata,
                        webhook_secret,
                        created_at,
                        updated_at
                    from provider_async_jobs
                    where provider_id = ?
                      and state = ?
                    order by created_at desc, id desc
                    limit ?
                    "#,
                )
                .bind(provider.as_str())
                .bind(state.as_str())
                .bind(normalize_limit(limit))
                .fetch_all(&self.pool)
                .await?
            }
            (Some(provider), None) => {
                sqlx::query(
                    r#"
                    select
                        id,
                        tenant_id,
                        request_log_id,
                        provider_id,
                        provider_account_id,
                        egress_proxy_id,
                        route,
                        upstream_job_id,
                        state,
                        last_status_code,
                        last_error,
                        poll_attempts,
                        next_poll_at,
                        settled_at,
                        metadata,
                        webhook_secret,
                        created_at,
                        updated_at
                    from provider_async_jobs
                    where provider_id = ?
                    order by created_at desc, id desc
                    limit ?
                    "#,
                )
                .bind(provider.as_str())
                .bind(normalize_limit(limit))
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(state)) => {
                sqlx::query(
                    r#"
                    select
                        id,
                        tenant_id,
                        request_log_id,
                        provider_id,
                        provider_account_id,
                        egress_proxy_id,
                        route,
                        upstream_job_id,
                        state,
                        last_status_code,
                        last_error,
                        poll_attempts,
                        next_poll_at,
                        settled_at,
                        metadata,
                        webhook_secret,
                        created_at,
                        updated_at
                    from provider_async_jobs
                    where state = ?
                    order by created_at desc, id desc
                    limit ?
                    "#,
                )
                .bind(state.as_str())
                .bind(normalize_limit(limit))
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    r#"
                    select
                        id,
                        tenant_id,
                        request_log_id,
                        provider_id,
                        provider_account_id,
                        egress_proxy_id,
                        route,
                        upstream_job_id,
                        state,
                        last_status_code,
                        last_error,
                        poll_attempts,
                        next_poll_at,
                        settled_at,
                        metadata,
                        webhook_secret,
                        created_at,
                        updated_at
                    from provider_async_jobs
                    order by created_at desc, id desc
                    limit ?
                    "#,
                )
                .bind(normalize_limit(limit))
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.into_iter()
            .map(map_provider_async_job_record)
            .collect()
    }

    pub async fn list_due_provider_async_jobs(
        &self,
        provider: ProviderId,
        limit: i64,
    ) -> Result<Vec<ProviderAsyncJobRecord>, StorageError> {
        let rows = sqlx::query(
            r#"
            select
                id,
                tenant_id,
                request_log_id,
                provider_id,
                provider_account_id,
                egress_proxy_id,
                route,
                upstream_job_id,
                state,
                last_status_code,
                last_error,
                poll_attempts,
                next_poll_at,
                settled_at,
                metadata,
                webhook_secret,
                created_at,
                updated_at
            from provider_async_jobs
            where provider_id = ?
              and state in ('pending', 'running')
              and (next_poll_at is null or next_poll_at <= current_timestamp)
            order by coalesce(next_poll_at, created_at), created_at, id
            limit ?
            "#,
        )
        .bind(provider.as_str())
        .bind(normalize_limit(limit))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(map_provider_async_job_record)
            .collect()
    }

    pub async fn find_provider_async_job_by_upstream_id(
        &self,
        provider: ProviderId,
        upstream_job_id: &str,
    ) -> Result<Option<ProviderAsyncJobRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            select
                id,
                tenant_id,
                request_log_id,
                provider_id,
                provider_account_id,
                egress_proxy_id,
                route,
                upstream_job_id,
                state,
                last_status_code,
                last_error,
                poll_attempts,
                next_poll_at,
                settled_at,
                metadata,
                webhook_secret,
                created_at,
                updated_at
            from provider_async_jobs
            where provider_id = ?
              and upstream_job_id = ?
            limit 1
            "#,
        )
        .bind(provider.as_str())
        .bind(upstream_job_id)
        .fetch_optional(&self.pool)
        .await?;

        row.map(map_provider_async_job_record).transpose()
    }

    pub async fn insert_provider_async_job(
        &self,
        entry: &ProviderAsyncJobInsert,
    ) -> Result<(), StorageError> {
        sqlx::query(
            r#"
            insert into provider_async_jobs (
                id,
                tenant_id,
                request_log_id,
                provider_id,
                provider_account_id,
                egress_proxy_id,
                route,
                upstream_job_id,
                state,
                last_status_code,
                last_error,
                poll_attempts,
                next_poll_at,
                settled_at,
                metadata,
                webhook_secret,
                updated_at
            ) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)
            "#,
        )
        .bind(entry.id.to_string())
        .bind(entry.tenant_id.as_deref())
        .bind(entry.request_log_id.map(|value| value.to_string()))
        .bind(entry.provider.as_str())
        .bind(entry.provider_account_id.as_deref())
        .bind(entry.egress_proxy_id.as_deref())
        .bind(&entry.route)
        .bind(&entry.upstream_job_id)
        .bind(entry.state.as_str())
        .bind(entry.last_status_code)
        .bind(entry.last_error.as_deref())
        .bind(entry.poll_attempts)
        .bind(entry.next_poll_at.as_deref())
        .bind(entry.settled_at.as_deref())
        .bind(entry.metadata.to_string())
        .bind(entry.webhook_secret.as_deref())
        .execute(&self.pool)
        .await?;

        info!(
            async_job_id = %entry.id,
            provider = %entry.provider,
            upstream_job_id = entry.upstream_job_id.as_str(),
            state = entry.state.as_str(),
            "provider async job inserted"
        );
        Ok(())
    }

    pub async fn update_provider_async_job(
        &self,
        job_id: &str,
        update: &ProviderAsyncJobUpdate,
    ) -> Result<bool, StorageError> {
        let result = sqlx::query(
            r#"
            update provider_async_jobs
            set
                state = ?,
                last_status_code = ?,
                last_error = ?,
                poll_attempts = poll_attempts + ?,
                next_poll_at = ?,
                settled_at = ?,
                metadata = ?,
                updated_at = current_timestamp
            where id = ?
              and state not in ('completed', 'failed', 'cancelled')
            "#,
        )
        .bind(update.state.as_str())
        .bind(update.status_code)
        .bind(update.last_error.as_deref())
        .bind(update.poll_attempt_increment.max(0))
        .bind(update.next_poll_at.as_deref())
        .bind(update.settled_at.as_deref())
        .bind(update.metadata.to_string())
        .bind(job_id)
        .execute(&self.pool)
        .await?;

        let updated = result.rows_affected() > 0;
        info!(
            async_job_id = job_id,
            state = update.state.as_str(),
            status_code = update.status_code,
            next_poll_at = update.next_poll_at.as_deref(),
            settled_at = update.settled_at.as_deref(),
            updated,
            "provider async job updated"
        );
        Ok(updated)
    }

    pub async fn find_provider_async_job_by_webhook_secret(
        &self,
        webhook_secret: &str,
    ) -> Result<Option<ProviderAsyncJobRecord>, StorageError> {
        let row = sqlx::query(
            r#"
            select
                id,
                tenant_id,
                request_log_id,
                provider_id,
                provider_account_id,
                egress_proxy_id,
                route,
                upstream_job_id,
                state,
                last_status_code,
                last_error,
                poll_attempts,
                next_poll_at,
                settled_at,
                metadata,
                webhook_secret,
                created_at,
                updated_at
            from provider_async_jobs
            where webhook_secret = ?
            limit 1
            "#,
        )
        .bind(webhook_secret)
        .fetch_optional(&self.pool)
        .await?;

        row.map(map_provider_async_job_record).transpose()
    }

    pub async fn insert_request_log(&self, entry: &RequestLogInsert) -> Result<(), StorageError> {
        let request_headers = serde_json::to_string(&entry.request_headers)?;
        let response_headers = serde_json::to_string(&entry.response_headers)?;

        sqlx::query(
            r#"
            insert into request_logs (
                id,
                tenant_id,
                platform_api_key_id,
                provider_id,
                provider_account_id,
                egress_proxy_id,
                method,
                route,
                upstream_url,
                status_code,
                latency_ms,
                failure_kind,
                failure_message,
                request_headers,
                response_headers,
                request_body,
                response_body
            ) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(entry.id.to_string())
        .bind(entry.tenant_id.as_deref())
        .bind(entry.platform_api_key_id.as_deref())
        .bind(entry.provider.as_str())
        .bind(entry.provider_account_id.as_deref())
        .bind(entry.egress_proxy_id.as_deref())
        .bind(&entry.method)
        .bind(&entry.route)
        .bind(&entry.upstream_url)
        .bind(entry.status_code)
        .bind(entry.latency_ms)
        .bind(entry.failure_kind.as_deref())
        .bind(entry.failure_message.as_deref())
        .bind(request_headers)
        .bind(response_headers)
        .bind(entry.request_body.as_deref())
        .bind(entry.response_body.as_deref())
        .execute(&self.pool)
        .await?;

        info!(
            request_log_id = %entry.id,
            provider = %entry.provider,
            egress_proxy_id = entry.egress_proxy_id.as_deref(),
            status_code = entry.status_code,
            "request log inserted"
        );
        Ok(())
    }

    pub async fn insert_admin_audit_log(
        &self,
        entry: &AdminAuditLogInsert,
    ) -> Result<(), StorageError> {
        let old_value = entry.old_value.as_ref().map(|v| v.to_string());
        let new_value = entry.new_value.as_ref().map(|v| v.to_string());
        sqlx::query(
            r#"
            insert into admin_audit_logs (
                id, admin_identity, action, resource_type, resource_id, old_value, new_value
            ) values (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(entry.id.to_string())
        .bind(&entry.admin_identity)
        .bind(&entry.action)
        .bind(&entry.resource_type)
        .bind(&entry.resource_id)
        .bind(&old_value)
        .bind(&new_value)
        .execute(&self.pool)
        .await?;

        info!(
            audit_log_id = %entry.id,
            action = entry.action.as_str(),
            resource_type = entry.resource_type.as_str(),
            "admin audit log inserted"
        );
        Ok(())
    }

    pub async fn list_admin_audit_logs(
        &self,
        since: Option<&str>,
        until: Option<&str>,
        resource_type: Option<&str>,
        limit: i64,
    ) -> Result<Vec<AdminAuditLogRecord>, StorageError> {
        let limit = normalize_limit(limit);
        let rows = sqlx::query(
            r#"
            select id, admin_identity, action, resource_type, resource_id,
                   old_value, new_value, created_at
            from admin_audit_logs
            where (? is null or created_at >= ?)
              and (? is null or created_at <= ?)
              and (? is null or resource_type = ?)
            order by created_at desc
            limit ?
            "#,
        )
        .bind(since)
        .bind(since)
        .bind(until)
        .bind(until)
        .bind(resource_type)
        .bind(resource_type)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter().map(map_admin_audit_log).collect()
    }
    pub async fn report_requests_by_provider(
        &self,
        since: Option<&str>,
        until: Option<&str>,
    ) -> Result<Vec<ProviderRequestReport>, StorageError> {
        let rows: Vec<sqlx::sqlite::SqliteRow> = sqlx::query(
            r#"
            select
                provider_id as provider,
                count(*) as total_requests,
                sum(case when status_code between 200 and 299 and failure_kind is null then 1 else 0 end) as success_count,
                sum(case when failure_kind is not null or status_code < 200 or status_code >= 300 then 1 else 0 end) as error_count,
                avg(latency_ms) as avg_latency_ms
            from request_logs
            where (? is null or created_at >= ?)
              and (? is null or created_at <= ?)
            group by provider_id
            order by total_requests desc
            "#,
        )
        .bind(since)
        .bind(since)
        .bind(until)
        .bind(until)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ProviderRequestReport {
                    provider: row.try_get("provider")?,
                    total_requests: row.try_get("total_requests")?,
                    success_count: row.try_get("success_count")?,
                    error_count: row.try_get("error_count")?,
                    avg_latency_ms: row.try_get("avg_latency_ms")?,
                })
            })
            .collect()
    }

    pub async fn report_account_health(&self) -> Result<Vec<AccountHealthReport>, StorageError> {
        let rows: Vec<sqlx::sqlite::SqliteRow> = sqlx::query(
            r#"
            select
                id, provider_id as provider, name,
                enabled, status, consecutive_failures, weight,
                last_used_at, last_error
            from provider_accounts
            order by provider_id, name
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(AccountHealthReport {
                    id: row.try_get("id")?,
                    provider: row.try_get("provider")?,
                    name: row.try_get("name")?,
                    enabled: int_to_bool(row.try_get::<i64, _>("enabled")?),
                    status: row.try_get("status")?,
                    consecutive_failures: row.try_get("consecutive_failures")?,
                    weight: row.try_get("weight")?,
                    last_used_at: row.try_get("last_used_at")?,
                    last_error: row.try_get("last_error")?,
                })
            })
            .collect()
    }

    pub async fn count_active_provider_accounts_by_provider(
        &self,
    ) -> Result<Vec<(String, i64)>, StorageError> {
        let rows: Vec<sqlx::sqlite::SqliteRow> = sqlx::query(
            r#"
            select provider_id, count(*) as cnt
            from provider_accounts
            where enabled = 1 and status = 'active'
            group by provider_id
            "#,
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<String, _>("provider_id")?,
                    row.try_get::<i64, _>("cnt")?,
                ))
            })
            .collect()
    }

    pub async fn count_recently_disabled_accounts(&self, since: &str) -> Result<i64, StorageError> {
        let row: (i64,) = sqlx::query_as(
            r#"
            select count(*) from provider_accounts
            where enabled = 0
              and status = 'disabled'
              and last_failure_at is not null
              and last_failure_at >= ?
            "#,
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn error_rate_since(&self, since: &str) -> Result<Option<f64>, StorageError> {
        let row: (i64, i64) = sqlx::query_as(
            r#"
            select
                count(*) as total,
                coalesce(sum(case when failure_kind is not null or status_code < 200 or status_code >= 300 then 1 else 0 end), 0) as errors
            from request_logs
            where created_at >= ?
            "#,
        )
        .bind(since)
        .fetch_one(&self.pool)
        .await?;
        if row.0 == 0 {
            Ok(None)
        } else {
            Ok(Some(row.1 as f64 / row.0 as f64 * 100.0))
        }
    }

    pub async fn count_stale_async_jobs(&self, stale_before: &str) -> Result<i64, StorageError> {
        let row: (i64,) = sqlx::query_as(
            r#"
            select count(*) from provider_async_jobs
            where state in ('pending', 'running')
              and coalesce(next_poll_at, updated_at, created_at) < ?
            "#,
        )
        .bind(stale_before)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.0)
    }

    pub async fn check_db_connection(&self) -> Result<(), StorageError> {
        sqlx::query("select 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn run_sqlite_optimize(&self) -> Result<(), StorageError> {
        sqlx::query("pragma optimize").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn sqlite_page_size(&self) -> Result<i64, StorageError> {
        let row = sqlx::query("pragma page_size")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.try_get(0)?)
    }

    pub async fn sqlite_checkpoint_noop(
        &self,
    ) -> Result<Option<SqliteCheckpointStats>, StorageError> {
        match sqlx::query("pragma wal_checkpoint(noop)")
            .fetch_one(&self.pool)
            .await
        {
            Ok(row) => Ok(Some(SqliteCheckpointStats {
                busy: row.try_get(0)?,
                log_frames: row.try_get(1)?,
                checkpointed_frames: row.try_get(2)?,
            })),
            Err(sqlx::Error::Database(error))
                if error.message().contains("syntax error")
                    || error.message().contains("near \"noop\"")
                    || error.message().contains("no such")
                    || error.message().contains("unknown")
                    || error.message().contains("unsupported") =>
            {
                Ok(None)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub async fn run_sqlite_checkpoint_passive(
        &self,
    ) -> Result<SqliteCheckpointStats, StorageError> {
        let row = sqlx::query("pragma wal_checkpoint(passive)")
            .fetch_one(&self.pool)
            .await?;
        Ok(SqliteCheckpointStats {
            busy: row.try_get(0)?,
            log_frames: row.try_get(1)?,
            checkpointed_frames: row.try_get(2)?,
        })
    }

    pub async fn run_sqlite_checkpoint_truncate(
        &self,
    ) -> Result<SqliteCheckpointStats, StorageError> {
        let row = sqlx::query("pragma wal_checkpoint(truncate)")
            .fetch_one(&self.pool)
            .await?;
        Ok(SqliteCheckpointStats {
            busy: row.try_get(0)?,
            log_frames: row.try_get(1)?,
            checkpointed_frames: row.try_get(2)?,
        })
    }

    pub async fn run_sqlite_incremental_vacuum(&self, pages: i64) -> Result<(), StorageError> {
        let clamped_pages = pages.max(0);
        let statement = format!("pragma incremental_vacuum({clamped_pages})");
        sqlx::query(&statement).execute(&self.pool).await?;
        Ok(())
    }

    pub async fn archive_request_logs_before(
        &self,
        archive_dir: &Path,
        before: &str,
        batch_size: i64,
    ) -> Result<RequestLogArchiveBatch, StorageError> {
        let rows = self
            .list_request_logs_for_archival(before, batch_size)
            .await?;
        archive_rows_to_partitions(self, archive_dir, rows).await
    }

    pub async fn archive_request_logs_older_than_hours(
        &self,
        archive_dir: &Path,
        hours: i64,
        batch_size: i64,
    ) -> Result<RequestLogArchiveBatch, StorageError> {
        let modifier = format!("-{} hours", hours.max(1));
        let rows = sqlx::query(
            r#"
            select
                id,
                tenant_id,
                platform_api_key_id,
                provider_id,
                provider_account_id,
                egress_proxy_id,
                method,
                route,
                upstream_url,
                status_code,
                latency_ms,
                failure_kind,
                failure_message,
                request_headers,
                response_headers,
                request_body,
                response_body,
                created_at
            from request_logs
            where created_at < datetime('now', ?)
            order by created_at asc, id asc
            limit ?
            "#,
        )
        .bind(modifier)
        .bind(normalize_limit(batch_size))
        .fetch_all(&self.pool)
        .await?;

        let rows = rows
            .into_iter()
            .map(|row| {
                Ok(ArchivedRequestLog {
                    id: row.try_get("id")?,
                    tenant_id: row.try_get("tenant_id")?,
                    platform_api_key_id: row.try_get("platform_api_key_id")?,
                    provider_id: row.try_get("provider_id")?,
                    provider_account_id: row.try_get("provider_account_id")?,
                    egress_proxy_id: row.try_get("egress_proxy_id")?,
                    method: row.try_get("method")?,
                    route: row.try_get("route")?,
                    upstream_url: row.try_get("upstream_url")?,
                    status_code: row.try_get("status_code")?,
                    latency_ms: row.try_get("latency_ms")?,
                    failure_kind: row.try_get("failure_kind")?,
                    failure_message: row.try_get("failure_message")?,
                    request_headers: row.try_get("request_headers")?,
                    response_headers: row.try_get("response_headers")?,
                    request_body: row.try_get("request_body")?,
                    response_body: row.try_get("response_body")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect::<Result<Vec<_>, StorageError>>()?;
        archive_rows_to_partitions(self, archive_dir, rows).await
    }

    pub async fn prune_request_logs_before(
        &self,
        before: &str,
        batch_size: i64,
    ) -> Result<u64, StorageError> {
        let result = sqlx::query(
            r#"
            delete from request_logs
            where id in (
                select id
                from request_logs
                where created_at < ?
                order by created_at asc, id asc
                limit ?
            )
            "#,
        )
        .bind(before)
        .bind(normalize_limit(batch_size))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn prune_request_logs_older_than_hours(
        &self,
        hours: i64,
        batch_size: i64,
    ) -> Result<u64, StorageError> {
        let modifier = format!("-{} hours", hours.max(1));
        let result = sqlx::query(
            r#"
            delete from request_logs
            where id in (
                select id
                from request_logs
                where created_at < datetime('now', ?)
                order by created_at asc, id asc
                limit ?
            )
            "#,
        )
        .bind(modifier)
        .bind(normalize_limit(batch_size))
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    async fn list_request_logs_for_archival(
        &self,
        before: &str,
        batch_size: i64,
    ) -> Result<Vec<ArchivedRequestLog>, StorageError> {
        let rows = sqlx::query(
            r#"
            select
                id,
                tenant_id,
                platform_api_key_id,
                provider_id,
                provider_account_id,
                egress_proxy_id,
                method,
                route,
                upstream_url,
                status_code,
                latency_ms,
                failure_kind,
                failure_message,
                request_headers,
                response_headers,
                request_body,
                response_body,
                created_at
            from request_logs
            where created_at < ?
            order by created_at asc, id asc
            limit ?
            "#,
        )
        .bind(before)
        .bind(normalize_limit(batch_size))
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                Ok(ArchivedRequestLog {
                    id: row.try_get("id")?,
                    tenant_id: row.try_get("tenant_id")?,
                    platform_api_key_id: row.try_get("platform_api_key_id")?,
                    provider_id: row.try_get("provider_id")?,
                    provider_account_id: row.try_get("provider_account_id")?,
                    egress_proxy_id: row.try_get("egress_proxy_id")?,
                    method: row.try_get("method")?,
                    route: row.try_get("route")?,
                    upstream_url: row.try_get("upstream_url")?,
                    status_code: row.try_get("status_code")?,
                    latency_ms: row.try_get("latency_ms")?,
                    failure_kind: row.try_get("failure_kind")?,
                    failure_message: row.try_get("failure_message")?,
                    request_headers: row.try_get("request_headers")?,
                    response_headers: row.try_get("response_headers")?,
                    request_body: row.try_get("request_body")?,
                    response_body: row.try_get("response_body")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    async fn delete_request_logs_by_ids(&self, ids: &[String]) -> Result<(), StorageError> {
        if ids.is_empty() {
            return Ok(());
        }

        let mut builder: QueryBuilder<'_, sqlx::Sqlite> =
            QueryBuilder::new("delete from request_logs where id in (");
        {
            let mut separated = builder.separated(", ");
            for id in ids {
                separated.push_bind(id);
            }
        }
        builder.push(")");
        builder.build().execute(&self.pool).await?;
        Ok(())
    }
}

async fn apply_sqlite_connection_tuning(
    connection: &mut sqlx::sqlite::SqliteConnection,
    tuning: &SqliteTuning,
) -> Result<(), sqlx::Error> {
    let cache_size = -tuning.cache_size_kib.max(1);
    let mmap_size = tuning.mmap_size_bytes.max(0);
    let journal_size_limit = tuning.journal_size_limit_bytes.max(0);

    connection.execute("pragma temp_store=memory").await?;
    sqlx::query(&format!("pragma cache_size={cache_size}"))
        .execute(&mut *connection)
        .await?;
    sqlx::query(&format!("pragma mmap_size={mmap_size}"))
        .execute(&mut *connection)
        .await?;
    sqlx::query(&format!("pragma journal_size_limit={journal_size_limit}"))
        .execute(&mut *connection)
        .await?;
    sqlx::query("pragma wal_autocheckpoint=0")
        .execute(&mut *connection)
        .await?;
    let _ = sqlx::query("pragma auto_vacuum=incremental")
        .execute(&mut *connection)
        .await;
    Ok(())
}

fn archive_partition_from_timestamp(value: &str) -> String {
    let prefix = value.get(..7).unwrap_or("unknown");
    prefix.to_owned()
}

async fn archive_rows_to_partitions(
    storage: &StorageService,
    archive_dir: &Path,
    rows: Vec<ArchivedRequestLog>,
) -> Result<RequestLogArchiveBatch, StorageError> {
    if rows.is_empty() {
        return Ok(RequestLogArchiveBatch::default());
    }

    std::fs::create_dir_all(archive_dir)?;
    let mut archive_batch = RequestLogArchiveBatch::default();
    let mut current_month = String::new();
    let mut current_rows = Vec::new();

    for row in rows {
        let month = archive_partition_from_timestamp(&row.created_at);
        if current_month.is_empty() {
            current_month = month.clone();
        }
        if month != current_month {
            let ids =
                archive_request_log_partition(archive_dir, &current_month, &current_rows).await?;
            storage.delete_request_logs_by_ids(&ids).await?;
            archive_batch.archived_rows += ids.len();
            archive_batch.deleted_rows += ids.len();
            archive_batch.months.push(current_month.clone());
            current_rows.clear();
            current_month = month;
        }
        current_rows.push(row);
    }

    if !current_rows.is_empty() {
        let ids = archive_request_log_partition(archive_dir, &current_month, &current_rows).await?;
        storage.delete_request_logs_by_ids(&ids).await?;
        archive_batch.archived_rows += ids.len();
        archive_batch.deleted_rows += ids.len();
        archive_batch.months.push(current_month);
    }

    Ok(archive_batch)
}

async fn archive_request_log_partition(
    archive_dir: &Path,
    month: &str,
    rows: &[ArchivedRequestLog],
) -> Result<Vec<String>, StorageError> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let file_name = format!("request-logs-{month}.sqlite3");
    let archive_path = archive_dir.join(file_name);
    let archive_url = format!("sqlite://{}", archive_path.display());
    let options = SqliteConnectOptions::from_str(&archive_url)
        .map_err(|error| StorageError::InvalidDatabaseUrl(error.to_string()))?
        .create_if_missing(true)
        .foreign_keys(false)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(15))
        .synchronous(SqliteSynchronous::Normal);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .after_connect(|connection, _meta| {
            Box::pin(async move {
                connection.execute("pragma temp_store=memory").await?;
                sqlx::query("pragma wal_autocheckpoint=0")
                    .execute(&mut *connection)
                    .await?;
                Ok(())
            })
        })
        .connect_with(options)
        .await?;

    sqlx::query(
        r#"
        create table if not exists request_logs (
            id text primary key,
            tenant_id text,
            platform_api_key_id text,
            provider_id text not null,
            provider_account_id text,
            egress_proxy_id text,
            method text not null,
            route text not null,
            upstream_url text not null,
            status_code integer,
            latency_ms integer,
            failure_kind text,
            failure_message text,
            request_headers text not null default '{}' check (json_valid(request_headers)),
            response_headers text not null default '{}' check (json_valid(response_headers)),
            request_body text,
            response_body text,
            created_at text not null
        ) strict
        "#,
    )
    .execute(&pool)
    .await?;
    sqlx::query(
        "create index if not exists idx_request_logs_created_at_id on request_logs (created_at desc, id desc)",
    )
    .execute(&pool)
    .await?;

    let mut transaction = pool.begin().await?;
    let mut archived_ids = Vec::with_capacity(rows.len());
    for row in rows {
        sqlx::query(
            r#"
            insert or ignore into request_logs (
                id,
                tenant_id,
                platform_api_key_id,
                provider_id,
                provider_account_id,
                egress_proxy_id,
                method,
                route,
                upstream_url,
                status_code,
                latency_ms,
                failure_kind,
                failure_message,
                request_headers,
                response_headers,
                request_body,
                response_body,
                created_at
            ) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&row.id)
        .bind(row.tenant_id.as_deref())
        .bind(row.platform_api_key_id.as_deref())
        .bind(&row.provider_id)
        .bind(row.provider_account_id.as_deref())
        .bind(row.egress_proxy_id.as_deref())
        .bind(&row.method)
        .bind(&row.route)
        .bind(&row.upstream_url)
        .bind(row.status_code)
        .bind(row.latency_ms)
        .bind(row.failure_kind.as_deref())
        .bind(row.failure_message.as_deref())
        .bind(&row.request_headers)
        .bind(&row.response_headers)
        .bind(row.request_body.as_deref())
        .bind(row.response_body.as_deref())
        .bind(&row.created_at)
        .execute(&mut *transaction)
        .await?;
        archived_ids.push(row.id.clone());
    }
    transaction.commit().await?;
    sqlx::query("pragma wal_checkpoint(passive)")
        .execute(&pool)
        .await?;
    Ok(archived_ids)
}

fn encrypt_credential(plaintext: &str, master_key: &[u8; 32]) -> String {
    let key = master_key;
    let cipher = Aes256Gcm::new(key.into());
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .expect("AES-GCM encryption should not fail");
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    BASE64.encode(&combined)
}

fn decrypt_credential(stored: &str, master_key: &[u8; 32]) -> Result<String, StorageError> {
    try_decrypt(stored, master_key).ok_or_else(|| StorageError::InvalidEncryptedCredential)
}

fn try_decrypt(stored: &str, key: &[u8; 32]) -> Option<String> {
    let combined = BASE64.decode(stored).ok()?;
    if combined.len() < 13 {
        return None;
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher.decrypt(nonce, ciphertext).ok()?;
    String::from_utf8(plaintext).ok()
}

fn map_admin_audit_log(row: sqlx::sqlite::SqliteRow) -> Result<AdminAuditLogRecord, StorageError> {
    let old_value: Option<String> = row.try_get("old_value")?;
    let new_value: Option<String> = row.try_get("new_value")?;
    Ok(AdminAuditLogRecord {
        id: row.try_get("id")?,
        admin_identity: row.try_get("admin_identity")?,
        action: row.try_get("action")?,
        resource_type: row.try_get("resource_type")?,
        resource_id: row.try_get("resource_id")?,
        old_value: old_value.as_deref().map(serde_json::from_str).transpose()?,
        new_value: new_value.as_deref().map(serde_json::from_str).transpose()?,
        created_at: row.try_get("created_at")?,
    })
}

fn map_alert_rule(row: sqlx::sqlite::SqliteRow) -> Result<AlertRuleRecord, StorageError> {
    Ok(AlertRuleRecord {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        kind: AlertRuleKind::from_str(row.try_get::<String, _>("kind")?.as_str())?,
        threshold_value: row.try_get("threshold_value")?,
        webhook_url: row.try_get("webhook_url")?,
        enabled: int_to_bool(row.try_get::<i64, _>("enabled")?),
        last_triggered_at: row.try_get("last_triggered_at")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_alert_event(row: sqlx::sqlite::SqliteRow) -> Result<AlertEventRecord, StorageError> {
    let metadata_str: String = row.try_get("metadata")?;
    Ok(AlertEventRecord {
        id: row.try_get("id")?,
        alert_rule_id: row.try_get("alert_rule_id")?,
        kind: row.try_get("kind")?,
        message: row.try_get("message")?,
        metadata: serde_json::from_str(&metadata_str)?,
        created_at: row.try_get("created_at")?,
    })
}

fn map_provider_account(
    row: sqlx::sqlite::SqliteRow,
    master_key: &[u8; 32],
) -> Result<ProviderAccount, StorageError> {
    let raw_api_key: String = row.try_get("encrypted_api_key")?;
    let api_key = decrypt_credential(&raw_api_key, master_key)?;
    let (reader_base_url, search_base_url) = parse_provider_account_config(&row)?;
    Ok(ProviderAccount {
        id: row.try_get("id")?,
        provider: ProviderId::from_str(row.try_get::<String, _>("provider_id")?.as_str())?,
        name: row.try_get("name")?,
        api_key,
        base_url: row.try_get("base_url")?,
        reader_base_url,
        search_base_url,
        enabled: int_to_bool(row.try_get::<i64, _>("enabled")?),
        status: ProviderAccountStatus::from_str(row.try_get::<String, _>("status")?.as_str())?,
        last_error: row.try_get("last_error")?,
        cooldown_until: row.try_get("cooldown_until")?,
        last_used_at: row.try_get("last_used_at")?,
        consecutive_failures: row.try_get("consecutive_failures")?,
        last_status_code: row.try_get("last_status_code")?,
        weight: row.try_get("weight")?,
        last_failure_at: row.try_get("last_failure_at")?,
    })
}

fn map_provider_account_summary(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ProviderAccountSummary, StorageError> {
    let (reader_base_url, search_base_url) = parse_provider_account_config(&row)?;
    Ok(ProviderAccountSummary {
        id: row.try_get("id")?,
        provider: ProviderId::from_str(row.try_get::<String, _>("provider_id")?.as_str())?,
        name: row.try_get("name")?,
        base_url: row.try_get("base_url")?,
        reader_base_url,
        search_base_url,
        enabled: int_to_bool(row.try_get::<i64, _>("enabled")?),
        status: ProviderAccountStatus::from_str(row.try_get::<String, _>("status")?.as_str())?,
        last_error: row.try_get("last_error")?,
        cooldown_until: row.try_get("cooldown_until")?,
        last_used_at: row.try_get("last_used_at")?,
        consecutive_failures: row.try_get("consecutive_failures")?,
        last_status_code: row.try_get("last_status_code")?,
        weight: row.try_get("weight")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn provider_account_config_json(
    reader_base_url: Option<&str>,
    search_base_url: Option<&str>,
) -> Result<String, StorageError> {
    serde_json::to_string(&serde_json::json!({
        "reader_base_url": reader_base_url,
        "search_base_url": search_base_url,
    }))
    .map_err(StorageError::Json)
}

fn parse_provider_account_config(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<(Option<String>, Option<String>), StorageError> {
    let config: serde_json::Value =
        serde_json::from_str(row.try_get::<String, _>("config")?.as_str())?;
    Ok((
        config
            .get("reader_base_url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        config
            .get("search_base_url")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    ))
}

fn map_egress_proxy(row: sqlx::sqlite::SqliteRow) -> Result<EgressProxy, StorageError> {
    Ok(EgressProxy {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        kind: EgressProxyKind::from_str(row.try_get::<String, _>("kind")?.as_str())?,
        proxy_url: row.try_get("proxy_url")?,
        region: row.try_get("region")?,
        enabled: int_to_bool(row.try_get::<i64, _>("enabled")?),
        status: EgressProxyStatus::from_str(row.try_get::<String, _>("status")?.as_str())?,
        last_error: row.try_get("last_error")?,
        cooldown_until: row.try_get("cooldown_until")?,
        last_used_at: row.try_get("last_used_at")?,
        consecutive_failures: row.try_get("consecutive_failures")?,
    })
}

fn map_egress_proxy_summary(
    row: sqlx::sqlite::SqliteRow,
) -> Result<EgressProxySummary, StorageError> {
    Ok(EgressProxySummary {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        kind: EgressProxyKind::from_str(row.try_get::<String, _>("kind")?.as_str())?,
        proxy_url: row.try_get("proxy_url")?,
        region: row.try_get("region")?,
        enabled: int_to_bool(row.try_get::<i64, _>("enabled")?),
        status: EgressProxyStatus::from_str(row.try_get::<String, _>("status")?.as_str())?,
        last_error: row.try_get("last_error")?,
        cooldown_until: row.try_get("cooldown_until")?,
        last_used_at: row.try_get("last_used_at")?,
        consecutive_failures: row.try_get("consecutive_failures")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_request_log_record(row: sqlx::sqlite::SqliteRow) -> Result<RequestLogRecord, StorageError> {
    Ok(RequestLogRecord {
        id: row.try_get("id")?,
        tenant_id: row.try_get("tenant_id")?,
        platform_api_key_id: row.try_get("platform_api_key_id")?,
        provider: ProviderId::from_str(row.try_get::<String, _>("provider_id")?.as_str())?,
        provider_account_id: row.try_get("provider_account_id")?,
        egress_proxy_id: row.try_get("egress_proxy_id")?,
        method: row.try_get("method")?,
        route: row.try_get("route")?,
        upstream_url: row.try_get("upstream_url")?,
        status_code: row.try_get("status_code")?,
        latency_ms: row.try_get("latency_ms")?,
        failure_kind: row.try_get("failure_kind")?,
        failure_message: row.try_get("failure_message")?,
        created_at: row.try_get("created_at")?,
    })
}

fn map_provider_async_job_record(
    row: sqlx::sqlite::SqliteRow,
) -> Result<ProviderAsyncJobRecord, StorageError> {
    Ok(ProviderAsyncJobRecord {
        id: row.try_get("id")?,
        tenant_id: row.try_get("tenant_id")?,
        request_log_id: row.try_get("request_log_id")?,
        provider: ProviderId::from_str(row.try_get::<String, _>("provider_id")?.as_str())?,
        provider_account_id: row.try_get("provider_account_id")?,
        egress_proxy_id: row.try_get("egress_proxy_id")?,
        route: row.try_get("route")?,
        upstream_job_id: row.try_get("upstream_job_id")?,
        state: ProviderAsyncJobState::from_str(row.try_get::<String, _>("state")?.as_str())?,
        last_status_code: row.try_get("last_status_code")?,
        last_error: row.try_get("last_error")?,
        poll_attempts: row.try_get("poll_attempts")?,
        next_poll_at: row.try_get("next_poll_at")?,
        settled_at: row.try_get("settled_at")?,
        metadata: parse_json_column(&row, "metadata")?,
        webhook_secret: row.try_get("webhook_secret")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn bool_to_int(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

fn int_to_bool(value: i64) -> bool {
    value != 0
}

fn normalize_limit(value: i64) -> i64 {
    value.clamp(1, 500)
}

fn parse_json_column(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
) -> Result<serde_json::Value, StorageError> {
    let raw = row.try_get::<String, _>(column)?;
    Ok(serde_json::from_str(&raw)?)
}

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("invalid database url: {0}")]
    InvalidDatabaseUrl(String),
    #[error("stored encrypted credential is invalid")]
    InvalidEncryptedCredential,
    #[error("tenant `{0}` was not found")]
    TenantNotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Core(#[from] ProviderError),
}
