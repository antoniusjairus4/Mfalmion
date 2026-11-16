pub const VERSION: i64 = 1;
pub const SQL: &str = r#"
CREATE TABLE schema_migrations(version INTEGER PRIMARY KEY, applied_at INTEGER NOT NULL DEFAULT(unixepoch()));
CREATE TABLE profiles(id INTEGER PRIMARY KEY, username TEXT NOT NULL COLLATE NOCASE UNIQUE, display_name TEXT, active INTEGER NOT NULL DEFAULT 0 CHECK(active IN(0,1)), created_at INTEGER NOT NULL DEFAULT(unixepoch()), updated_at INTEGER NOT NULL DEFAULT(unixepoch()), last_sync INTEGER, sync_status TEXT NOT NULL DEFAULT 'never');
CREATE UNIQUE INDEX one_active_profile ON profiles(active) WHERE active=1;
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at INTEGER NOT NULL DEFAULT(unixepoch()));
CREATE TABLE documents(id INTEGER PRIMARY KEY, fingerprint TEXT NOT NULL UNIQUE, pgn TEXT NOT NULL);
CREATE TABLE games(id INTEGER PRIMARY KEY, profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, external_id TEXT, document_id INTEGER REFERENCES documents(id), pgn TEXT NOT NULL, white TEXT NOT NULL, black TEXT NOT NULL, date TEXT NOT NULL, time_control TEXT NOT NULL, time_class TEXT NOT NULL, result TEXT NOT NULL, eco TEXT NOT NULL, opening TEXT NOT NULL, fingerprint TEXT NOT NULL, imported_at INTEGER NOT NULL DEFAULT(unixepoch()), updated_at INTEGER NOT NULL DEFAULT(unixepoch()), UNIQUE(profile_id,fingerprint));
CREATE INDEX games_profile_date ON games(profile_id,date,id);
CREATE TABLE game_moves(game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE, ply INTEGER NOT NULL CHECK(ply>=0), san TEXT, uci TEXT, fen TEXT NOT NULL, clock TEXT, PRIMARY KEY(game_id,ply));
CREATE TABLE analysis(id INTEGER PRIMARY KEY, game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE, ply INTEGER NOT NULL, context TEXT NOT NULL, engine TEXT NOT NULL, configuration TEXT NOT NULL, depth INTEGER NOT NULL, multipv INTEGER NOT NULL, lines TEXT NOT NULL, status TEXT NOT NULL CHECK(status='completed'), created_at INTEGER NOT NULL DEFAULT(unixepoch()), UNIQUE(game_id,ply,context,engine,configuration,depth,multipv));
CREATE TABLE jobs(id INTEGER PRIMARY KEY, profile_id INTEGER NOT NULL REFERENCES profiles(id) ON DELETE CASCADE, game_id INTEGER REFERENCES games(id) ON DELETE CASCADE, kind TEXT NOT NULL, work_key TEXT NOT NULL, status TEXT NOT NULL CHECK(status IN('pending','running','paused','completed','failed','cancelled')), total INTEGER NOT NULL CHECK(total>=0), completed INTEGER NOT NULL DEFAULT 0 CHECK(completed>=0 AND completed<=total), last_position INTEGER, created_at INTEGER NOT NULL DEFAULT(unixepoch()), started_at INTEGER, completed_at INTEGER, last_error TEXT, retries INTEGER NOT NULL DEFAULT 0);
CREATE UNIQUE INDEX active_job ON jobs(profile_id,work_key) WHERE status IN('pending','running','paused');
CREATE INDEX jobs_profile ON jobs(profile_id,status);
"#;
