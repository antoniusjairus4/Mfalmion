use crate::{
    analysis::{
        Config,
        worker::{Client, Command, State},
    },
    storage::batch::Selection,
};
use eframe::egui;
use std::sync::{Arc, atomic::Ordering};
#[derive(Default)]
pub struct BatchPanel {
    client: Option<Client>,
    pub selected: Option<i64>,
    pub requested: Option<Command>,
    pub state: State,
    config: Config,
    profile: i64,
    last_review: Option<crate::storage::review::ReviewKey>,
    dispatched: bool,
    dispatch_revision: u64,
    pub changed: bool,
    dirty: bool,
}
impl BatchPanel {
    pub fn busy(&self) -> bool {
        self.requested.is_some()
            || self.dispatched
            || self.state.status == "running"
            || self
                .client
                .as_ref()
                .is_some_and(|c| c.active.load(Ordering::Acquire))
    }
    pub fn tick(&mut self, ctx: &egui::Context, profile: i64) {
        if self.client.is_none() {
            let ctx = ctx.clone();
            match Client::new(Arc::new(move || ctx.request_repaint())) {
                Ok(c) => self.client = Some(c),
                Err(e) => {
                    self.state.message = e;
                    return;
                }
            }
        }
        if let Some(c) = &self.client {
            let state = c.snapshot();
            if state.config != self.state.config {
                self.last_review = None;
            }
            if self.dispatched
                && !c.active.load(Ordering::Acquire)
                && (state.revision > self.dispatch_revision && state.status != "running")
            {
                self.dispatched = false;
                self.changed = true;
                self.last_review = None;
            }
            if !self.dirty && state.profile == profile {
                self.config = state.config.clone();
            }
            self.state = state;
        }
        if self.profile != profile && !self.busy() {
            self.profile = profile;
            self.selected = None;
            self.last_review = None;
            self.send(Command::Inspect(profile, None));
        }
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
    fn send(&mut self, c: Command) {
        if let Some(client) = &self.client
            && let Err(e) = client.send(c)
        {
            self.state.message = e;
            self.dispatched = false;
        }
    }
    pub fn dispatch(&mut self) {
        if let Some(command) = self.requested.take() {
            self.dispatched = true;
            self.dispatch_revision = self.state.revision;
            self.state.message.clear();
            self.send(command);
            self.dirty = false;
        }
    }
    pub fn invalidate(&mut self) {
        self.profile = 0;
        self.last_review = None;
        self.state.review = None;
        self.state.review_data = Default::default();
    }
    pub fn shutdown(&self) {
        if let Some(c) = &self.client {
            c.shutdown();
        }
    }
    pub fn stopped(&self) -> bool {
        self.client
            .as_ref()
            .is_none_or(|c| c.done.load(Ordering::Acquire))
    }
    pub fn show(&mut self, ui: &mut egui::Ui, profile: i64, name: &str, available: bool) {
        egui::CollapsingHeader::new("Account Analysis").default_open(true).show(ui,|ui|{
            ui.label(format!("{name} · eligible {} · analyzed {} · pending {} · failed {}",self.state.coverage.total,self.state.coverage.analyzed,self.state.coverage.pending,self.state.coverage.failed));
            ui.label(format!("{} · job {:?} · game {:?} · position {} / {} · games {} / {}",self.state.status,self.state.job,self.state.current_game,self.state.position,self.state.positions,self.state.completed,self.state.total));
            if self.state.total>0{ui.add(egui::ProgressBar::new(self.state.completed as f32/self.state.total as f32).show_percentage());}
            ui.label(&self.state.message);ui.label(&self.state.statistics);
            ui.add_enabled_ui(!self.busy() && available,|ui|{
                ui.horizontal_wrapped(|ui|{
                    ui.label("Batch depth");self.dirty|=ui.add(egui::DragValue::new(&mut self.config.engine.depth).range(1..=32)).changed();
                    ui.label("MultiPV");self.dirty|=ui.add(egui::DragValue::new(&mut self.config.engine.multipv).range(1..=3)).changed();
                    ui.label("Threads");self.dirty|=ui.add(egui::DragValue::new(&mut self.config.engine.threads).range(1..=crate::engine::Settings::max_threads())).changed();
                    ui.label("Hash MiB");self.dirty|=ui.add(egui::DragValue::new(&mut self.config.engine.hash_mib).range(16..=512)).changed();
                });
                egui::CollapsingHeader::new("Batch engine executable / identity").show(ui,|ui|{self.dirty|=ui.text_edit_singleline(&mut self.config.engine.path).changed();self.dirty|=ui.text_edit_singleline(&mut self.config.identity).changed();});
                ui.label("Higher depth increases computational cost. Batch settings are independent of interactive analysis. Multi-thread searches can vary; stored completed evidence is reused exactly.");
                if ui.button("Save batch settings").clicked(){self.send(Command::Save(self.config.clone()));self.dirty=false;}
                ui.horizontal_wrapped(|ui|{
                    if ui.button("Analyze All Unanalyzed").clicked(){self.requested=Some(Command::Start(profile,self.config.clone(),Selection::Unanalyzed));}
                    if ui.add_enabled(self.selected.is_some(),egui::Button::new("Analyze Selected Game")).clicked() && let Some(id)=self.selected{self.requested=Some(Command::Start(profile,self.config.clone(),Selection::Selected(id)));}
                    if ui.add_enabled(self.selected.is_some(),egui::Button::new("Reanalyze Selected Game")).clicked() && let Some(id)=self.selected{self.requested=Some(Command::Start(profile,self.config.clone(),Selection::Reanalyze(id)));}
                    if ui.button("Retry Failed").clicked(){self.requested=Some(Command::Start(profile,self.config.clone(),Selection::RetryFailed));}
                    if ui.button("Refresh analysis status").clicked(){self.send(Command::Inspect(profile,None));}
                });
                if ui.add_enabled(self.state.job.is_some() && matches!(self.state.status.as_str(),"paused"|"pending"),egui::Button::new("Resume saved job (original settings)")).clicked(){self.requested=Some(Command::Resume(profile));}
            });
            ui.horizontal(|ui|{
                if ui.add_enabled(self.busy(),egui::Button::new("Pause")).clicked(){if let Some(c)=&self.client{c.interrupt(false);}self.requested=None;}
                if ui.add_enabled(self.busy() || self.state.status=="paused",egui::Button::new("Cancel")).clicked(){if self.busy(){if let Some(c)=&self.client{c.interrupt(true);}}else{self.send(Command::Cancel(profile));}self.requested=None;}
                ui.label(format!("Selected game: {:?}",self.selected));
            });
        });
    }
    pub fn load_review(&mut self, identity: Option<(i64, i64, String)>, ply: usize) {
        let key = identity.map(
            |(profile, game, fingerprint)| crate::storage::review::ReviewKey {
                profile,
                game,
                fingerprint,
                ply,
            },
        );
        if key != self.last_review && !self.busy() {
            if let Some(key) = key {
                if let Some(client) = &self.client
                    && client.send(Command::Review(key.clone())).is_ok()
                {
                    self.last_review = Some(key);
                }
            } else {
                self.last_review = None;
            }
        }
    }
    pub fn review_data(&self) -> crate::storage::review::ReviewData {
        self.state.review_data.clone()
    }
}
