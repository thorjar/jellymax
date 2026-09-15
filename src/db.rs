use crate::error::{Error, Result};
use rusqlite::Connection;
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

#[derive(Clone)]
pub struct Database(Arc<Mutex<Connection>>);

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 7 {
            return Err(Error::internal(
                "Database was created by a newer server version",
            ));
        }
        connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
        let tx = connection.transaction()?;
        if version > 0 {
            migrate_items_metadata(&tx)?;
            migrate_items_parent(&tx)?;
            migrate_remote_columns(&tx)?;
            migrate_remote_device(&tx)?;
            let object_store_column: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('libraries') WHERE name='object_store_id')",
                [], |row| row.get(0),
            )?;
            if !object_store_column {
                tx.execute(
                    "ALTER TABLE libraries ADD COLUMN object_store_id TEXT REFERENCES object_stores(id) ON DELETE CASCADE",
                    [],
                )?;
            }
            for column in ["index_number", "parent_index_number"] {
                let exists: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM pragma_table_info('items') WHERE name=?1)",
                    [column],
                    |r| r.get(0),
                )?;
                if !exists {
                    tx.execute(
                        &format!("ALTER TABLE items ADD COLUMN {column} INTEGER"),
                        [],
                    )?;
                }
            }
        }
        tx.execute_batch(include_str!("schema.sql"))?;
        tx.commit()?;
        connection.execute(
            "INSERT OR IGNORE INTO settings(key,value) VALUES ('server_id',?1)",
            [uuid::Uuid::new_v4().simple().to_string()],
        )?;
        Ok(Self(Arc::new(Mutex::new(connection))))
    }

    // SQLite and password/filesystem work never run on Tokio's async worker threads.
    pub async fn call<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        let db = self.0.clone();
        tokio::task::spawn_blocking(move || {
            let mut connection = db.lock().map_err(Error::internal)?;
            operation(&mut connection)
        })
        .await
        .map_err(Error::internal)?
    }
}

fn migrate_remote_device(connection: &Connection) -> Result<()> {
    let table_exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='remote_servers')",
        [],
        |r| r.get(0),
    )?;
    if !table_exists {
        return Ok(());
    }
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('remote_servers') WHERE name='device_id')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        connection.execute("ALTER TABLE remote_servers ADD COLUMN device_id TEXT", [])?;
        connection.execute(
            "UPDATE remote_servers SET device_id='jellymax-'||id WHERE device_id IS NULL",
            [],
        )?;
    }
    Ok(())
}

fn migrate_remote_columns(connection: &Connection) -> Result<()> {
    for (table, column) in [
        ("libraries", "remote_server_id"),
        ("libraries", "remote_item_id"),
        ("items", "remote_server_id"),
        ("items", "remote_item_id"),
    ] {
        let exists: bool = connection.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name=?1)"),
            [column],
            |r| r.get(0),
        )?;
        if !exists {
            connection.execute(&format!("ALTER TABLE {table} ADD COLUMN {column} TEXT"), [])?;
        }
    }
    Ok(())
}

fn migrate_items_parent(connection: &Connection) -> Result<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM pragma_table_info('items') WHERE name='parent_id')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        // Nullable foreign keys can be added without rewriting existing rows.
        connection.execute(
            "ALTER TABLE items ADD COLUMN parent_id TEXT REFERENCES items(id) ON DELETE CASCADE",
            [],
        )?;
        connection.execute(
            "CREATE INDEX IF NOT EXISTS items_parent ON items(parent_id)",
            [],
        )?;
    }
    Ok(())
}

fn migrate_items_metadata(connection: &Connection) -> Result<()> {
    let mut columns = connection.prepare("PRAGMA table_info(items)")?;
    let existing: Vec<String> = columns
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (column, ddl) in [
        ("tmdb_id", "TEXT"),
        ("year", "INTEGER"),
        ("overview", "TEXT"),
        ("genres", "TEXT NOT NULL DEFAULT '[]'"),
        ("rating", "REAL"),
    ] {
        if !existing.iter().any(|existing| existing == column) {
            connection.execute(&format!("ALTER TABLE items ADD COLUMN {column} {ddl}"), [])?;
        }
    }
    Ok(())
}
