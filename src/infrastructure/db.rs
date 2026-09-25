//! rbatis pool bootstrap: supports SQLite (dev) and MySQL (production) through one API.

use once_cell::sync::OnceCell;
use rbatis::rbatis::RBatis;
use rbdc_mysql::driver::MysqlDriver;
use rbdc_sqlite::driver::SqliteDriver;

static RB: OnceCell<RBatis> = OnceCell::new();
static INIT: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

/// Global pool reference. Panics only if called before `init`.
pub fn rb() -> &'static RBatis {
    RB.get().expect("rbatis is not initialized")
}

/// Begin a transaction that rolls back automatically when the guard is dropped
/// without commit (error paths included). Without this, an early `?` return
/// would hand a connection with an open transaction back to the pool and poison it.
pub async fn begin(rb: &RBatis) -> Result<rbatis::executor::RBatisTxExecutorGuard, rbatis::Error> {
    let tx = rb.acquire_begin().await?;
    Ok(tx.defer_async(|tx| async move {
        if !tx.done() {
            let _ = tx.rollback().await;
        }
    }))
}

/// Initialize the pool from a database URL and bootstrap the schema.
/// Supported URLs: `sqlite://data/ecshop.sqlite3`, `mysql://user:pass@host/db`.
pub async fn init(database_url: &str) -> Result<&'static RBatis, String> {
    INIT.get_or_try_init(|| async {
        let rb = RBatis::new();
        if database_url.starts_with("sqlite://") {
            // rbdc-sqlite creates the DB file but not its parent directories.
            if let Some(parent) = database_url
                .trim_start_matches("sqlite://")
                .split('?')
                .next()
                .map(std::path::Path::new)
                .and_then(|p| p.parent())
                .filter(|p| !p.as_os_str().is_empty())
            {
                let _ = std::fs::create_dir_all(parent);
            }
            rb.init(SqliteDriver {}, database_url)
                .map_err(|e| format!("init sqlite pool failed: {e}"))?;
            bootstrap_sqlite_schema(&rb)
                .await
                .map_err(|e| format!("bootstrap sqlite schema failed: {e}"))?;
        } else if database_url.starts_with("mysql://") {
            rb.init(MysqlDriver {}, database_url)
                .map_err(|e| format!("init mysql pool failed: {e}"))?;
            tracing::warn!("mysql: expect ecs_* schema to be provisioned by DBA/migration tooling");
        } else {
            return Err(format!("unsupported database url: {database_url}"));
        }
        RB.set(rb)
            .map_err(|_| "rbatis already initialized".to_string())
    })
    .await
    .map_err(|e| e.clone())?;
    Ok(RB.get().expect("initialized above"))
}

async fn bootstrap_sqlite_schema(rb: &RBatis) -> Result<(), rbatis::Error> {
    let schema = include_str!("../../docs/sql/sqlite/001_ecshop_catalog.sql");
    for stmt in split_statements(schema) {
        rb.exec(&stmt, vec![]).await?;
    }
    let compat = include_str!("../../docs/sql/sqlite/002_compat.sql");
    for stmt in split_statements(compat) {
        rb.exec(&stmt, vec![]).await?;
    }
    // The dev schema lacks a few columns the API surface expects; add them
    // idempotently (ALTER TABLE errors are ignored when the column exists).
    for stmt in [
        "ALTER TABLE ecs_users ADD COLUMN mobile_phone VARCHAR(32) NOT NULL DEFAULT ''",
        "ALTER TABLE ecs_users ADD COLUMN is_validated INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE ecs_users ADD COLUMN pay_points INTEGER NOT NULL DEFAULT 0",
    ] {
        if let Err(e) = rb.exec(stmt, vec![]).await {
            tracing::debug!("schema bootstrap (expected when column exists): {e}");
        }
    }
    Ok(())
}

/// Quote-aware SQL statement splitter: splits on `;` outside string literals,
/// drops `--` line comments, and strips transaction wrappers / PRAGMA lines
/// (each statement runs through the pool directly).
fn split_statements(sql: &str) -> Vec<String> {
    let mut statements: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut in_quote = false;
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            // SQLite escapes quotes by doubling: '' toggles twice => still inside.
            in_quote = !in_quote;
            current.push(c);
        } else if c == ';' && !in_quote {
            statements.push(current.clone());
            current.clear();
        } else if c == '-' && !in_quote && chars.peek() == Some(&'-') {
            // Line comment: skip up to and including the newline.
            for n in chars.by_ref() {
                if n == '\n' {
                    break;
                }
            }
        } else {
            current.push(c);
        }
    }
    statements.push(current);
    statements
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| {
            let upper = s.to_ascii_uppercase();
            upper != "BEGIN" && upper != "COMMIT" && !upper.starts_with("PRAGMA")
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_statements_strips_wrappers_and_comments() {
        let sql = "-- comment\nPRAGMA foreign_keys = ON;\nBEGIN;\nCREATE TABLE a (id INTEGER);\nINSERT INTO a VALUES (1);\nCOMMIT;\n";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].starts_with("CREATE TABLE a"));
        assert!(stmts[1].starts_with("INSERT INTO a"));
    }

    #[test]
    fn split_statements_keeps_multi_line_statements_and_quoted_semicolons() {
        let sql = "CREATE TABLE t (\n  id INTEGER,\n  note TEXT\n);\nINSERT INTO t VALUES (1, 'a;b');\n";
        let stmts = split_statements(sql);
        assert_eq!(stmts.len(), 2);
        assert!(stmts[0].contains("note TEXT"));
        assert!(stmts[1].contains("'a;b'"), "quoted semicolon must not split: {stmts:?}");
    }
}
