use crate::settings::{Provider, data_dir, write_private};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const MIN_SAMPLES: u32 = 5;
pub const MIN_THRESHOLD_MS: f64 = 1500.0;
const DECAY: f64 = 0.95;
const OUTLIER_SDS: f64 = 3.0;
const PRIOR_WEIGHT: f64 = 25.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    pub samples: u32,
    w: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
    var: f64,
}

impl Baseline {
    pub fn expected_ms(&self, secs: f64) -> Option<f64> {
        if self.w <= 0.0 {
            return None;
        }
        let (mx, my) = (self.sx / self.w, self.sy / self.w);
        let sxx_c = (self.sxx - self.w * mx * mx).max(0.0);
        let sxy_c = self.sxy - self.w * mx * my;
        let prior = if mx > 0.0 { my / mx } else { 0.0 };
        let slope = ((sxy_c + PRIOR_WEIGHT * prior) / (sxx_c + PRIOR_WEIGHT)).max(0.0);
        Some((my + slope * (secs - mx)).max(0.0))
    }

    pub fn sd_ms(&self) -> f64 {
        self.var.sqrt()
    }

    pub fn ready(&self) -> bool {
        self.samples >= MIN_SAMPLES
    }

    pub fn threshold_ms(&self, secs: f64) -> Option<f64> {
        self.ready().then(|| self.expected_ms(secs)).flatten().map(|e| (e + self.sd_ms()).max(MIN_THRESHOLD_MS))
    }

    pub fn record(&mut self, secs: f64, ms: f64) {
        let mut y = ms;
        if let Some(expected) = self.expected_ms(secs) {
            let mut err = ms - expected;
            if self.ready() {
                let limit = OUTLIER_SDS * self.sd_ms().max(MIN_THRESHOLD_MS / OUTLIER_SDS);
                err = err.min(limit);
                y = expected + err;
            }
            self.var = if self.samples <= 1 { err * err } else { DECAY * self.var + (1.0 - DECAY) * err * err };
        }
        self.w = DECAY * self.w + 1.0;
        self.sx = DECAY * self.sx + secs;
        self.sy = DECAY * self.sy + y;
        self.sxx = DECAY * self.sxx + secs * secs;
        self.sxy = DECAY * self.sxy + secs * y;
        self.samples += 1;
    }
}

pub fn key(provider: Provider, model: &str) -> String {
    format!("{provider:?}:{model}")
}

pub enum Recorded {
    Normal,
    Slow,
    Recovered,
}

pub struct Latency {
    path: Option<PathBuf>,
    baselines: Mutex<HashMap<String, Baseline>>,
    degraded: AtomicBool,
}

impl Latency {
    pub fn load() -> Self {
        let path = data_dir().join("latency.json");
        let baselines = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { path: Some(path), baselines: Mutex::new(baselines), degraded: AtomicBool::new(false) }
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self { path: None, baselines: Mutex::default(), degraded: AtomicBool::new(false) }
    }

    pub fn baseline(&self, key: &str) -> Baseline {
        self.baselines.lock().unwrap().get(key).copied().unwrap_or_default()
    }

    pub fn all(&self) -> Vec<(String, Baseline)> {
        let mut all: Vec<_> = self.baselines.lock().unwrap().iter().map(|(k, b)| (k.clone(), *b)).collect();
        all.sort_by(|a, b| a.0.cmp(&b.0));
        all
    }

    pub fn threshold(&self, key: &str, secs: f64) -> Option<Duration> {
        self.baseline(key).threshold_ms(secs).map(|ms| Duration::from_millis(ms as u64))
    }

    pub fn record(&self, key: &str, secs: f64, wait_ms: u64) -> Recorded {
        let mut map = self.baselines.lock().unwrap();
        let b = map.entry(key.to_string()).or_default();
        let slow = b.threshold_ms(secs).is_some_and(|t| wait_ms as f64 > t);
        b.record(secs, wait_ms as f64);
        if let Some(path) = &self.path
            && let Ok(bytes) = serde_json::to_vec_pretty(&*map) {
                let _ = write_private(path, &bytes);
            }
        match slow {
            true => Recorded::Slow,
            false if self.degraded.swap(false, Ordering::SeqCst) => Recorded::Recovered,
            false => Recorded::Normal,
        }
    }

