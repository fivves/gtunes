pub const SCHEMA_VERSION: i32 = 1;

pub const V1: &str = r#"
CREATE TABLE IF NOT EXISTS app_settings (
  key TEXT PRIMARY KEY NOT NULL,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS jellyfin_servers (
  id TEXT PRIMARY KEY NOT NULL,
  base_url TEXT NOT NULL,
  display_name TEXT,
  last_seen_at TEXT
);

CREATE TABLE IF NOT EXISTS media_items (
  id TEXT PRIMARY KEY NOT NULL,
  item_type TEXT NOT NULL,
  name TEXT NOT NULL,
  album_id TEXT,
  artist_ids TEXT,
  playlist_ids TEXT,
  runtime_ticks INTEGER,
  container TEXT,
  json TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS artwork_cache (
  item_id TEXT NOT NULL,
  image_kind TEXT NOT NULL,
  image_tag TEXT,
  local_path TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (item_id, image_kind)
);

CREATE TABLE IF NOT EXISTS waveform_cache (
  item_id TEXT NOT NULL,
  media_source_id TEXT NOT NULL,
  sample_count INTEGER NOT NULL,
  local_path TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  PRIMARY KEY (item_id, media_source_id)
);

CREATE TABLE IF NOT EXISTS queue_state (
  position INTEGER PRIMARY KEY NOT NULL,
  item_id TEXT NOT NULL,
  source TEXT NOT NULL,
  added_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_media_items_type_name ON media_items(item_type, name);
"#;
