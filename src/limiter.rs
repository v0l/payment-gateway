use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct Limiter {
    max: u32,
    window: Duration,
    hits: Mutex<HashMap<String, (u32, Instant)>>,
}

impl Limiter {
    pub fn new(max: u32, window: Duration) -> Self {
        Self {
            max,
            window,
            hits: Default::default(),
        }
    }

    pub fn blocked(&self, key: &str) -> bool {
        let mut hits = self.hits.lock().unwrap();
        match hits.get(key) {
            Some((n, at)) if at.elapsed() < self.window => *n >= self.max,
            Some(_) => {
                hits.remove(key);
                false
            }
            None => false,
        }
    }

    pub fn failure(&self, key: &str) {
        let mut hits = self.hits.lock().unwrap();
        if hits.len() > 10_000 {
            let window = self.window;
            hits.retain(|_, (_, at)| at.elapsed() < window);
        }
        let entry = hits.entry(key.to_string()).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= self.window {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
    }

    pub fn clear(&self, key: &str) {
        self.hits.lock().unwrap().remove(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_after_max_failures_and_clears() {
        let l = Limiter::new(3, Duration::from_secs(60));
        for _ in 0..2 {
            l.failure("a");
        }
        assert!(!l.blocked("a"));
        l.failure("a");
        assert!(l.blocked("a"));
        assert!(!l.blocked("b"));
        l.clear("a");
        assert!(!l.blocked("a"));
    }

    #[test]
    fn window_expiry_resets() {
        let l = Limiter::new(1, Duration::from_millis(20));
        l.failure("a");
        assert!(l.blocked("a"));
        std::thread::sleep(Duration::from_millis(40));
        assert!(!l.blocked("a"));
    }
}
