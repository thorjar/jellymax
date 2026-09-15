CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS users(
    id TEXT PRIMARY KEY, name TEXT NOT NULL COLLATE NOCASE UNIQUE,
    password_hash TEXT NOT NULL, is_admin INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS sessions(
    token_hash TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_expiry ON sessions(expires_at);
CREATE TABLE IF NOT EXISTS object_stores(
    id TEXT PRIMARY KEY, name TEXT NOT NULL, endpoint TEXT, region TEXT NOT NULL,
    bucket TEXT NOT NULL, prefix TEXT NOT NULL DEFAULT '',
    access_key_id TEXT NOT NULL, secret_access_key TEXT NOT NULL, session_token TEXT
);
CREATE TABLE IF NOT EXISTS libraries(
    id TEXT PRIMARY KEY, name TEXT NOT NULL, path TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK(kind IN ('movies','tvshows','music','homevideos')),
    root_identity TEXT, remote_server_id TEXT, remote_item_id TEXT,
    object_store_id TEXT REFERENCES object_stores(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS remote_servers(
    id TEXT PRIMARY KEY, name TEXT NOT NULL, base_url TEXT NOT NULL UNIQUE,
    server_id TEXT NOT NULL, user_id TEXT NOT NULL, access_token TEXT NOT NULL,
    device_id TEXT NOT NULL, last_sync INTEGER, last_error TEXT
);
CREATE TABLE IF NOT EXISTS items(
    id TEXT PRIMARY KEY, library_id TEXT NOT NULL REFERENCES libraries(id) ON DELETE CASCADE,
    path TEXT NOT NULL UNIQUE, name TEXT NOT NULL, kind TEXT NOT NULL,
    container TEXT NOT NULL, size INTEGER NOT NULL, modified INTEGER NOT NULL,
    runtime_ticks INTEGER, media_streams TEXT NOT NULL DEFAULT '[]',
    tmdb_id TEXT, year INTEGER, overview TEXT, genres TEXT NOT NULL DEFAULT '[]', rating REAL,
    index_number INTEGER, parent_index_number INTEGER,
    parent_id TEXT REFERENCES items(id) ON DELETE CASCADE, scan_id TEXT NOT NULL,
    remote_server_id TEXT, remote_item_id TEXT
);
CREATE INDEX IF NOT EXISTS items_library_name ON items(library_id,name COLLATE NOCASE,id);
CREATE INDEX IF NOT EXISTS items_parent ON items(parent_id);

CREATE TABLE IF NOT EXISTS playback_tickets(
    ticket_hash TEXT PRIMARY KEY,
    session_hash TEXT NOT NULL REFERENCES sessions(token_hash) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL, created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS playback_tickets_session ON playback_tickets(session_hash,created_at);
CREATE INDEX IF NOT EXISTS playback_tickets_expiry ON playback_tickets(expires_at);
CREATE TABLE IF NOT EXISTS user_data(
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    position_ticks INTEGER NOT NULL DEFAULT 0 CHECK(position_ticks >= 0),
    played INTEGER NOT NULL DEFAULT 0, favorite INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(user_id,item_id)
);
CREATE TABLE IF NOT EXISTS playlists(
    id TEXT PRIMARY KEY, user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS playlist_items(
    playlist_id TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES items(id) ON DELETE CASCADE,
    position INTEGER NOT NULL, PRIMARY KEY(playlist_id,position)
);
CREATE INDEX IF NOT EXISTS user_data_item ON user_data(item_id);
CREATE INDEX IF NOT EXISTS playlist_items_item ON playlist_items(item_id);
CREATE INDEX IF NOT EXISTS playback_tickets_item ON playback_tickets(item_id);
CREATE INDEX IF NOT EXISTS sessions_user_expiry ON sessions(user_id,expires_at);
PRAGMA user_version = 7;
