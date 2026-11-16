use super::*;
use crate::notation::{self, GameRecord};
use serde::Deserialize;
use std::collections::BTreeSet;
pub const HOST: &str = "https://api.chess.com";
pub fn username(value: &str) -> Result<String> {
    if !(3..=100).contains(&value.len())
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
        || !value.bytes().any(|c| c.is_ascii_alphabetic())
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err("Chess.com username must be 3–100 ASCII letters/digits/hyphens/underscores and begin/end with a letter or digit and contain a letter".to_owned().into());
    }
    Ok(value.to_ascii_lowercase())
}
pub fn profile_url(name: &str) -> Result<String> {
    Ok(format!("{HOST}/pub/player/{}", username(name)?))
}
pub fn index_url(name: &str) -> Result<String> {
    Ok(format!("{}/games/archives", profile_url(name)?))
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Month {
    pub year: u16,
    pub month: u8,
}
impl Month {
    pub fn key(&self) -> String {
        format!("{:04}/{:02}", self.year, self.month)
    }
    pub fn url(&self, name: &str) -> Result<String> {
        Ok(format!("{}/games/{}", profile_url(name)?, self.key()))
    }
    pub fn recent(&self, current: &Self) -> bool {
        let n = i32::from(self.year) * 12 + i32::from(self.month);
        let c = i32::from(current.year) * 12 + i32::from(current.month);
        n >= c - 1
    }
    pub fn from_unix(seconds: i64) -> Self {
        // Civil date conversion, UTC; no locale/time-zone dependency.
        let z = seconds.div_euclid(86400) + 719468;
        let era = z.div_euclid(146097);
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let m = mp + if mp < 10 { 3 } else { -9 };
        Self {
            year: (y + i64::from(m <= 2)).clamp(0, 9999) as u16,
            month: m as u8,
        }
    }
}
pub fn archive_url(url: &str, name: &str) -> Result<Month> {
    let suffix = url
        .strip_prefix(&format!("{HOST}/pub/player/"))
        .ok_or_else(|| Failure::from("Unexpected archive host/path".to_owned()))?;
    let (account, path) = suffix
        .split_once("/games/")
        .ok_or_else(|| Failure::from("Unexpected archive endpoint".to_owned()))?;
    if username(account)? != username(name)? {
        return Err("Archive belongs to another account".to_owned().into());
    }
    let b = path.as_bytes();
    if b.len() != 7 || b[4] != b'/' || !b[..4].iter().chain(&b[5..]).all(u8::is_ascii_digit) {
        return Err("Invalid archive endpoint format".to_owned().into());
    }
    let year = path[..4]
        .parse::<u16>()
        .map_err(|_| Failure::from("Invalid archive year".to_owned()))?;
    let month = path[5..]
        .parse::<u8>()
        .map_err(|_| Failure::from("Invalid archive month".to_owned()))?;
    if !(2007..=9999).contains(&year) || !(1..=12).contains(&month) {
        return Err("Archive date out of range".to_owned().into());
    }
    Ok(Month { year, month })
}
pub fn validate_endpoint(url: &str) -> Result<()> {
    let suffix = url
        .strip_prefix(&format!("{HOST}/pub/player/"))
        .ok_or_else(|| {
            Failure::from("Only HTTPS api.chess.com player endpoints are allowed".to_owned())
        })?;
    let (name, path) = suffix.split_once('/').unwrap_or((suffix, ""));
    username(name)?;
    if path.is_empty() || path == "games/archives" {
        return Ok(());
    }
    archive_url(url, name).map(|_| ())
}
#[derive(Clone, Deserialize, Debug)]
pub struct PublicProfile {
    pub username: String,
    pub player_id: Option<u64>,
    pub name: Option<String>,
}
pub fn profile(text: &str, requested: &str) -> Result<PublicProfile> {
    let p: PublicProfile = serde_json::from_str(text)
        .map_err(|e| Failure::from(format!("Malformed public profile: {e}")))?;
    if username(&p.username)? != username(requested)? {
        return Err("Public profile resolves to a different username; select that account explicitly instead of merging".to_owned().into());
    }
    if p.name.as_ref().is_some_and(|n| n.len() > 512) {
        return Err("Profile display name is too long".to_owned().into());
    }
    Ok(p)
}
pub fn archives(text: &str, name: &str) -> Result<Vec<Month>> {
    #[derive(Deserialize)]
    struct Index {
        archives: Vec<String>,
    }
    let index: Index = serde_json::from_str(text)
        .map_err(|e| Failure::from(format!("Malformed archive index: {e}")))?;
    if index.archives.len() > 1200 {
        return Err("Archive index exceeds 1200 entries".to_owned().into());
    }
    let mut months = BTreeSet::new();
    for url in index.archives {
        months.insert(archive_url(&url, name)?);
    }
    Ok(months.into_iter().collect())
}
#[derive(Debug, Deserialize)]
pub struct Player {
    pub username: String,
    pub rating: Option<u32>,
    pub result: Option<String>,
}
#[derive(Debug, Deserialize)]
pub struct RemoteGame {
    pub url: String,
    pub pgn: String,
    pub white: Player,
    pub black: Player,
    pub time_class: Option<String>,
    pub time_control: Option<String>,
    pub end_time: Option<i64>,
    pub rated: Option<bool>,
    pub rules: Option<String>,
    pub eco: Option<String>,
    pub opening: Option<String>,
}
pub fn game_url(value: &str) -> Result<String> {
    let path = value
        .strip_prefix("https://www.chess.com/game/")
        .or_else(|| value.strip_prefix("https://chess.com/game/"))
        .ok_or_else(|| Failure::from("Invalid Chess.com game URL".to_owned()))?;
    let (kind, id) = path
        .split_once('/')
        .ok_or_else(|| Failure::from("Invalid game identifier".to_owned()))?;
    if !matches!(kind, "live" | "daily")
        || id.is_empty()
        || id.len() > 24
        || !id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("Invalid game URL endpoint".to_owned().into());
    }
    Ok(format!("https://www.chess.com/game/{kind}/{id}"))
}
impl RemoteGame {
    pub fn validate(&self, account: &str) -> Result<GameRecord> {
        let white = username(&self.white.username)?;
        let black = username(&self.black.username)?;
        let account = username(account)?;
        if account != white && account != black {
            return Err("Game does not belong to the selected account"
                .to_owned()
                .into());
        }
        game_url(&self.url)?;
        if self.rules.as_deref().is_some_and(|s| s != "chess") {
            return Err("Unsupported nonstandard chess variant".to_owned().into());
        }
        if self.white.rating.is_some_and(|n| n > 10000)
            || self.black.rating.is_some_and(|n| n > 10000)
            || self.end_time.is_some_and(|n| n < 0)
            || self.time_class.as_ref().is_some_and(|s| s.len() > 64)
            || self.time_control.as_ref().is_some_and(|s| s.len() > 128)
            || self.white.result.as_ref().is_some_and(|s| s.len() > 128)
            || self.black.result.as_ref().is_some_and(|s| s.len() > 128)
        {
            return Err("Invalid game metadata".to_owned().into());
        }
        if self.eco.as_ref().is_some_and(|s| s.len() > 2048)
            || self.opening.as_ref().is_some_and(|s| s.len() > 512)
        {
            return Err("Opening metadata exceeds limit".to_owned().into());
        }
        let mut records = notation::parse_pgn(&self.pgn).map_err(Failure::from)?;
        if records.len() != 1 {
            return Err("Expected exactly one PGN per API game".to_owned().into());
        }
        let record = records.remove(0);
        for (tag, name) in [("White", white), ("Black", black)] {
            if record
                .headers
                .get(tag)
                .map(|s| s.to_ascii_lowercase())
                .as_deref()
                != Some(&name)
            {
                return Err("API player and PGN player mismatch".to_owned().into());
            }
        }
        Ok(record)
    }
}
pub fn monthly(text: &str) -> Result<Vec<serde_json::Value>> {
    #[derive(Deserialize)]
    struct Archive {
        games: Vec<serde_json::Value>,
    }
    let a: Archive = serde_json::from_str(text)
        .map_err(|e| Failure::from(format!("Malformed monthly archive: {e}")))?;
    if a.games.len() > 10000 {
        return Err("Monthly archive exceeds 10,000 games".to_owned().into());
    }
    Ok(a.games)
}
