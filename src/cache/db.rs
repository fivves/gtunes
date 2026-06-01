use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config;

use super::schema;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("project directory is unavailable")]
    ProjectDirectoryUnavailable,
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

pub struct CacheDatabase {
    connection: Connection,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JellyfinSession {
    pub server_url: String,
    pub server_id: Option<String>,
    pub user_id: String,
    pub username: String,
    pub access_token: String,
}

impl CacheDatabase {
    pub fn reset_default_cache() -> Result<(), CacheError> {
        let project_dirs = ProjectDirs::from("dev", config::DEVELOPER_NAME, config::APP_NAME)
            .ok_or(CacheError::ProjectDirectoryUnavailable)?;
        remove_dir_if_exists(project_dirs.data_dir())?;
        remove_dir_if_exists(project_dirs.cache_dir())?;
        remove_temp_artwork_cache()?;
        Ok(())
    }

    pub fn open_default() -> Result<Self, CacheError> {
        let project_dirs = ProjectDirs::from("dev", config::DEVELOPER_NAME, config::APP_NAME)
            .ok_or(CacheError::ProjectDirectoryUnavailable)?;
        std::fs::create_dir_all(project_dirs.data_dir())?;
        Self::open(project_dirs.data_dir().join("gtunes.sqlite3"))
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, CacheError> {
        let connection = Connection::open(path)?;
        let database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    pub fn open_memory() -> Result<Self, CacheError> {
        let connection = Connection::open_in_memory()?;
        let database = Self { connection };
        database.migrate()?;
        Ok(database)
    }

    pub fn migrate(&self) -> Result<(), CacheError> {
        self.connection.execute_batch(schema::V1)?;
        self.connection
            .pragma_update(None, "user_version", schema::SCHEMA_VERSION)?;
        Ok(())
    }

    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    pub fn save_jellyfin_session(&self, session: &JellyfinSession) -> Result<(), CacheError> {
        let json = serde_json::to_string(session)?;
        self.set_setting("jellyfin.session", &json)
    }

    pub fn load_jellyfin_session(&self) -> Result<Option<JellyfinSession>, CacheError> {
        self.get_setting("jellyfin.session")?
            .map(|json| serde_json::from_str(&json).map_err(CacheError::from))
            .transpose()
    }

    pub fn clear_jellyfin_session(&self) -> Result<(), CacheError> {
        self.connection.execute(
            "DELETE FROM app_settings WHERE key = ?1",
            ["jellyfin.session"],
        )?;
        Ok(())
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), CacheError> {
        self.connection.execute(
            "INSERT INTO app_settings (key, value, updated_at)
             VALUES (?1, ?2, CURRENT_TIMESTAMP)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
            (key, value),
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, CacheError> {
        let mut statement = self
            .connection
            .prepare("SELECT value FROM app_settings WHERE key = ?1")?;
        let mut rows = statement.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn save_waveform_cache(
        &self,
        item_id: &str,
        media_source_id: &str,
        sample_count: usize,
        local_path: &Path,
    ) -> Result<(), CacheError> {
        self.connection.execute(
            "INSERT INTO waveform_cache (item_id, media_source_id, sample_count, local_path, updated_at)
             VALUES (?1, ?2, ?3, ?4, CURRENT_TIMESTAMP)
             ON CONFLICT(item_id, media_source_id) DO UPDATE SET
               sample_count = excluded.sample_count,
               local_path = excluded.local_path,
               updated_at = CURRENT_TIMESTAMP",
            (
                item_id,
                media_source_id,
                sample_count as i64,
                local_path.to_string_lossy().as_ref(),
            ),
        )?;
        Ok(())
    }

    pub fn waveform_cache_path(
        &self,
        item_id: &str,
        media_source_id: &str,
    ) -> Result<Option<PathBuf>, CacheError> {
        let mut statement = self.connection.prepare(
            "SELECT local_path FROM waveform_cache WHERE item_id = ?1 AND media_source_id = ?2",
        )?;
        let mut rows = statement.query((item_id, media_source_id))?;
        if let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            Ok(Some(PathBuf::from(path)))
        } else {
            Ok(None)
        }
    }
}

fn remove_dir_if_exists(path: &Path) -> Result<(), CacheError> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CacheError::Io(error)),
    }
}

fn remove_temp_artwork_cache() -> Result<(), CacheError> {
    for entry in std::fs::read_dir(std::env::temp_dir())? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("gtunes-artwork-") {
            match std::fs::remove_file(entry.path()) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(CacheError::Io(error)),
            }
        }
    }
    Ok(())
}
