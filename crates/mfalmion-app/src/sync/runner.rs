use super::{
    api::Month,
    client::{Response, Transport, check},
    *,
};
use crate::storage::{Store, sync::HttpCache};
use std::sync::atomic::AtomicBool;
struct Fetch<'a> {
    url: &'a str,
    limit: usize,
    body_needed: bool,
    unconditional: bool,
}
pub struct Runner<T> {
    pub transport: T,
}
impl<T: Transport> Runner<T> {
    fn fetch(
        &mut self,
        store: &Store,
        profile: i64,
        options: Fetch<'_>,
        cancel: &AtomicBool,
        now: i64,
    ) -> Result<(Option<String>, HttpCache)> {
        let Fetch {
            url,
            limit,
            body_needed,
            unconditional,
        } = options;
        check(cancel)?;
        let cache = store.http_cache(profile, url)?;
        let complete = if body_needed {
            cache.body.is_some()
        } else {
            true
        };
        if cache.fresh_until > now && complete {
            return Ok((cache.body.clone(), cache));
        }
        let blank = HttpCache::default();
        let received = std::time::Instant::now();
        let response = self.transport.get(
            url,
            if unconditional { &blank } else { &cache },
            limit,
            cancel,
        )?;
        let now = now.saturating_add(received.elapsed().as_secs() as i64);
        check(cancel)?;
        if response.status != 200 && response.status != 304 {
            let failure = client::response_error(&response, now);
            if failure.retry_at > now {
                store.cooldown(failure.retry_at)?;
            }
            return Err(failure);
        }
        let no_store = response.headers.get("cache-control").is_some_and(|s| {
            s.split(',')
                .any(|v| v.trim().eq_ignore_ascii_case("no-store"))
        });
        let fresh = client::fresh_until(&response, now);
        if response.status == 304 {
            if body_needed && cache.body.is_none() {
                return Err("HTTP 304 without cached endpoint data".to_owned().into());
            }
            if no_store {
                return Ok((cache.body, HttpCache::default()));
            }
            let cache = HttpCache {
                etag: response.headers.get("etag").cloned().or(cache.etag),
                modified: response
                    .headers
                    .get("last-modified")
                    .cloned()
                    .or(cache.modified),
                fresh_until: fresh,
                body: cache.body,
            };
            return Ok((cache.body.clone(), cache));
        }
        let Response { body, headers, .. } = response;
        let cache = HttpCache {
            etag: if no_store {
                None
            } else {
                headers.get("etag").cloned()
            },
            modified: if no_store {
                None
            } else {
                headers.get("last-modified").cloned()
            },
            fresh_until: fresh,
            body: if body_needed && !no_store {
                Some(body.clone())
            } else {
                None
            },
        };
        Ok((Some(body), cache))
    }
    pub fn run(
        &mut self,
        store: &mut Store,
        profile: i64,
        full: bool,
        cancel: &AtomicBool,
        now: i64,
        mut emit: impl FnMut(Progress),
    ) -> (Progress, Option<Failure>) {
        let initial = store.sync_info(profile);
        let mut progress = match initial {
            Ok(p) => p,
            Err(e) => {
                return (
                    Progress {
                        profile,
                        status: "error".into(),
                        error: Some(e.clone()),
                        ..Default::default()
                    },
                    Some(e.into()),
                );
            }
        };
        progress.status = "syncing".into();
        progress.error = None;
        progress.discovered = 0;
        progress.processed = 0;
        progress.imported = 0;
        progress.skipped = 0;
        progress.failed = 0;
        let started = std::time::Instant::now();
        let at = || now.saturating_add(started.elapsed().as_secs() as i64);
        let result = (|| -> Result<()> {
            api::username(&progress.username)?;
            let cooldown = store.http_cooldown()?;
            if cooldown > now {
                return Err(Failure {
                    message: "Chess.com cooldown is active; wait until Retry-After expires".into(),
                    retry_at: cooldown,
                    offline: false,
                    permanent: false,
                });
            }
            store.save_sync(&progress)?;
            emit(progress.clone());
            let url = api::profile_url(&progress.username)?;
            let (body, cache) = self.fetch(
                store,
                profile,
                Fetch {
                    url: &url,
                    limit: 1024 * 1024,
                    body_needed: true,
                    unconditional: false,
                },
                cancel,
                at(),
            )?;
            let public = api::profile(
                body.as_deref()
                    .ok_or_else(|| Failure::from("Missing profile JSON".to_owned()))?,
                &progress.username,
            )?;
            store.resolve_public(profile, &public)?;
            store.save_http(profile, &url, &cache)?;
            progress.username = public.username;
            let url = api::index_url(&progress.username)?;
            let (body, cache) = self.fetch(
                store,
                profile,
                Fetch {
                    url: &url,
                    limit: 1024 * 1024,
                    body_needed: true,
                    unconditional: false,
                },
                cancel,
                at(),
            )?;
            let months = api::archives(
                body.as_deref()
                    .ok_or_else(|| Failure::from("Missing archive index".to_owned()))?,
                &progress.username,
            )?;
            store.save_http(profile, &url, &cache)?;
            progress.discovered = months.len();
            store.save_sync(&progress)?;
            emit(progress.clone());
            let current = Month::from_unix(now);
            for month in months {
                check(cancel)?;
                let key = month.key();
                let complete = store.month_done(profile, &key)?;
                if complete && !full && !month.recent(&current) {
                    let (saved, failed) = store.retained_month_counts(profile, &key)?;
                    progress.skipped += saved;
                    progress.failed += failed;
                    progress.processed += 1;
                    emit(progress.clone());
                    continue;
                }
                let url = month.url(&progress.username)?;

                // Interrupted months must have no reusable HTTP metadata.
                let (body, cache) = self.fetch(
                    store,
                    profile,
                    Fetch {
                        url: &url,
                        limit: 32 * 1024 * 1024,
                        body_needed: false,
                        unconditional: full || !complete,
                    },
                    cancel,
                    at(),
                )?;
                let mut counts = (0, 0, 0);
                if let Some(body) = body {
                    let games = api::monthly(&body)?;
                    store.begin_month(profile, &key, &url)?;
                    let mut positions = 0;
                    for (index, value) in games.into_iter().enumerate() {
                        check(cancel)?;
                        let parsed =
                            (|| -> Result<(api::RemoteGame, crate::notation::GameRecord)> {
                                let game: api::RemoteGame =
                                    serde_json::from_value(value).map_err(|e| {
                                        Failure::from(format!("Malformed game metadata: {e}"))
                                    })?;
                                let record = game.validate(&progress.username)?;
                                positions += record.positions.len();
                                if positions > 200_000 {
                                    return Err(
                                        "Monthly import exceeds 200,000 validated positions"
                                            .to_owned()
                                            .into(),
                                    );
                                }
                                Ok((game, record))
                            })();
                        match parsed {
                            Ok((game, record)) => {
                                if store.import_remote(profile, &game, &record)? {
                                    counts.0 += 1;
                                    progress.imported += 1;
                                } else {
                                    counts.1 += 1;
                                    progress.skipped += 1;
                                }
                            }
                            Err(e) => {
                                counts.2 += 1;
                                progress.failed += 1;
                                let detail: String = e.message.chars().take(2000).collect();
                                progress.error =
                                    Some(format!("{} game {}: {}", key, index + 1, detail));
                                store.sync_failure(profile, &key, index, &e.message)?;
                            }
                        }
                        if index % 25 == 0 {
                            store.save_sync(&progress)?;
                            emit(progress.clone());
                        }
                    }
                    check(cancel)?;
                    store.finish_month(&progress, &key, counts, &cache, &url)?;
                } else {
                    if !complete {
                        return Err("HTTP 304 without a completed monthly checkpoint"
                            .to_owned()
                            .into());
                    }
                    let (saved, failed) = store.retained_month_counts(profile, &key)?;
                    progress.skipped += saved;
                    progress.failed += failed;
                    store.refresh_month(profile, &key, &cache, &url)?;
                }
                check(cancel)?;
                progress.processed += 1;
                store.save_sync(&progress)?;
                emit(progress.clone());
            }
            check(cancel)?;
            Ok(())
        })();
        let failure = result.err();
        if let Some(e) = &failure {
            progress.status = if cancel.load(std::sync::atomic::Ordering::Acquire) {
                "cancelled"
            } else if e.offline {
                "offline"
            } else {
                "error"
            }
            .into();
            progress.error = Some(e.message.clone());
            progress.retry_at = e.retry_at;
        } else {
            progress.status = if progress.failed == 0 {
                "complete"
            } else {
                "complete_with_errors"
            }
            .into();
            progress.last_success = Some(at());
            progress.retry_at = 0;
            if progress.failed > 0 {
                let detail = progress.error.take().unwrap_or_else(|| {
                    "Saved rejected-item details retained in sync_failures".into()
                });
                progress.error = Some(format!(
                    "{} rejected game(s); originals retained. {}",
                    progress.failed, detail
                ));
            }
        }
        if let Err(e) = store.save_sync(&progress) {
            progress.status = "error".into();
            progress.error = Some(format!("Cannot persist sync progress: {e}"));
            return (progress.clone(), Some(e.into()));
        }
        emit(progress.clone());
        (progress, failure)
    }
}
