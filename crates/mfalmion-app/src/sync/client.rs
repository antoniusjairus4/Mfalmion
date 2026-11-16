//! Serial HTTPS only; redirects disabled, bounded decoded bodies and deadlines.
use super::*;
use crate::storage::sync::HttpCache;
use std::{
    collections::BTreeMap,
    io::Read,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
pub const USER_AGENT: &str =
    "Mfalmion/0.1.0 (+https://github.com/antoniusjairus4/Mfalmion; public-data desktop client)";
#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}
pub trait Transport: Send {
    fn get(
        &mut self,
        url: &str,
        cache: &HttpCache,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Response>;
}
pub struct Http {
    agent: ureq::Agent,
    last: Option<Instant>,
}
impl Http {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .https_only(true)
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(12)))
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_per_read(Some(Duration::from_secs(2)))
            .timeout_per_write(Some(Duration::from_secs(2)))
            .build();
        Self {
            agent: config.into(),
            last: None,
        }
    }
    pub(crate) fn request(
        &self,
        url: &str,
        cache: &HttpCache,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Response> {
        check(cancel)?;
        let mut request = self
            .agent
            .get(url)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json");
        if let Some(etag) = &cache.etag {
            request = request.header("If-None-Match", etag);
        }
        if let Some(modified) = &cache.modified {
            request = request.header("If-Modified-Since", modified);
        }
        let mut response = request.call().map_err(|e| Failure {
            message: format!("Chess.com network/timeout error: {e}"),
            retry_at: 0,
            offline: true,
            permanent: false,
        })?;
        check(cancel)?;
        let status = response.status().as_u16();
        let mut headers = BTreeMap::new();
        for name in [
            "etag",
            "last-modified",
            "cache-control",
            "age",
            "retry-after",
            "expires",
            "date",
        ] {
            if let Some(value) = response.headers().get(name).and_then(|v| v.to_str().ok())
                && value.len() <= 4096
            {
                headers.insert(name.into(), value.into());
            }
        }
        // Error/redirect bodies are not needed and can be arbitrarily large HTML.
        if status != 200 {
            return Ok(Response {
                status,
                headers,
                body: String::new(),
            });
        }
        let mut bytes = Vec::new();
        let mut reader = response.body_mut().as_reader();
        let mut buffer = [0u8; 16384];
        loop {
            check(cancel)?;
            let n = reader.read(&mut buffer).map_err(|e| Failure {
                message: format!("Chess.com body read/timeout: {e}"),
                retry_at: 0,
                offline: true,
                permanent: false,
            })?;
            if n == 0 {
                break;
            }
            if bytes.len() + n > limit {
                return Err(
                    "Chess.com response exceeds the configured decoded-body limit"
                        .to_owned()
                        .into(),
                );
            }
            bytes.extend_from_slice(&buffer[..n]);
        }
        let body = String::from_utf8(bytes)
            .map_err(|_| Failure::from("Chess.com response is not UTF-8".to_owned()))?;
        Ok(Response {
            status,
            headers,
            body,
        })
    }
}
impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}
impl Transport for Http {
    fn get(
        &mut self,
        url: &str,
        cache: &HttpCache,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Response> {
        api::validate_endpoint(url)?;
        if let Some(last) = self.last {
            while last.elapsed() < Duration::from_secs(1) {
                check(cancel)?;
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        let result = self.request(url, cache, limit, cancel);
        self.last = Some(Instant::now());
        result
    }
}
pub fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(
            "Sync cancelled; committed games and completed months retained"
                .to_owned()
                .into(),
        )
    } else {
        Ok(())
    }
}
fn header_time(value: &str) -> Option<i64> {
    httpdate::parse_http_date(value)
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs().min(i64::MAX as u64) as i64)
}
pub fn fresh_until(r: &Response, now: i64) -> i64 {
    let control = r
        .headers
        .get("cache-control")
        .map_or("", String::as_str)
        .to_ascii_lowercase();
    if control
        .split(',')
        .any(|s| matches!(s.trim(), "no-cache" | "no-store"))
    {
        return now;
    }
    let age = r
        .headers
        .get("age")
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
        .max(0);
    if let Some(max_age) = control.split(',').find_map(|s| {
        s.trim()
            .strip_prefix("max-age=")
            .and_then(|s| s.trim_matches('"').parse::<i64>().ok())
    }) {
        let date_age = r
            .headers
            .get("date")
            .and_then(|s| header_time(s))
            .map_or(0, |date| now.saturating_sub(date).max(0));
        return now.saturating_add(max_age.max(0).saturating_sub(age.max(date_age)).max(0));
    }
    r.headers
        .get("expires")
        .and_then(|s| header_time(s))
        .unwrap_or(now)
        .max(now)
}
pub fn response_error(r: &Response, now: i64) -> Failure {
    let retry = r
        .headers
        .get("retry-after")
        .and_then(|s| {
            s.parse::<i64>()
                .ok()
                .map(|n| now.saturating_add(n.max(0)))
                .or_else(|| header_time(s))
        })
        .unwrap_or_else(|| {
            if r.status == 429 || r.status >= 500 {
                now + 60
            } else {
                0
            }
        });
    Failure {
        message: match r.status {
            404 => "Chess.com returned HTTP 404: profile or archive not found".into(),
            429 => "Chess.com HTTP 429: throttled; Retry-After cooldown retained".into(),
            300..=399 => {
                "Chess.com redirect refused; only direct expected API endpoints are used".into()
            }
            s => format!("Chess.com HTTP {s}; no game data replaced"),
        },
        retry_at: retry,
        offline: false,
        permanent: matches!(r.status, 400..=499) && r.status != 429,
    }
}
#[cfg(test)]
impl Http {
    pub fn local(timeout: Duration) -> Self {
        let config = ureq::Agent::config_builder()
            .max_redirects(0)
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build();
        Self {
            agent: config.into(),
            last: None,
        }
    }
}
