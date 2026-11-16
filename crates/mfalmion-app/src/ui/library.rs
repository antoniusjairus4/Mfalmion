use crate::storage::{
    analysis::{Key, Line},
    games::{GameRow, Query},
    settings::Preferences,
    worker::{Client, Command, Reply},
};
use eframe::egui;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::Ordering},
};
#[derive(Default)]
pub struct Library {
    client: Option<Client>,
    sync: super::sync_panel::SyncPanel,
    pub ready: bool,
    busy: bool,
    profiles: Vec<(i64, String, bool)>,
    pub active: i64,
    username: String,
    query: Query,
    rows: Vec<GameRow>,
    pub preferences: Preferences,
    pub loaded_preferences: bool,
    pub notice: String,
    bytes: u64,
    text: String,
    file: Option<crate::file_dialog::FileTask>,
    confirm_game: Option<i64>,
    confirm_profile: Option<i64>,
    backup_path: String,
    restore_path: String,
    safety_path: String,
    confirm_restore: bool,
    pub opened: Option<(i64, i64, crate::notation::GameRecord)>,
    pub cached: Option<(Key, Vec<Line>)>,
    refresh: bool,
    pub switched: bool,
}
impl Library {
    pub fn start(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        match Client::new(Arc::new(move || ctx.request_repaint())) {
            Ok(c) => {
                self.client = Some(c);
                self.busy = true;
            }
            Err(e) => self.notice = e,
        }
    }
    pub fn send(&mut self, command: Command) -> bool {
        if matches!(command, Command::Restore(..) | Command::DeleteProfile(..)) && self.sync.busy()
        {
            self.notice = "Cancel Sync and wait before restoring or deleting a profile".into();
            return false;
        }
        if matches!(command, Command::Select(..)) {
            self.sync.cancel();
        }
        if self.busy {
            return false;
        }
        match self
            .client
            .as_ref()
            .ok_or("Storage unavailable".to_owned())
            .and_then(|c| c.send(command))
        {
            Ok(()) => {
                self.busy = true;
                true
            }
            Err(e) => {
                self.notice = e;
                false
            }
        }
    }
    pub fn idle(&self) -> bool {
        !self.busy
    }
    pub fn available(&self) -> bool {
        self.ready && !self.busy && self.file.is_none()
    }
    pub fn poll(&mut self) {
        let reply = match self.client.as_ref().map(|c| c.receiver.try_recv()) {
            Some(Ok(reply)) => Some(reply),
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                self.busy = false;
                self.ready = false;
                self.notice =
                    "Storage worker stopped; restart after checking the database error".into();
                None
            }
            _ => None,
        };
        if let Some(reply) = reply {
            self.busy = false;
            match reply {
                Reply::State {
                    profiles,
                    active,
                    preferences,
                    sync_preferences,
                    warning,
                    bytes,
                } => {
                    self.cached = None;
                    self.rows.clear();
                    self.query.offset = 0;
                    self.profiles = profiles;
                    self.active = active;
                    self.preferences = preferences;
                    self.sync.preferences = sync_preferences;
                    self.sync.reload();
                    self.loaded_preferences = true;
                    self.bytes = bytes;
                    self.ready = true;
                    self.refresh = true;
                    self.switched = true;
                    self.notice = warning.unwrap_or_else(|| "Local database ready".into());
                }
                Reply::Games(rows) => self.rows = rows,
                Reply::Opened(profile, id, record) => self.opened = Some((profile, id, *record)),
                Reply::Imported(report) => {
                    self.notice = format!(
                        "Imported {}, skipped {}, failed {}. {}",
                        report.imported,
                        report.skipped,
                        report.errors.len(),
                        report.errors.join("; ")
                    );
                    self.refresh = true;
                }
                Reply::Cached(key, lines) => {
                    self.notice = if lines.is_some() {
                        "Saved completed FEN-only analysis; isolated to this game history"
                    } else {
                        "No compatible completed analysis"
                    }
                    .into();
                    self.cached = lines.map(|lines| (key, lines));
                }
                Reply::Done(message) => {
                    self.notice = message;
                    self.refresh = true;
                }
                Reply::Error(e) => self.notice = e,
            }
        }
        if let Some(reply) = self.file.as_ref().and_then(|f| f.try_recv().ok()) {
            self.file = None;
            match reply {
                Ok(crate::file_dialog::FileReply::Text(text)) => {
                    self.send(Command::Import(self.active, text));
                }
                Ok(_) => {}
                Err(e) => self.notice = e,
            }
        }
        if self.refresh && self.available() {
            self.refresh = false;
            self.send(Command::Refresh(self.active, self.query.clone()));
        }
    }
    pub fn tick(&mut self, ctx: &egui::Context) {
        if self.ready {
            let name = self
                .profiles
                .iter()
                .find(|p| p.0 == self.active)
                .map_or("", |p| p.1.as_str())
                .to_owned();
            self.sync.tick(ctx, self.active, &name, self.available());
            if self.sync.refresh {
                self.sync.refresh = false;
                self.refresh = true;
            }
            if self.sync.save_preferences && self.available() {
                let p = self.sync.preferences.clone();
                if self.send(Command::SyncPreferences(p)) {
                    self.sync.save_preferences = false;
                }
            }
        }
    }
    pub fn shutdown(&mut self) {
        self.sync.shutdown();
        if let Some(c) = &self.client {
            c.shutdown();
        }
    }
    pub fn stopped(&self) -> bool {
        self.sync.stopped()
            && ((!self.ready && !self.busy)
                || self
                    .client
                    .as_ref()
                    .is_none_or(|c| c.done.load(Ordering::Acquire)))
    }
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("Game Library");
        ui.label(format!(
            "{} · Database and WAL: {} KiB",
            self.notice,
            self.bytes / 1024
        ));
        if !self.ready {
            ui.label("Database unavailable. Existing Game Review remains usable. Check startup path and restart after correcting the error.");
            return;
        }
        self.sync.show(ui, self.available());
        ui.add_enabled_ui(self.available(),|ui|{
            ui.horizontal_wrapped(|ui|{
                let mut selected=None;
                egui::ComboBox::from_id_salt("profile").selected_text(self.profiles.iter().find(|p|p.0==self.active).map_or("Profile",|p|p.1.as_str())).show_ui(ui,|ui|{for (id,name,_) in &self.profiles{if ui.selectable_label(*id==self.active,name).clicked(){selected=Some(name.clone());}}});
                if let Some(name)=selected{self.send(Command::Select(name));}
                ui.text_edit_singleline(&mut self.username);
                if ui.button("Create / select profile").clicked(){self.send(Command::Select(self.username.trim().into()));}
                if ui.button("Delete profile…").clicked(){self.confirm_profile=Some(self.active);}
            });
            ui.label("Switching profiles keeps the current review open. Save local variations using export or Save current game before replacing them.");
            ui.horizontal(|ui|{
                if ui.button("Import PGN file…").clicked(){self.file=Some(crate::file_dialog::library_import(ui.ctx().clone()));}
                if ui.button("Refresh").clicked(){self.refresh=true;}
            });
            egui::CollapsingHeader::new("Paste PGN import").show(ui,|ui|{ui.add(egui::TextEdit::multiline(&mut self.text).desired_rows(4).char_limit(crate::notation::MAX_PGN_BYTES));if ui.button("Import pasted PGN").clicked(){self.send(Command::Import(self.active,self.text.clone()));}});
            let before=self.query.clone();
            ui.horizontal_wrapped(|ui|{
                ui.label("Search players / opening");ui.text_edit_singleline(&mut self.query.search);
                egui::ComboBox::from_id_salt("result_filter").selected_text(if self.query.result.is_empty(){"All results"}else{&self.query.result}).show_ui(ui,|ui|{for result in ["","1-0","0-1","1/2-1/2","*"]{ui.selectable_value(&mut self.query.result,result.into(),if result.is_empty(){"All results"}else{result});}});
                ui.checkbox(&mut self.query.analyzed,"Has cached analysis");ui.checkbox(&mut self.query.oldest,"Oldest first");
                if ui.button("Apply search").clicked(){self.query.offset=0;self.refresh=true;}
            });
            if before.result!=self.query.result||before.analyzed!=self.query.analyzed||before.oldest!=self.query.oldest{self.query.offset=0;self.refresh=true;}
            let mut open=None;let mut delete=None;
            egui::ScrollArea::horizontal().show(ui,|ui|{egui::Grid::new("library_rows").striped(true).show(ui,|ui|{
                for h in ["White","Black","Date","Result","Time control","Opening","Analysis","Actions"]{ui.strong(h);}ui.end_row();
                for row in &self.rows{
                    for value in [&row.white,&row.black,&row.date,&row.result,&row.time,&row.opening]{ui.label(value);}
                    ui.label(if row.analyzed{"Cached positions"}else{"Not analyzed"});
                    ui.horizontal(|ui|{if ui.button("Open").clicked(){open=Some(row.id);}
if ui.button("Delete…").clicked(){delete=Some(row.id);}});ui.end_row();
                }
            });});
            if let Some(id)=open{self.send(Command::Open(self.active,id));}
if delete.is_some(){self.confirm_game=delete;}
            ui.horizontal(|ui|{
                if ui.add_enabled(self.query.offset>0,egui::Button::new("Previous 50")).clicked(){self.query.offset=self.query.offset.saturating_sub(50);self.refresh=true;}
                ui.label(format!("Page {}",self.query.offset/50+1));
                if ui.add_enabled(self.rows.len()==50,egui::Button::new("Next 50")).clicked(){self.query.offset+=50;self.refresh=true;}
            });
            egui::CollapsingHeader::new("Database maintenance").show(ui,|ui|{
                if ui.button("Integrity check / size").clicked(){self.send(Command::Integrity);}
                ui.label("Backup destination (new file only)");ui.text_edit_singleline(&mut self.backup_path);
                if ui.button("Create backup").clicked(){self.send(Command::Backup(PathBuf::from(&self.backup_path)));}
                ui.label("Restore source");ui.text_edit_singleline(&mut self.restore_path);
                ui.label("New safety-backup path (required before restore)");ui.text_edit_singleline(&mut self.safety_path);
                if ui.button("Restore…").clicked(){self.confirm_restore=true;}
                ui.label("Close other Mfalmion instances before restore. The current review remains in memory; reopen the desired stored game afterward.");
                if ui.button("Save preferences").clicked(){self.send(Command::Preferences(self.preferences.clone()));}
            });
        });
        if let Some(id) = self.confirm_game {
            egui::Window::new("Delete selected game?")
                .collapsible(false)
                .show(ui.ctx(), |ui| {
                    ui.label(format!(
                        "Delete game {id} and its analysis/jobs from this profile?"
                    ));
                    if ui.button("Cancel").clicked() {
                        self.confirm_game = None;
                    }
                    if ui
                        .add_enabled(self.available(), egui::Button::new("Confirm delete game"))
                        .clicked()
                    {
                        self.send(Command::Delete(self.active, id, true));
                        self.confirm_game = None;
                    }
                });
        }
        if let Some(id) = self.confirm_profile {
            egui::Window::new("Delete selected profile?").collapsible(false).show(ui.ctx(),|ui|{ui.label(format!("Delete profile {id} and only its games, analysis and jobs? This cannot be undone."));if ui.button("Cancel").clicked(){self.confirm_profile=None;}
if ui.add_enabled(self.available(),egui::Button::new("Confirm delete profile")).clicked(){self.send(Command::DeleteProfile(id,true));self.confirm_profile=None;}});
        }
        if self.confirm_restore {
            egui::Window::new("Replace database from backup?").collapsible(false).show(ui.ctx(),|ui|{ui.label("A new safety backup of the current database is required. Restore replaces all database profiles and records.");if ui.button("Cancel").clicked(){self.confirm_restore=false;}
if ui.add_enabled(self.available(),egui::Button::new("Confirm restore")).clicked(){self.send(Command::Restore(self.restore_path.clone().into(),self.safety_path.clone().into(),true));self.confirm_restore=false;}});
        }
    }
    pub fn cache(&mut self, key: Key, snapshot: crate::engine::protocol::Snapshot) -> bool {
        self.send(Command::Cache(key, snapshot))
    }
    pub fn lookup(&mut self, key: Key) -> bool {
        self.send(Command::Lookup(key))
    }
}
