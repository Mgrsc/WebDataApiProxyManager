use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use thiserror::Error;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use tracing::info;
use wdapm_core::{
    ProviderAccount, ProviderId, SchedulerSelection, parse_sqlite_timestamp, summarize_proxy_url,
};
use wdapm_storage::{StorageError, StorageService};

#[derive(Clone, Debug)]
pub struct SchedulerConfig {
    pub health_penalty_per_failure: i64,
    pub max_retries: u32,
    pub failure_decay_window_secs: i64,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            health_penalty_per_failure: 20,
            max_retries: 2,
            failure_decay_window_secs: 600,
        }
    }
}

#[derive(Clone)]
pub struct SchedulerService {
    state: Arc<Mutex<SchedulerState>>,
    config: SchedulerConfig,
}

#[derive(Clone, Copy, Default)]
pub struct SchedulerExclusions<'a> {
    pub account_ids: &'a [&'a str],
    pub routes: &'a [SchedulerRouteExclusion<'a>],
}

#[derive(Clone, Copy)]
pub struct SchedulerRouteExclusion<'a> {
    pub account_id: &'a str,
    pub proxy_id: Option<&'a str>,
}

#[derive(Default)]
struct SchedulerState {
    account_cursors: BTreeMap<ProviderId, usize>,
    proxy_cursors: BTreeMap<String, usize>,
}

impl Default for SchedulerService {
    fn default() -> Self {
        Self::new()
    }
}

impl SchedulerService {
    pub fn new() -> Self {
        Self::with_config(SchedulerConfig::default())
    }

    pub fn with_config(config: SchedulerConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(SchedulerState::default())),
            config,
        }
    }

    pub fn config(&self) -> &SchedulerConfig {
        &self.config
    }

    pub async fn select_route(
        &self,
        storage: &StorageService,
        provider: ProviderId,
    ) -> Result<SchedulerSelection, SchedulerError> {
        self.select_route_excluding(storage, provider, SchedulerExclusions::default())
            .await
    }

    pub async fn select_route_excluding(
        &self,
        storage: &StorageService,
        provider: ProviderId,
        exclusions: SchedulerExclusions<'_>,
    ) -> Result<SchedulerSelection, SchedulerError> {
        let mut accounts = storage.list_routable_provider_accounts(provider).await?;
        if !exclusions.account_ids.is_empty() {
            accounts.retain(|a| !exclusions.account_ids.contains(&a.id.as_str()));
        }
        if accounts.is_empty() {
            return Err(SchedulerError::NoAvailableAccount(provider));
        }

        let penalty = self.config.health_penalty_per_failure;
        let decay_window = self.config.failure_decay_window_secs;
        let now = OffsetDateTime::now_utc();
        let mut scored: Vec<(i64, usize)> = accounts
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let effective = effective_failures(a, now, decay_window);
                let score = a.weight - effective * penalty;
                (score, i)
            })
            .collect();
        scored.sort_by_key(|item| std::cmp::Reverse(item.0));

        let mut candidate_indices: Vec<usize> = scored.iter().map(|(_, i)| *i).collect();
        if let Some((top_score, _)) = scored.first() {
            let tied_count = scored
                .iter()
                .take_while(|(score, _)| score == top_score)
                .count();
            let mut state = self
                .state
                .lock()
                .map_err(|_| SchedulerError::StatePoisoned)?;
            let cursor = state.account_cursors.entry(provider).or_insert(0);
            if tied_count > 1 {
                candidate_indices[..tied_count].rotate_left(*cursor % tied_count);
                *cursor = (*cursor + 1) % tied_count;
            }
        }

        for account_index in candidate_indices {
            let account = accounts[account_index].clone();
            let mut proxies = storage
                .list_routable_bound_egress_proxies(&account.id)
                .await?;
            if !exclusions.routes.is_empty() {
                proxies.retain(|proxy| {
                    !exclusions.routes.iter().any(|excluded| {
                        excluded.account_id == account.id.as_str()
                            && excluded.proxy_id == Some(proxy.id.as_str())
                    })
                });
            }
            let direct_excluded = exclusions.routes.iter().any(|excluded| {
                excluded.account_id == account.id.as_str() && excluded.proxy_id.is_none()
            });
            let egress_proxy = if proxies.is_empty() {
                if direct_excluded {
                    continue;
                }
                None
            } else {
                let proxy_index = {
                    let mut state = self
                        .state
                        .lock()
                        .map_err(|_| SchedulerError::StatePoisoned)?;
                    let proxy_cursor = state.proxy_cursors.entry(account.id.clone()).or_insert(0);
                    let proxy_index = *proxy_cursor % proxies.len();
                    *proxy_cursor = (*proxy_cursor + 1) % proxies.len();
                    proxy_index
                };
                Some(proxies[proxy_index].clone())
            };
            let selection_reason = match &egress_proxy {
                Some(proxy) => format!(
                    "account:{} egress:proxy id:{} target:{}",
                    account.id,
                    proxy.id,
                    summarize_proxy_url(&proxy.proxy_url)
                ),
                None => format!(
                    "account:{} egress:direct reason:no_proxy_binding",
                    account.id
                ),
            };

            info!(
                provider = %provider,
                provider_account_id = %account.id,
                egress_proxy_id = egress_proxy.as_ref().map(|value| value.id.as_str()),
                selection_reason = %selection_reason,
                "egress route selected"
            );

            return Ok(SchedulerSelection {
                account,
                egress_proxy,
                selection_reason,
            });
        }

        Err(SchedulerError::NoAvailableAccount(provider))
    }
}

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("provider `{0}` has no available account")]
    NoAvailableAccount(ProviderId),
    #[error("scheduler state is poisoned")]
    StatePoisoned,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

fn effective_failures(
    account: &ProviderAccount,
    now: OffsetDateTime,
    decay_window_secs: i64,
) -> i64 {
    if account.consecutive_failures <= 0 {
        return 0;
    }
    let elapsed_secs = account
        .last_failure_at
        .as_deref()
        .and_then(|ts| {
            OffsetDateTime::parse(ts, &Rfc3339)
                .ok()
                .or_else(|| parse_sqlite_timestamp(ts))
        })
        .map(|t| (now - t).whole_seconds().max(0))
        .unwrap_or(0);
    if elapsed_secs >= decay_window_secs {
        return 0;
    }
    let decay_factor = 1.0 - (elapsed_secs as f64 / decay_window_secs as f64);
    (account.consecutive_failures as f64 * decay_factor).ceil() as i64
}
