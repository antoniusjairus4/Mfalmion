use crate::sync::{self, Preferences, Progress, Schedule, worker::Client};
use eframe::egui;
use std::sync::{Arc, atomic::Ordering};
#[derive(Default)]
pub struct SyncPanel {
    client: Option<Client>,
    pub preferences: Preferences,
    schedule: Option<Schedule>,
    pub progress: Progress,
    profile: i64,
    need_info: bool,
    closing: bool,
    pub refresh: bool,
    pub save_preferences: bool,
    initialized: bool,
}
impl SyncPanel {
    pub fn reload(&mut self) {
        self.profile = 0;
        self.need_info = true;
    }
    pub fn busy(&self) -> bool {
        self.client
            .as_ref()
            .is_some_and(|c| c.busy.load(Ordering::Acquire))
    }
    pub fn tick(&mut self, ctx: &egui::Context, profile: i64, username: &str, allow_start: bool) {
        if self.closing {
            return;
        }
        if self.client.is_none() && !self.initialized {
            self.initialized = true;
            let ctx = ctx.clone();
            match Client::new(Arc::new(move || ctx.request_repaint())) {
                Ok(c) => self.client = Some(c),
                Err(e) => {
                    self.progress.error = Some(e);
                    self.progress.status = "error".into();
                }
            }
        }
        if self.profile != profile {
            if let Some(c) = &self.client {
                c.cancel();
            }
            self.profile = profile;
            self.need_info = true;
            self.progress = Progress {
                profile,
                username: username.into(),
                status: "Not checked".into(),
                ..Default::default()
            };
            self.schedule = Some(Schedule::new(profile, &self.preferences, sync::now()));
        }
        if let Some(update) = self.client.as_ref().and_then(Client::take) {
            if update.finished {
                self.refresh = true;
            }
            if update.progress.profile == profile || update.progress.profile == 0 {
                if update.finished
                    && let Some(schedule) = &mut self.schedule
                {
                    schedule.finish(
                        &self.preferences,
                        sync::now(),
                        if update.progress.status == "cancelled" {
                            None
                        } else {
                            update.failure.as_ref()
                        },
                    );
                    schedule.next = schedule.next.max(update.progress.retry_at);
                }
                self.progress = update.progress;
            }
        }
        if self.need_info && !self.busy() && allow_start {
            if let Some(c) = &self.client {
                match c.info(profile) {
                    Ok(()) => self.need_info = false,
                    Err(e) => self.progress.error = Some(e),
                }
            }
        } else if self
            .schedule
            .as_ref()
            .is_some_and(|s| s.due(profile, &self.preferences, sync::now(), self.busy()))
            && allow_start
        {
            self.start(false);
        }
        // One scheduled wake, not a polling loop. Worker callbacks wake for progress.
        if self.preferences.enabled
            && let Some(delay) = self
                .schedule
                .as_ref()
                .and_then(|s| s.next_wake(sync::now()))
        {
            ctx.request_repaint_after(std::time::Duration::from_secs(delay));
        }
    }
    fn start(&mut self, full: bool) {
        if let Err(mut e) = sync::api::username(&self.progress.username) {
            e.permanent = true;
            if let Some(schedule) = &mut self.schedule {
                schedule.finish(&self.preferences, sync::now(), Some(&e));
            }
            self.progress.error = Some(e.message);
            return;
        }
        if let Some(c) = &self.client {
            match c.start(self.profile, full) {
                Ok(()) => {
                    self.progress.status = "Queued".into();
                    self.progress.error = None;
                }
                Err(e) => self.progress.error = Some(e),
            }
        }
    }
    pub fn cancel(&self) {
        if let Some(c) = &self.client {
            c.cancel();
        }
    }
    pub fn shutdown(&mut self) {
        self.closing = true;
        if let Some(c) = &self.client {
            c.shutdown();
        }
    }
    pub fn stopped(&self) -> bool {
        self.client
            .as_ref()
            .is_none_or(|c| c.done.load(Ordering::Acquire))
    }
    pub fn show(&mut self, ui: &mut egui::Ui, allow_start: bool) {
        egui::CollapsingHeader::new("Chess.com Sync").default_open(true).show(ui,|ui|{
            ui.label(format!("Selected: {} · {}",self.progress.username,self.progress.status));
            ui.label("Public data only. Mfalmion is not affiliated with Chess.com.");
            ui.label(format!("Last successful sync: {}",self.progress.last_success.map_or_else(||"Never".into(),timestamp)));
            if let Some(e)=&self.progress.error{ui.colored_label(crate::theme::PURPLE,e);}
            if self.progress.retry_at>sync::now(){ui.label(format!("Retry no earlier than {}",timestamp(self.progress.retry_at)));}
            ui.label(format!("Archives: {}/{} · Imported {} · Skipped {} · Failed {}",self.progress.processed,self.progress.discovered,self.progress.imported,self.progress.skipped,self.progress.failed));
            if self.progress.discovered>0{ui.add(egui::ProgressBar::new(self.progress.processed as f32/self.progress.discovered as f32).show_percentage());}
            ui.horizontal_wrapped(|ui|{
                let enabled=allow_start&&!self.busy()&&self.progress.retry_at<=sync::now();
                if ui.add_enabled(enabled,egui::Button::new("Sync Now")).clicked(){self.start(false);}
                if ui.add_enabled(enabled,egui::Button::new("Full Resync")).clicked(){self.start(true);}
                if ui.add_enabled(self.busy(),egui::Button::new("Cancel Sync")).clicked(){self.cancel();}
            });
            let old=self.preferences.clone();ui.add_enabled_ui(allow_start,|ui|{ui.horizontal(|ui|{ui.checkbox(&mut self.preferences.enabled,"Auto Sync");ui.add(egui::DragValue::new(&mut self.preferences.interval_minutes).range(5..=10080).prefix("Interval (minutes): "));});});
            if self.preferences!=old{self.schedule=Some(Schedule::new(self.profile,&self.preferences,sync::now()));self.save_preferences=true;}
            ui.label("Automatic checks run only while open. Server cache freshness and cooldowns may defer downloads. Full Resync revisits all available months without replacing local PGNs.");
        });
    }
}
fn timestamp(value: i64) -> String {
    u64::try_from(value)
        .ok()
        .filter(|s| *s <= 253402300799)
        .and_then(|s| std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(s)))
        .map_or_else(|| "Unknown".into(), httpdate::fmt_http_date)
}

#[cfg(test)]
mod tests {
    #[test]
    fn corrupt_or_extreme_timestamps_do_not_panic() {
        assert_eq!(super::timestamp(i64::MAX), "Unknown");
        assert_eq!(super::timestamp(-1), "Unknown");
        assert_eq!(super::timestamp(0), "Thu, 01 Jan 1970 00:00:00 GMT");
    }
}
