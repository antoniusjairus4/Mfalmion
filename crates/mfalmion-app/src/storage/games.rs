use super::*;
use crate::notation::{self, GameRecord};
use shakmaty::{CastlingMode, uci::UciMove};
#[derive(Default, Debug)]
pub struct ImportReport {
    pub imported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub struct Query {
    pub search: String,
    pub result: String,
    pub analyzed: bool,
    pub oldest: bool,
    pub offset: usize,
}
#[derive(Debug)]
pub struct GameRow {
    pub id: i64,
    pub white: String,
    pub black: String,
    pub date: String,
    pub result: String,
    pub time: String,
    pub opening: String,
    pub analyzed: bool,
    pub batch_status: String,
}
pub fn fingerprint(record: &GameRecord) -> String {
    digest(&record.pgn())
}
/// Split only at top-level termination markers, never inside tags/comments/RAV.
/// Broken delimiters cannot be safely resynchronized and fail the remaining fragment.
pub fn fragments(text: &str) -> Vec<&str> {
    let b = text.as_bytes();
    let (mut i, mut start, mut rav, mut brace, mut tag, mut quote, mut line) =
        (0, 0, 0usize, false, false, false, false);
    let mut parts = Vec::new();
    while i < b.len() {
        let c = b[i];
        if line {
            if c == b'\n' {
                line = false;
            }
            i += 1;
            continue;
        }
        if brace {
            if c == b'}' {
                brace = false;
            }
            i += 1;
            continue;
        }
        if tag {
            if c == b'\\' && quote {
                i = (i + 2).min(b.len());
                continue;
            }
            if c == b'"' {
                quote = !quote;
            }
            if c == b']' && !quote {
                tag = false;
            }
            i += 1;
            continue;
        }
        match c {
            b';' | b'%' => {
                line = true;
                i += 1;
            }
            b'{' => {
                brace = true;
                i += 1;
            }
            b'[' => {
                tag = true;
                i += 1;
            }
            b'(' => {
                rav += 1;
                i += 1;
            }
            b')' => {
                rav = rav.saturating_sub(1);
                i += 1;
            }
            c if c.is_ascii_whitespace() => i += 1,
            _ => {
                let from = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && !b"{}[]();".contains(&b[i]) {
                    i += 1;
                }
                if from == i {
                    i += 1;
                    continue;
                }
                let token = &text[from..i];
                if rav == 0 && matches!(token, "*" | "1-0" | "0-1" | "1/2-1/2") {
                    parts.push(&text[start..i]);
                    if parts.len() > 10_000 {
                        return parts;
                    }
                    start = i;
                }
            }
        }
    }
    if !text[start..].trim().is_empty() {
        let tail = &text[start..];
        if !parts.is_empty() && annotations_only(tail) {
            let previous = parts.last().map_or(start, |p| start - p.len());
            if let Some(last) = parts.last_mut() {
                *last = &text[previous..];
            }
        } else {
            parts.push(tail);
        }
    }
    parts
}
fn annotations_only(text: &str) -> bool {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {}
            ';' | '%' => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            '{' => {
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return false;
                }
            }
            _ => return false,
        }
    }
    true
}
impl Store {
    pub fn import_pgn(&mut self, profile: i64, text: &str) -> Result<ImportReport> {
        if text.len() > notation::MAX_PGN_BYTES {
            return Err("PGN exceeds 16 MiB".into());
        }
        let parts = fragments(text);
        if parts.len() > 10_000 {
            return Err("Import exceeds 10,000 games; split the file into smaller batches".into());
        }
        let mut report = ImportReport::default();
        let mut positions = 0;
        for (index, part) in parts.into_iter().enumerate() {
            match notation::parse_pgn(part) {
                Ok(records) => {
                    for record in records {
                        positions += record.positions.len();
                        if positions > 200_000 {
                            report.errors.push(format!("Game {}: 200,000-position import budget exceeded; this game was not inserted",index+1));
                            continue;
                        }
                        match self.insert_game(profile, &record, Some(part)) {
                            Ok(true) => report.imported += 1,
                            Ok(false) => report.skipped += 1,
                            Err(e) => report.errors.push(format!("Game {}: {e}", index + 1)),
                        }
                    }
                }
                Err(e) => report.errors.push(format!("Game {}: {e}", index + 1)),
            }
        }
        if text.trim().is_empty() {
            report.errors.push("No games found".into());
        }
        Ok(report)
    }
    pub fn insert_game(
        &mut self,
        profile: i64,
        record: &GameRecord,
        original: Option<&str>,
    ) -> Result<bool> {
        if record.positions.is_empty()
            || record.positions.len() != record.moves.len() + 1
            || record.moves.len() != record.played_moves.len()
        {
            return Err("Invalid game timeline shape".into());
        }
        let canonical = record.pgn();
        // Validate public record invariants before serializing, not merely trust callers.
        let parsed = notation::parse_pgn(&canonical)?;
        let record = &parsed[0];
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        let (inserted, _) = insert_validated(&tx, profile, record, original, None)?;
        tx.commit().map_err(|e| e.to_string())?;
        Ok(inserted)
    }
    pub fn list_games(&self, profile: i64, q: &Query) -> Result<Vec<GameRow>> {
        let mut s=self.db.prepare("SELECT g.id,white,black,date,result,time_control,opening,EXISTS(SELECT 1 FROM analysis a WHERE a.game_id=g.id) ,coalesce((SELECT 'Batch '||b.status FROM batch_runs b WHERE b.game_id=g.id ORDER BY b.id DESC LIMIT 1),'Not analyzed') FROM games g WHERE profile_id=?1 AND (?2='' OR instr(lower(white||' '||black||' '||opening),lower(?2))>0) AND (?3='' OR result=?3) AND (?4=0 OR EXISTS(SELECT 1 FROM analysis a WHERE a.game_id=g.id)) ORDER BY CASE WHEN ?5=1 THEN date END ASC, CASE WHEN ?5=0 THEN date END DESC,id DESC LIMIT 50 OFFSET ?6").map_err(|e|e.to_string())?;
        s.query_map(
            params![
                profile,
                q.search,
                q.result,
                q.analyzed,
                q.oldest,
                q.offset.min(i64::MAX as usize) as i64
            ],
            |r| {
                Ok(GameRow {
                    id: r.get(0)?,
                    white: r.get(1)?,
                    black: r.get(2)?,
                    date: r.get(3)?,
                    result: r.get(4)?,
                    time: r.get(5)?,
                    opening: r.get(6)?,
                    analyzed: r.get(7)?,
                    batch_status: r.get(8)?,
                })
            },
        )
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())
    }
    pub fn load_game(&self, profile: i64, id: i64) -> Result<GameRecord> {
        let (pgn,original):(String,Option<String>)=self.db.query_row("SELECT g.pgn,d.pgn FROM games g LEFT JOIN documents d ON d.id=g.document_id WHERE g.profile_id=?1 AND g.id=?2",params![profile,id],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|e|e.to_string())?;
        let mut records = notation::parse_pgn(&pgn)?;
        let mut record = records.remove(0);
        let rows: Vec<(i64, Option<String>, Option<String>, String)> = self
            .db
            .prepare("SELECT ply,san,uci,fen FROM game_moves WHERE game_id=?1 ORDER BY ply")
            .and_then(|mut s| {
                s.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                    .collect()
            })
            .map_err(|e| e.to_string())?;
        if rows.len() != record.positions.len() {
            return Err("Stored timeline length is inconsistent".into());
        }
        for (i, (ply, san, uci, fen)) in rows.iter().enumerate() {
            let expected = i.checked_sub(1).map(|n| {
                UciMove::from_move(record.played_moves[n], CastlingMode::Standard).to_string()
            });
            if *ply != i as i64
                || *fen != notation::fen(&record.positions[i])
                || san.as_ref() != i.checked_sub(1).map(|n| &record.moves[n])
                || *uci != expected
            {
                return Err("Stored timeline failed validation".into());
            }
        }
        record.original_pgn = original.map(Into::into);
        Ok(record)
    }
    pub fn delete_game(&mut self, profile: i64, id: i64, confirmed: bool) -> Result<()> {
        if !confirmed {
            return Err("Explicit game deletion confirmation required".into());
        }
        let tx = self.db.transaction().map_err(|e| e.to_string())?;
        tx.execute(
            "DELETE FROM games WHERE profile_id=?1 AND id=?2",
            params![profile, id],
        )
        .map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM documents WHERE id NOT IN(SELECT document_id FROM games WHERE document_id IS NOT NULL)",[]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())
    }
}