    pub fn mark_degraded(&self) -> bool {
        !self.degraded.swap(true, Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trained(points: &[(f64, f64)]) -> Baseline {
        let mut b = Baseline::default();
        for &(x, y) in points {
            b.record(x, y);
        }
        b
    }

    #[test]
    fn insufficient_history_gives_no_threshold() {
        let b = trained(&[(5.0, 800.0), (6.0, 900.0), (4.0, 700.0), (5.0, 820.0)]);
        assert_eq!(b.threshold_ms(5.0), None);
        assert!(trained(&[(5.0, 800.0), (6.0, 900.0), (4.0, 700.0), (5.0, 820.0), (5.0, 790.0)]).threshold_ms(5.0).is_some());
    }

    #[test]
    fn threshold_scales_with_audio_duration() {
        let pts: Vec<(f64, f64)> = (0..30).map(|i| {
            let x = [3.0, 10.0, 30.0, 60.0][i % 4];
            (x, 400.0 + 40.0 * x + if i % 2 == 0 { 50.0 } else { -50.0 })
        }).collect();
        let b = trained(&pts);
        let short = b.threshold_ms(3.0).unwrap();
        let long = b.threshold_ms(60.0).unwrap();
        assert!((b.expected_ms(60.0).unwrap() - 2800.0).abs() < 150.0, "{:?}", b.expected_ms(60.0));
        assert!(short >= MIN_THRESHOLD_MS && long > short + 1500.0, "{short} {long}");
    }

    #[test]
    fn similar_lengths_extrapolate_proportionally_not_flat() {
        let b = trained(&(0..10).map(|i| (5.0, 1000.0 + (i % 3) as f64 * 20.0)).collect::<Vec<_>>());
        assert!(b.expected_ms(60.0).unwrap() > 6000.0);
    }

    #[test]
    fn a_single_extreme_slowdown_barely_moves_the_baseline() {
        let pts: Vec<(f64, f64)> = (0..20).map(|i| (5.0, 1000.0 + (i % 5) as f64 * 40.0)).collect();
        let mut b = trained(&pts);
        let before = b.expected_ms(5.0).unwrap();
        b.record(5.0, 60_000.0);
        assert!(b.expected_ms(5.0).unwrap() - before < 300.0);
    }

    #[test]
    fn a_lasting_change_in_speed_is_learned() {
        let mut b = trained(&(0..20).map(|i| (5.0, 1000.0 + (i % 5) as f64 * 40.0)).collect::<Vec<_>>());
        for i in 0..60 {
            b.record(5.0, 2500.0 + (i % 5) as f64 * 40.0);
        }
        assert!((b.expected_ms(5.0).unwrap() - 2580.0).abs() < 200.0, "{:?}", b.expected_ms(5.0));
    }

    #[test]
    fn baselines_survive_a_restart() {
        let b = trained(&[(5.0, 800.0), (6.0, 900.0)]);
        let json = serde_json::to_string(&HashMap::from([("OpenAI:m".to_string(), b)])).unwrap();
        let back: HashMap<String, Baseline> = serde_json::from_str(&json).unwrap();
        assert_eq!(back["OpenAI:m"], b);
    }

    #[test]
    fn recovery_is_reported_once_after_degradation() {
        let l = Latency::in_memory();
        for i in 0..10 {
            l.record("k", 5.0, 1000 + i * 10);
        }
        assert!(l.mark_degraded());
        assert!(!l.mark_degraded());
        assert!(matches!(l.record("k", 5.0, 30_000), Recorded::Slow));
        assert!(matches!(l.record("k", 5.0, 1000), Recorded::Recovered));
        assert!(matches!(l.record("k", 5.0, 1000), Recorded::Normal));
    }
}
