use super::*;
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default)]
pub struct Preferences {
    pub engine: crate::engine::Settings,
    pub sync_interval_hours: u32,
    pub database_reference: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            engine: Default::default(),
            sync_interval_hours: 24,
            database_reference:
                "startup environment: MFALMION_DATABASE / MFALMION_DATA_DIR / XDG_DATA_HOME".into(),
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<()> {
        self.engine.validate()?;
        if !(1..=8760).contains(&self.sync_interval_hours) || self.database_reference.len() > 4096 {
            return Err("Invalid preferences".into());
        }
        Ok(())
    }
}
impl Store {
    pub fn preferences(&self) -> Result<(Preferences, Option<String>)> {
        let value: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='preferences'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let Some(value) = value else {
            return Ok((Preferences::default(), None));
        };
        match serde_json::from_str::<Preferences>(&value) {
            Ok(p) if p.validate().is_ok() => Ok((p, None)),
            _ => Ok((
                Preferences::default(),
                Some("Invalid saved settings; defaults loaded, stored value retained".into()),
            )),
        }
    }
    pub fn save_preferences(&self, p: &Preferences) -> Result<()> {
        p.validate()?;
        let value = serde_json::to_string(p).map_err(|e| e.to_string())?;
        self.db.execute("INSERT INTO settings(key,value) VALUES('preferences',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=unixepoch()",[value]).map_err(|e|e.to_string())?;
        Ok(())
    }
}
impl Store {
    pub fn sync_preferences(&self) -> Result<(crate::sync::Preferences, Option<String>)> {
        let value: Option<String> = self
            .db
            .query_row(
                "SELECT value FROM settings WHERE key='chesscom_preferences'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        match value {
            None => Ok((Default::default(), None)),
            Some(value) => match serde_json::from_str::<crate::sync::Preferences>(&value) {
                Ok(p) if p.validate().is_ok() => Ok((p, None)),
                _ => Ok((
                    Default::default(),
                    Some("Invalid saved sync preferences; defaults loaded".into()),
                )),
            },
        }
    }
    pub fn save_sync_preferences(&self, p: &crate::sync::Preferences) -> Result<()> {
        p.validate()?;
        self.db.execute("INSERT INTO settings(key,value) VALUES('chesscom_preferences',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=unixepoch()",[serde_json::to_string(p).map_err(|e|e.to_string())?]).map_err(|e|e.to_string())?;
        Ok(())
    }
}