pub(crate) fn insert_validated(
    tx: &rusqlite::Transaction<'_>,
    profile: i64,
    record: &GameRecord,
    original: Option<&str>,
    external_identity: Option<&str>,
) -> Result<(bool, i64)> {
    let canonical = record.pgn();
    let fingerprint = external_identity.map_or_else(
        || digest(&canonical),
        |id| digest(&format!("{canonical}\nexternal:{id}")),
    );
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM games WHERE profile_id=?1 AND fingerprint=?2",
            params![profile, fingerprint],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some(id) = existing {
        return Ok((false, id));
    }
    let mut document = None;
    if let Some(original) = original {
        let hash = digest(original);
        tx.execute("INSERT INTO documents(fingerprint,pgn) VALUES(?1,?2) ON CONFLICT(fingerprint) DO NOTHING",params![hash,original]).map_err(|e|e.to_string())?;
        document = Some(
            tx.query_row(
                "SELECT id FROM documents WHERE fingerprint=?1",
                [hash],
                |r| r.get::<_, i64>(0),
            )
            .map_err(|e| e.to_string())?,
        );
    }
    let tag = |key: &str| record.headers.get(key).map_or("", String::as_str);
    tx.execute("INSERT INTO games(profile_id,external_id,document_id,pgn,white,black,date,time_control,time_class,result,eco,opening,fingerprint) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",params![profile,tag("Link"),document,canonical,tag("White"),tag("Black"),tag("Date"),tag("TimeControl"),tag("TimeClass"),record.result,tag("ECO"),tag("Opening"),fingerprint]).map_err(|e|e.to_string())?;
    let id = tx.last_insert_rowid();
    {
        let mut statement = tx
            .prepare("INSERT INTO game_moves(game_id,ply,san,uci,fen) VALUES(?1,?2,?3,?4,?5)")
            .map_err(|e| e.to_string())?;
        for (ply, position) in record.positions.iter().enumerate() {
            let san = ply.checked_sub(1).and_then(|i| record.moves.get(i));
            let uci = ply
                .checked_sub(1)
                .and_then(|i| record.played_moves.get(i))
                .map(|m| UciMove::from_move(*m, CastlingMode::Standard).to_string());
            statement
                .execute(params![id, ply as i64, san, uci, notation::fen(position)])
                .map_err(|e| e.to_string())?;
        }
    }
    Ok((true, id))
}
