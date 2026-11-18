use std::collections::BTreeMap;
use std::fmt::Write;
use std::sync::Mutex;

const BUCKETS: [f64; 11] = [
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

type Key = (String, String);

#[derive(Default)]
struct Hist {
    counts: [u64; BUCKETS.len()],
    count: u64,
    sum: f64,
}

#[derive(Default)]
struct Inner {
    counters: BTreeMap<Key, u64>,
    gauges: BTreeMap<Key, f64>,
    hists: BTreeMap<Key, Hist>,
}

#[derive(Default)]
pub struct Metrics {
    inner: Mutex<Inner>,
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn label_string(labels: &[(&str, &str)]) -> String {
    labels
        .iter()
        .map(|(k, v)| format!("{k}=\"{}\"", escape(v)))
        .collect::<Vec<_>>()
        .join(",")
}

fn key(name: &str, labels: &[(&str, &str)]) -> Key {
    (name.to_string(), label_string(labels))
}

fn braces(labels: &str) -> String {
    if labels.is_empty() {
        String::new()
    } else {
        format!("{{{labels}}}")
    }
}

impl Metrics {
    pub fn inc(&self, name: &str, labels: &[(&str, &str)]) {
        *self
            .inner
            .lock()
            .unwrap()
            .counters
            .entry(key(name, labels))
            .or_default() += 1;
    }

    pub fn set(&self, name: &str, labels: &[(&str, &str)], value: f64) {
        self.inner
            .lock()
            .unwrap()
            .gauges
            .insert(key(name, labels), value);
    }

    pub fn observe(&self, name: &str, labels: &[(&str, &str)], seconds: f64) {
        let mut inner = self.inner.lock().unwrap();
        let h = inner.hists.entry(key(name, labels)).or_default();
        h.count += 1;
        h.sum += seconds;
        for (i, bound) in BUCKETS.iter().enumerate() {
            if seconds <= *bound {
                h.counts[i] += 1;
            }
        }
    }

    pub fn counter(&self, name: &str, labels: &[(&str, &str)]) -> u64 {
        self.inner
            .lock()
            .unwrap()
            .counters
            .get(&key(name, labels))
            .copied()
            .unwrap_or(0)
    }

    pub fn gauges(&self, name: &str) -> Vec<(String, f64)> {
        self.inner
            .lock()
            .unwrap()
            .gauges
            .iter()
            .filter(|((n, _), _)| n == name)
            .map(|((_, l), v)| (l.clone(), *v))
            .collect()
    }

    pub fn render(&self) -> String {
        let inner = self.inner.lock().unwrap();
        let mut out = String::new();
        let mut last = "";
        for ((name, labels), v) in &inner.counters {
            if name != last {
                let _ = writeln!(out, "# TYPE {name} counter");
                last = name;
            }
            let _ = writeln!(out, "{name}{} {v}", braces(labels));
        }
        last = "";
        for ((name, labels), v) in &inner.gauges {
            if name != last {
                let _ = writeln!(out, "# TYPE {name} gauge");
                last = name;
            }
            let _ = writeln!(out, "{name}{} {v}", braces(labels));
        }
        last = "";
        for ((name, labels), h) in &inner.hists {
            if name != last {
                let _ = writeln!(out, "# TYPE {name} histogram");
                last = name;
            }
            let sep = if labels.is_empty() { "" } else { "," };
            for (i, bound) in BUCKETS.iter().enumerate() {
                let _ = writeln!(
                    out,
                    "{name}_bucket{{{labels}{sep}le=\"{bound}\"}} {}",
                    h.counts[i]
                );
            }
            let _ = writeln!(out, "{name}_bucket{{{labels}{sep}le=\"+Inf\"}} {}", h.count);
            let _ = writeln!(out, "{name}_sum{} {}", braces(labels), h.sum);
            let _ = writeln!(out, "{name}_count{} {}", braces(labels), h.count);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_counters_gauges_and_histograms() {
        let m = Metrics::default();
        m.inc("a_total", &[("route", "/x")]);
        m.inc("a_total", &[("route", "/x")]);
        m.set("g", &[], 3.0);
        m.observe("d_seconds", &[("route", "/x")], 0.03);
        let out = m.render();
        assert!(out.contains("a_total{route=\"/x\"} 2"));
        assert!(out.contains("g 3"));
        assert!(out.contains("d_seconds_bucket{route=\"/x\",le=\"0.025\"} 0"));
        assert!(out.contains("d_seconds_bucket{route=\"/x\",le=\"0.05\"} 1"));
        assert!(out.contains("d_seconds_count{route=\"/x\"} 1"));
    }

    #[test]
    fn label_values_are_escaped() {
        let m = Metrics::default();
        m.inc("a", &[("v", "q\"\n")]);
        assert!(m.render().contains("a{v=\"q\\\"\\n\"} 1"));
    }
}
