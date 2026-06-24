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
    #[error(
        "cache schema migration failed: {0}. Reset the database and cache from Settings, then reconnect to Jellyfin."
    )]
    Migration(String),
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
        let user_version = self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))?;
        if user_version > schema::SCHEMA_VERSION {
            return Err(CacheError::Migration(format!(
                "database version {user_version} is newer than supported version {}",
                schema::SCHEMA_VERSION
            )));
        }

        self.connection
            .execute_batch(schema::V1)
            .map_err(|error| CacheError::Migration(error.to_string()))?;
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

    pub fn save_discord_artwork_url(
        &self,
        artwork_hash: &str,
        url: &str,
    ) -> Result<(), CacheError> {
        self.set_setting(&discord_artwork_setting_key(artwork_hash), url)
    }

    pub fn load_discord_artwork_urls(&self) -> Result<Vec<(String, String)>, CacheError> {
        let mut statement = self
            .connection
            .prepare("SELECT key, value FROM app_settings WHERE key LIKE 'discord.artwork.%'")?;
        let rows = statement.query_map([], |row| {
            let key: String = row.get(0)?;
            let value: String = row.get(1)?;
            Ok((key, value))
        })?;

        let mut urls = Vec::new();
        for row in rows {
            let (key, value) = row?;
            if let Some(hash) = key.strip_prefix("discord.artwork.") {
                urls.push((hash.to_string(), value));
            }
        }
        Ok(urls)
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

fn discord_artwork_setting_key(artwork_hash: &str) -> String {
    format!("discord.artwork.{artwork_hash}")
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn settings_persist_and_update_in_memory() {
        let database = CacheDatabase::open_memory().expect("in-memory cache opens");

        assert_eq!(
            database.get_setting("library.sort").expect("read setting"),
            None
        );
        database
            .set_setting("library.sort", "title")
            .expect("write setting");
        database
            .set_setting("library.sort", "artist")
            .expect("update setting");

        assert_eq!(
            database.get_setting("library.sort").expect("read setting"),
            Some("artist".to_string())
        );
    }

    #[test]
    fn session_round_trips_through_settings() {
        let database = CacheDatabase::open_memory().expect("in-memory cache opens");
        let session = JellyfinSession {
            server_url: "https://jellyfin.example/".to_string(),
            server_id: Some("server".to_string()),
            user_id: "user".to_string(),
            username: "eddie".to_string(),
            access_token: "token".to_string(),
        };

        database
            .save_jellyfin_session(&session)
            .expect("save session");

        let loaded = database
            .load_jellyfin_session()
            .expect("load session")
            .expect("session exists");

        assert_eq!(loaded.server_url, session.server_url);
        assert_eq!(loaded.server_id, session.server_id);
        assert_eq!(loaded.user_id, session.user_id);
        assert_eq!(loaded.username, session.username);
        assert_eq!(loaded.access_token, session.access_token);
    }

    #[test]
    fn discord_artwork_urls_round_trip_through_settings() {
        let database = CacheDatabase::open_memory().expect("in-memory cache opens");

        database
            .save_discord_artwork_url("abc123", "https://img.fvvs.me/abc123.jpg")
            .expect("save discord artwork URL");

        assert_eq!(
            database.load_discord_artwork_urls().expect("load URLs"),
            vec![(
                "abc123".to_string(),
                "https://img.fvvs.me/abc123.jpg".to_string()
            )]
        );
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
