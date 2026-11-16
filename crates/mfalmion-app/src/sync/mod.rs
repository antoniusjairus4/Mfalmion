//! Selected-account public-data sync. No engine, credential or external archive access.
pub mod api;
pub mod client;
pub mod runner;
#[cfg(test)]
mod tests;
pub mod worker;
pub use crate::storage::sync::Progress;
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs().min(i64::MAX as u64) as i64)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
    pub retry_at: i64,
    pub offline: bool,
    pub permanent: bool,
}
impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self {
            message,
            retry_at: 0,
            offline: false,
            permanent: false,
        }
    }
}
pub type Result<T> = std::result::Result<T, Failure>;
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Preferences {
    pub enabled: bool,
    pub interval_minutes: u32,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_minutes: 30,
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if !(5..=10080).contains(&self.interval_minutes) {
            Err("Sync interval must be 5–10080 minutes".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Debug)]
pub struct Schedule {
    pub next: i64,
    profile: i64,
    failures: u32,
    permanent: bool,
}
impl Schedule {
    pub fn new(profile: i64, p: &Preferences, now: i64) -> Self {
        Self {
            profile,
            next: now + i64::from(p.interval_minutes) * 60,
            failures: 0,
            permanent: false,
        }
    }
    pub fn next_wake(&self, now: i64) -> Option<u64> {
        (!self.permanent).then(|| self.next.saturating_sub(now).clamp(1, 3600) as u64)
    }
    pub fn due(&self, profile: i64, p: &Preferences, now: i64, busy: bool) -> bool {
        p.enabled && !busy && !self.permanent && self.profile == profile && now >= self.next
    }
    pub fn finish(&mut self, p: &Preferences, now: i64, failure: Option<&Failure>) {
        if let Some(f) = failure {
            self.failures = self.failures.saturating_add(1);
            self.permanent = f.permanent;
            let delay = (60_i64.saturating_mul(1_i64 << self.failures.min(5))).min(1800);
            self.next = (now + delay).max(f.retry_at);
        } else {
            self.failures = 0;
            self.permanent = false;
            self.next = now + i64::from(p.interval_minutes) * 60;
        }
    }
}
