use std::sync::Arc;

use rusqlite::Connection;
use tokio::sync::Mutex;

pub type SharedDb = Arc<Mutex<Connection>>;

/// Open (or create) the SQLite development database and apply the catalog schema.
pub fn open_sqlite(path: &str) -> anyhow::Result<SharedDb> {
    if let Some(parent) = std::path::Path::new(path).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let schema = include_str!("../../docs/sql/sqlite/001_ecshop_catalog.sql");
    conn.execute_batch(schema)?;
    Ok(Arc::new(Mutex::new(conn)))
}
