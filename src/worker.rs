use crate::db;
use crate::providers::EventSink;
use crate::service::{ProviderSink, Service};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::task::JoinHandle;

type Handles = Arc<Mutex<Vec<JoinHandle<()>>>>;

pub struct Workers {
    handles: Handles,
}

impl Workers {
    pub fn abort(&self) {
        for h in self.handles.lock().unwrap().iter() {
            h.abort();
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.abort();
    }
}

pub fn spawn(service: Arc<Service>, tick: Duration) -> Workers {
    let handles: Handles = Default::default();

    let s = service.clone();
    handles.lock().unwrap().push(tokio::spawn(async move {
        loop {
            if let Err(e) = s.expire_due().await {
                log::error!("expiry run failed: {e:?}");
            }
            tokio::time::sleep(tick).await;
        }
    }));

    let s = service.clone();
    handles.lock().unwrap().push(tokio::spawn(async move {
        loop {
            if let Err(e) = s.deliver_due().await {
                log::error!("webhook delivery failed: {e:?}");
            }
            tokio::time::sleep(tick).await;
        }
    }));

    let s = service.clone();
    let watcher_handles = handles.clone();
    handles.lock().unwrap().push(tokio::spawn(async move {
        let watching: Arc<Mutex<HashSet<u64>>> = Default::default();
        loop {
            match db::list_all_enabled_provider_configs(&s.pool).await {
                Ok(configs) => {
                    for cfg in configs {
                        if matches!(cfg.method.as_str(), "lightning" | "onchain") {
                            start_watcher(s.clone(), cfg.id, watching.clone(), &watcher_handles);
                            if let Err(e) = s.reconcile_provider(cfg.id).await {
                                log::warn!("reconcile provider {} failed: {e:?}", cfg.id);
                            }
                        }
                        if matches!(cfg.method.as_str(), "stripe" | "revolut") {
                            setup_hooks(&s, &cfg).await;
                        }
                    }
                }
                Err(e) => log::error!("listing providers failed: {e:#}"),
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    }));

    Workers { handles }
}

async fn setup_hooks(s: &Arc<Service>, cfg: &db::ProviderConfigRow) {
    if cfg.method != "revolut" {
        return;
    }
    let Ok(provider) = s.registry.get(cfg.id).await else {
        return;
    };
    match provider.setup(&s.hook_url(cfg.tenant_id, cfg.id)).await {
        Ok(Some(patch)) => {
            let merged = s.registry.open_config(cfg).map(|mut c| {
                if let (Some(obj), Some(p)) = (c.as_object_mut(), patch.as_object()) {
                    obj.extend(p.clone());
                }
                c
            });
            match merged {
                Ok(c) => {
                    if let Err(e) = s.registry.persist_config(cfg.id, &c).await {
                        log::error!("storing webhook secret for {} failed: {e:#}", cfg.id);
                    }
                    s.registry.invalidate(cfg.id);
                }
                Err(e) => log::error!("provider {} config unreadable: {e:#}", cfg.id),
            }
        }
        Ok(None) => {}
        Err(e) => log::warn!("provider {} hook setup failed: {e:#}", cfg.id),
    }
}

fn start_watcher(
    s: Arc<Service>,
    provider_id: u64,
    watching: Arc<Mutex<HashSet<u64>>>,
    handles: &Handles,
) {
    if !watching.lock().unwrap().insert(provider_id) {
        return;
    }
    let handle = tokio::spawn(async move {
        let mut backoff = Duration::from_secs(2);
        loop {
            let provider = match s.registry.get(provider_id).await {
                Ok(p) => p,
                Err(e) => {
                    log::error!("provider {provider_id} unavailable: {e:#}");
                    tokio::time::sleep(backoff).await;
                    backoff = (backoff * 2).min(Duration::from_secs(120));
                    continue;
                }
            };
            if !provider.watches() {
                break;
            }
            let cursor = db::get_cursor(&s.pool, provider_id).await.ok().flatten();
            let sink: Arc<dyn EventSink> = Arc::new(ProviderSink {
                service: s.clone(),
                provider_id,
            });
            log::info!("watching provider {provider_id}");
            let started = std::time::Instant::now();
            let result = provider.watch(sink, cursor).await;
            if started.elapsed() > Duration::from_secs(60) {
                backoff = Duration::from_secs(2);
            }
            match result {
                Ok(()) => log::warn!("watcher {provider_id} ended"),
                Err(e) => log::error!("watcher {provider_id} failed: {e:#}"),
            }
            s.registry.invalidate(provider_id);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(120));
        }
        watching.lock().unwrap().remove(&provider_id);
    });
    handles.lock().unwrap().push(handle);
}
