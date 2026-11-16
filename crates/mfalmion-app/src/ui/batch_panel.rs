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
    last_review: Option<(i64, i64, String)>,
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
    pub fn review(&mut self, ui: &mut egui::Ui, identity: Option<(i64, i64, String)>, ply: usize) {
        if identity != self.last_review && !self.busy() {
            self.last_review = identity.clone();
            if let Some((profile, id, _)) = &identity {
                self.send(Command::Inspect(*profile, Some(*id)));
            }
        }
        let Some((profile, id, fingerprint, summary)) = &self.state.review else {
            return;
        };
        if identity.as_ref() != Some(&(*profile, *id, fingerprint.clone())) {
            return;
        }
        ui.separator();
        ui.strong("Mfalmion Estimated Accuracy");
        let format = |v: Option<f64>| v.map_or_else(|| "—".into(), |v| format!("{v:.1}%"));
        ui.label(format!(
            "White {} · Black {} · {} · depth {} / MultiPV {} · {}",
            format(summary.accuracy.white),
            format(summary.accuracy.black),
            summary.config.identity,
            summary.config.engine.depth,
            summary.config.engine.multipv,
            summary.config.algorithm
        ));
        ui.label(format!(
            "Eligible {:?} · excluded/unclassified {:?}",
            summary.accuracy.eligible, summary.accuracy.excluded
        ));
        ui.horizontal_wrapped(|ui| {
            for label in [
                "Brilliant",
                "Great",
                "Best",
                "Excellent",
                "Good",
                "Inaccuracy",
                "Mistake",
                "Blunder",
                "Miss",
            ] {
                ui.label(format!(
                    "{label}: {}",
                    summary.counts.get(label).copied().unwrap_or(0)
                ));
            }
        });
        if let Some(m) = ply.checked_sub(1).and_then(|i| summary.moves.get(i)) {
            ui.strong(format!(
                "{}. {} · {} · {}",
                m.ply,
                m.san,
                m.classification.label(),
                format_args!("{} · depth {}", m.confidence, m.depth)
            ));
            ui.label(format!(
                "Expected-score loss: {} · centipawn loss {:?} · best alternative {}",
                m.loss
                    .map_or_else(|| "insufficient".into(), |l| format!("{l:.4}")),
                m.centipawn_loss,
                m.best.as_deref().unwrap_or("—")
            ));
            ui.label(format!(
                "PV (UCI): {} · {}",
                m.pv.join(" "),
                m.tags.join(", ")
            ));
        }
        egui::CollapsingHeader::new("Move-by-move classifications").show(ui, |ui| {
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    for m in &summary.moves {
                        let text = egui::RichText::new(format!(
                            "{} {} — {}",
                            m.ply,
                            m.san,
                            m.classification.label()
                        ));
                        let text = if m.ply == ply {
                            text.strong().color(crate::theme::CYAN)
                        } else {
                            text
                        };
                        ui.label(text)
                            .on_hover_text(format!("Loss {:?}; best {:?}", m.loss, m.best));
                    }
                });
        });
    }
}
