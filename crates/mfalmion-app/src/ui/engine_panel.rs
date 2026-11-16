use crate::{
    engine::{
        self, EngineClient, Event, Request, Settings, Status,
        protocol::{Bound, Snapshot},
    },
    theme,
};
use eframe::egui::{self, RichText};
use shakmaty::Chess;
use std::sync::Arc;

#[derive(Default)]
pub struct EnginePanel {
    client: Option<EngineClient>,
    pub batch_owned: bool,
    pub settings: Settings,
    active_settings: Settings,
    enabled: bool,
    generation: u64,
    key: Option<(String, u64)>,
    pub status: Status,
    snapshot: Option<Snapshot>,
    best_move: Option<Option<String>>,
    completion: Option<(String, Settings, Snapshot)>,
    name: Option<String>,
    error: Option<String>,
}
impl EnginePanel {
    fn next_generation(&mut self) {
        self.completion = None;
        self.snapshot = None;
        self.best_move = None;
        self.error = None;
        if let Some(next) = self.generation.checked_add(1) {
            self.generation = next;
        } else {
            self.enabled = false;
            self.status = Status::Error;
            self.error = Some("Analysis request counter exhausted; restart the app".into());
            if let Some(client) = &self.client {
                client.shutdown();
            }
        }
    }
    pub fn invalidate(&mut self) {
        self.next_generation();
        self.key = None;
        if self.enabled
            && let Some(client) = &self.client
        {
            client.stop(self.generation);
            self.status = Status::Stopping;
        }
    }
    fn start(&mut self, ctx: &egui::Context) {
        if let Err(error) = self.settings.validate() {
            self.error = Some(error);
            if !self.enabled {
                self.status = Status::Error;
            }
            return;
        }
        if self.batch_owned {
            return;
        }
        if self.client.as_ref().is_none_or(EngineClient::is_shutdown) {
            let ctx = ctx.clone();
            match EngineClient::new(Arc::new(move || ctx.request_repaint())) {
                Ok(client) => self.client = Some(client),
                Err(error) => {
                    self.error = Some(error);
                    self.status = Status::Error;
                    return;
                }
            }
        }
        self.active_settings = self.settings.clone();
        self.enabled = true;
        self.invalidate();
        if self.enabled {
            self.status = Status::Starting;
        }
    }
    fn stop(&mut self) {
        self.enabled = false;
        self.invalidate();
        if let Some(client) = &self.client {
            client.stop(self.generation);
            self.status = Status::Stopping;
        }
    }
    fn accept(&mut self, event: Event) {
        if event.id() != self.generation {
            return;
        }
        match event {
            Event::Status(_, status) => self.status = status,
            Event::Identity(_, name) => self.name = Some(name),
            Event::Snapshot(_, snapshot) if self.enabled => self.snapshot = Some(snapshot),
            Event::BestMove(_, mv) if self.enabled => {
                self.best_move = Some(mv);
                if let (Some(name), Some(snapshot)) = (&self.name, &self.snapshot)
                    && snapshot.depth == self.active_settings.depth
                {
                    self.completion =
                        Some((name.clone(), self.active_settings.clone(), snapshot.clone()));
                }
            }
            Event::Error(_, error) => {
                self.status = Status::Error;
                self.error = Some(error);
                self.enabled = false;
                self.snapshot = None;
                self.best_move = None;
            }
            _ => {}
        }
    }
    pub fn completed_analysis(&mut self) -> Option<(String, Settings, Snapshot)> {
        self.completion.take()
    }
    pub fn sync(&mut self, position: &Chess, session: u64) {
        let key = (crate::notation::fen(position), session);
        if self.enabled && self.key.as_ref() != Some(&key) {
            self.next_generation();
            if !self.enabled {
                return;
            }
            self.key = Some(key);
            self.status = Status::Starting;
            if let Some(client) = &self.client {
                client.analyze(Request {
                    id: self.generation,
                    session,
                    position: position.clone(),
                    history: None,
                    settings: self.active_settings.clone(),
                });
            }
        }
        if let Some(client) = &self.client {
            let events = client.drain();
            for event in events {
                self.accept(event);
            }
        }
    }
    pub fn shutdown(&mut self) {
        self.enabled = false;
        self.snapshot = None;
        if let Some(client) = &self.client {
            client.shutdown();
            self.status = Status::Stopping;
        }
    }
    pub fn is_shutdown(&self) -> bool {
        self.client.as_ref().is_none_or(EngineClient::is_shutdown)
    }
    pub fn show(&mut self, ui: &mut egui::Ui, position: &Chess, session: u64) {
        self.sync(position, session);
        ui.separator();
        ui.label(RichText::new("STOCKFISH ANALYSIS").color(theme::CYAN));
        ui.label(format!(
            "{:?}{}",
            self.status,
            self.name
                .as_ref()
                .map_or(String::new(), |name| format!(" · {name}"))
        ));
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.batch_owned,
                    egui::Button::new(if self.enabled {
                        "Stop Analysis"
                    } else {
                        "Start Analysis"
                    }),
                )
                .clicked()
            {
                if self.enabled {
                    self.stop();
                } else {
                    self.start(ui.ctx());
                }
            }
            if self.enabled && ui.button("Apply settings").clicked() {
                self.start(ui.ctx());
            }
        });
        ui.horizontal(|ui| {
            ui.label("Depth");
            ui.add(egui::DragValue::new(&mut self.settings.depth).range(1..=32));
            ui.label("MultiPV");
            ui.add(egui::DragValue::new(&mut self.settings.multipv).range(1..=3));
        });
        ui.collapsing("Engine settings", |ui| {
            ui.label("Executable path"); ui.add(egui::TextEdit::singleline(&mut self.settings.path).desired_width(205.0).char_limit(4096));
            ui.horizontal(|ui| { ui.label("Threads"); ui.add(egui::DragValue::new(&mut self.settings.threads).range(1..=Settings::max_threads())); });
            ui.horizontal(|ui| { ui.label("Hash (MiB)"); ui.add(egui::DragValue::new(&mut self.settings.hash_mib).range(16..=512)); });
            ui.label(RichText::new("Search stops at the requested depth or 30 seconds. Apply settings to restart an active analysis.").small().color(theme::MUTED));
        });
        if let Some(snapshot) = &self.snapshot {
            if let Some(primary) = snapshot.lines.first() {
                if let Some(score) = primary.score {
                    ui.label(
                        RichText::new(score.label(primary.bound))
                            .size(22.0)
                            .color(theme::MINT),
                    );
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 14.0),
                        egui::Sense::hover(),
                    );
                    ui.painter().rect_filled(rect, 2, egui::Color32::BLACK);
                    let white = egui::Rect::from_min_size(
                        rect.min,
                        egui::vec2(rect.width() * score.white_fraction(), rect.height()),
                    );
                    ui.painter().rect_filled(white, 2, egui::Color32::WHITE);
                    response.on_hover_text("White on the left, Black on the right. A nonlinear evaluation scale, not a win probability. Bound scores are not exact evaluations.");
                    ui.label(
                        RichText::new("White  ◀  advantage  ▶  Black")
                            .small()
                            .color(theme::MUTED),
                    );
                    if primary.bound != Bound::Exact {
                        ui.label("Bound estimate · not exact");
                    }
                }
                ui.label(format!(
                    "Depth {} · seldepth {}",
                    snapshot.depth,
                    primary.seldepth.map_or("—".into(), |n| n.to_string())
                ));
                ui.label(format!(
                    "Nodes {} · {} n/s · {} ms",
                    primary.nodes.map_or("—".into(), |n| n.to_string()),
                    primary.nps.map_or("—".into(), |n| n.to_string()),
                    primary.time_ms.map_or("—".into(), |n| n.to_string())
                ));
            }
            for line in &snapshot.lines {
                let pv = engine::protocol::decode_pv(position, &line.pv);
                ui.label(format!(
                    "{}. {}",
                    line.multipv,
                    line.score.map_or("—".into(), |s| s.label(line.bound))
                ));
                ui.label(
                    RichText::new(if pv.text.is_empty() {
                        "No continuation"
                    } else {
                        &pv.text
                    })
                    .small(),
                );
                if let Some(error) = pv.invalid {
                    ui.colored_label(theme::DANGER, error);
                }
            }
        } else {
            ui.label(
                RichText::new("No current evaluation.")
                    .small()
                    .color(theme::MUTED),
            );
        }
        if let Some(mv) = &self.best_move {
            ui.label(format!(
                "Best move: {}",
                mv.as_deref().unwrap_or("none (terminal position)")
            ));
        }
        if let Some(error) = &self.error {
            ui.colored_label(theme::DANGER, error);
        }
        self.sync(position, session); // Include Start/Stop/settings clicks from this frame.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot() -> Snapshot {
        let engine::protocol::Message::Info(info) =
            engine::protocol::parse("info depth 4 score cp 40 pv e2e4", shakmaty::Color::White)
                .unwrap()
        else {
            panic!("info");
        };
        Snapshot {
            depth: 4,
            lines: vec![info],
        }
    }
    #[test]
    fn generations_reject_stale_scores_bestmoves_status_and_errors() {
        let mut panel = EnginePanel {
            enabled: true,
            generation: 2,
            ..Default::default()
        };
        for event in [
            Event::Snapshot(1, snapshot()),
            Event::BestMove(1, Some("1. e4".into())),
            Event::Status(1, Status::Analyzing),
            Event::Error(1, "old".into()),
        ] {
            panel.accept(event);
        }
        assert!(panel.snapshot.is_none());
        assert!(panel.best_move.is_none());
        assert!(panel.error.is_none());
        assert_eq!(panel.status, Status::Offline);
        panel.accept(Event::Snapshot(2, snapshot()));
        assert!(panel.snapshot.is_some());
        panel.invalidate();
        assert!(panel.snapshot.is_none());
        assert_eq!(panel.generation, 3);
        panel.accept(Event::Snapshot(2, snapshot()));
        assert!(panel.snapshot.is_none());
        panel.enabled = false;
        panel.accept(Event::Snapshot(3, snapshot()));
        assert!(panel.snapshot.is_none());
    }
    #[test]
    fn changed_fen_and_session_invalidate_without_starting_an_engine() {
        let mut panel = EnginePanel {
            enabled: true,
            ..Default::default()
        };
        let root = Chess::default();
        panel.sync(&root, 1);
        let first = panel.generation;
        panel.accept(Event::Snapshot(first, snapshot()));
        panel.sync(&root, 1);
        assert_eq!(panel.generation, first);
        assert!(panel.snapshot.is_some());
        panel.sync(&root, 2);
        assert!(panel.generation > first);
        assert!(panel.snapshot.is_none());
        let before = panel.generation;
        let position = crate::notation::parse_fen(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
        )
        .unwrap();
        panel.sync(&position, 2);
        assert!(panel.generation > before);
        assert!(panel.client.is_none());
    }
    #[test]
    fn stop_clears_display_and_counter_never_wraps() {
        let mut panel = EnginePanel {
            enabled: true,
            generation: 10,
            snapshot: Some(snapshot()),
            ..Default::default()
        };
        panel.stop();
        assert!(!panel.enabled);
        assert!(panel.snapshot.is_none());
        assert_eq!(panel.generation, 11);
        panel.generation = u64::MAX;
        panel.enabled = true;
        panel.invalidate();
        assert_eq!(panel.generation, u64::MAX);
        assert!(!panel.enabled);
        assert_eq!(panel.status, Status::Error);
        assert!(panel.error.is_some());
    }
    #[test]
    fn only_live_requested_depth_completion_is_persistable() {
        let mut panel = EnginePanel {
            enabled: true,
            generation: 3,
            name: Some("Stockfish 17.1".into()),
            active_settings: Settings {
                depth: 4,
                ..Default::default()
            },
            ..Default::default()
        };
        panel.accept(Event::Snapshot(3, snapshot()));
        assert!(panel.completed_analysis().is_none());
        panel.accept(Event::BestMove(3, Some("e4".into())));
        assert!(panel.completed_analysis().is_some());
        panel.accept(Event::BestMove(3, Some("e4".into())));
        panel.invalidate();
        assert!(panel.completed_analysis().is_none());
        panel.accept(Event::BestMove(3, Some("e4".into())));
        assert!(panel.completed_analysis().is_none());
        panel.generation = 5;
        panel.accept(Event::Snapshot(5, snapshot()));
        panel.active_settings.depth = 6;
        panel.accept(Event::BestMove(5, Some("e4".into())));
        assert!(panel.completed_analysis().is_none());
        panel.stop();
        panel.accept(Event::BestMove(panel.generation, Some("e4".into())));
        assert!(panel.completed_analysis().is_none());
    }
}
