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
    stop: tokio::sync::watch::Sender<bool>,
    loops: usize,
}

impl Workers {
    pub fn abort(&self) {
        for h in self.handles.lock().unwrap().iter() {
            h.abort();
        }
    }

    pub async fn shutdown(&self, grace: Duration) {
        let _ = self.stop.send(true);
        let loops: Vec<JoinHandle<()>> = {
            let mut all = self.handles.lock().unwrap();
            all.drain(..self.loops).collect()
        };
        let _ = tokio::time::timeout(grace, futures::future::join_all(loops)).await;
        self.abort();
    }
}

async fn pause(stop: &mut tokio::sync::watch::Receiver<bool>, d: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => false,
        _ = stop.changed() => true,
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.abort();
    }
}

pub fn spawn(service: Arc<Service>, tick: Duration) -> Workers {
    let handles: Handles = Default::default();
    let (stop, stop_rx) = tokio::sync::watch::channel(false);

    let s = service.clone();
    let mut rx = stop_rx.clone();
    handles.lock().unwrap().push(tokio::spawn(async move {
        loop {
            if let Err(e) = s.expire_due().await {
                log::error!("expiry run failed: {e:?}");
            }
            if pause(&mut rx, tick).await {
                break;
            }
        }
    }));

    let s = service.clone();
    let mut rx = stop_rx.clone();
    handles.lock().unwrap().push(tokio::spawn(async move {
        loop {
            if let Err(e) = s.deliver_due().await {
                log::error!("webhook delivery failed: {e:?}");
            }
            if pause(&mut rx, tick).await {
                break;
            }
        }
    }));

    let s = service.clone();
    let mut rx = stop_rx.clone();
    handles.lock().unwrap().push(tokio::spawn(async move {
        loop {
            if let Err(e) = s.run_payouts().await {
                log::error!("payout run failed: {e:?}");
            }
            if let Err(e) = s.bill_due().await {
                log::error!("billing run failed: {e:?}");
            }
            if let Err(e) = s.run_platform_billing().await {
                log::error!("platform billing run failed: {e:?}");
            }
            if pause(&mut rx, tick).await {
                break;
            }
        }
    }));

    let s = service.clone();
    let watcher_handles = handles.clone();
    let mut rx = stop_rx;
    handles.lock().unwrap().push(tokio::spawn(async move {
        let watching: Arc<Mutex<HashSet<u64>>> = Default::default();
        loop {
            match db::list_all_enabled_provider_configs(&s.pool).await {
                Ok(configs) => {
                    for cfg in configs {
                        if matches!(cfg.method.as_str(), "lightning" | "onchain") {
                            start_watcher(s.clone(), cfg.id, watching.clone(), &watcher_handles);
                            match s.reconcile_provider(cfg.id).await {
                                Ok(_) => s.provider_recovered(cfg.id),
                                Err(e) => {
                                    log::warn!("reconcile provider {} failed: {e:?}", cfg.id);
                                    s.provider_failed(cfg.id, &format!("{e:?}")).await;
                                }
                            }
                        }
                        if matches!(cfg.method.as_str(), "stripe" | "revolut") {
                            setup_hooks(&s, &cfg).await;
                        }
                    }
                }
                Err(e) => log::error!("listing providers failed: {e:#}"),
            }
            match db::list_shared_providers(&s.pool, true).await {
                Ok(shared) => {
                    for sp in shared {
                        let key = crate::shared::shared_key(sp.id);
                        start_watcher(s.clone(), key, watching.clone(), &watcher_handles);
                        match s.reconcile_provider(key).await {
                            Ok(_) => s.provider_recovered(key),
                            Err(e) => {
                                log::warn!("reconcile shared provider {} failed: {e:?}", sp.id)
                            }
                        }
                    }
                }
                Err(e) => log::error!("listing shared providers failed: {e:#}"),
            }
            if pause(&mut rx, Duration::from_secs(30)).await {
                break;
            }
        }
    }));

    Workers {
        handles,
        stop,
        loops: 3,
    }
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
                    s.provider_failed(provider_id, &format!("{e:#}")).await;
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
            let label = provider_id.to_string();
            s.metrics
                .set("gateway_watcher_up", &[("provider", &label)], 1.0);
            let result = provider.watch(sink, cursor).await;
            s.metrics
                .set("gateway_watcher_up", &[("provider", &label)], 0.0);
            if started.elapsed() > Duration::from_secs(60) {
                backoff = Duration::from_secs(2);
                s.provider_recovered(provider_id);
            }
            match result {
                Ok(()) => log::warn!("watcher {provider_id} ended"),
                Err(e) => {
                    log::error!("watcher {provider_id} failed: {e:#}");
                    s.provider_failed(provider_id, &format!("{e:#}")).await;
                }
            }
            s.registry.invalidate(provider_id);
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(Duration::from_secs(120));
        }
        watching.lock().unwrap().remove(&provider_id);
    });
    handles.lock().unwrap().push(handle);
}

pub async fn lead(
    service: Arc<Service>,
    tick: Duration,
    lease: Arc<crate::lease::Lease>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let renew_every = lease.ttl() / 3;
    let mut workers: Option<Workers> = None;
    loop {
        match lease.acquire_or_renew().await {
            Ok(true) => {
                if workers.is_none() {
                    log::info!("acquired worker lease as {}", lease.holder());
                    workers = Some(spawn(service.clone(), tick));
                }
            }
            Ok(false) => {
                if let Some(w) = workers.take() {
                    log::warn!("lost worker lease");
                    w.abort();
                }
            }
            Err(e) => {
                log::error!("lease renewal failed: {e:#}");
                if let Some(w) = workers.take() {
                    w.abort();
                }
            }
        }
        service.metrics.set(
            "gateway_leader",
            &[],
            if lease.is_held() { 1.0 } else { 0.0 },
        );
        tokio::select! {
            _ = tokio::time::sleep(renew_every) => {}
            _ = shutdown.changed() => break,
        }
    }
    if let Some(w) = workers.take() {
        w.shutdown(Duration::from_secs(20)).await;
    }
    if let Err(e) = lease.release().await {
        log::error!("releasing lease failed: {e:#}");
    }
    service.metrics.set("gateway_leader", &[], 0.0);
}
