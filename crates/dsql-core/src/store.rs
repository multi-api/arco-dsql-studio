//! Local app data in `studio.sqlite`: saved connections, query history and settings. Never any secret.

use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConnection {
    pub id: Option<i64>,
    pub name: String,
    pub profile: String,
    pub region: String,
    pub cluster_id: String,
    pub host: String,
    /// `"admin"` or a custom role name.
    pub role: String,
    pub schema: Option<String>,
    pub production: bool,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: Option<i64>,
    pub connection_id: Option<i64>,
    pub sql: String,
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    pub rows: Option<u64>,
    pub error: Option<String>,
}

pub struct Store {
    db: Connection,
}

/// Schema migrations, applied in order; `PRAGMA user_version` holds how many have run.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE connections (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL,
        profile TEXT NOT NULL,
        region TEXT NOT NULL,
        cluster_id TEXT NOT NULL,
        host TEXT NOT NULL,
        role TEXT NOT NULL,
        schema_name TEXT,
        production INTEGER NOT NULL,
        read_only INTEGER NOT NULL
    );
    CREATE TABLE history (
        id INTEGER PRIMARY KEY,
        connection_id INTEGER,
        sql TEXT NOT NULL,
        started_at TEXT NOT NULL,
        duration_ms INTEGER NOT NULL,
        rows INTEGER,
        error TEXT
    );
    CREATE INDEX history_started_at ON history (started_at);
    CREATE TABLE settings (name TEXT PRIMARY KEY, value TEXT NOT NULL);
"];

fn store_error(e: rusqlite::Error) -> CoreError {
    CoreError::Io(format!("local database: {e}"))
}

/// Fixed-width UTC timestamps sort the same as text and as time.
fn timestamp(t: &DateTime<Utc>) -> String {
    t.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl Store {
    /// Opens (or creates) the database file and brings its schema up to date.
    pub fn open(path: &Path) -> Result<Store, CoreError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let db = Connection::open(path).map_err(store_error)?;
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(store_error)?;
        for (i, migration) in MIGRATIONS.iter().enumerate().skip(version.max(0) as usize) {
            db.execute_batch(&format!(
                "BEGIN; {migration} PRAGMA user_version = {}; COMMIT;",
                i + 1
            ))
            .map_err(store_error)?;
        }
        Ok(Store { db })
    }

    /// Inserts a new connection, or updates the one with the same `id`. Returns the id.
    pub fn save_connection(&self, c: &SavedConnection) -> Result<i64, CoreError> {
        let values = rusqlite::params![
            c.id,
            c.name,
            c.profile,
            c.region,
            c.cluster_id,
            c.host,
            c.role,
            c.schema,
            c.production,
            c.read_only
        ];
        self.db
            .execute(
                "INSERT INTO connections (id, name, profile, region, cluster_id, host, role, schema_name, production, read_only)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT (id) DO UPDATE SET name = ?2, profile = ?3, region = ?4, cluster_id = ?5, host = ?6,
                     role = ?7, schema_name = ?8, production = ?9, read_only = ?10",
                values,
            )
            .map_err(store_error)?;
        Ok(c.id.unwrap_or_else(|| self.db.last_insert_rowid()))
    }

    pub fn list_connections(&self) -> Result<Vec<SavedConnection>, CoreError> {
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, name, profile, region, cluster_id, host, role, schema_name, production, read_only
                 FROM connections ORDER BY name COLLATE NOCASE, id",
            )
            .map_err(store_error)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(SavedConnection {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    profile: r.get(2)?,
                    region: r.get(3)?,
                    cluster_id: r.get(4)?,
                    host: r.get(5)?,
                    role: r.get(6)?,
                    schema: r.get(7)?,
                    production: r.get(8)?,
                    read_only: r.get(9)?,
                })
            })
            .map_err(store_error)?;
        rows.collect::<Result<_, _>>().map_err(store_error)
    }

    pub fn delete_connection(&self, id: i64) -> Result<(), CoreError> {
        self.db
            .execute("DELETE FROM connections WHERE id = ?1", [id])
            .map_err(store_error)?;
        Ok(())
    }

    pub fn add_history(&self, e: &HistoryEntry) -> Result<i64, CoreError> {
        self.db
            .execute(
                "INSERT INTO history (connection_id, sql, started_at, duration_ms, rows, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    e.connection_id,
                    e.sql,
                    timestamp(&e.started_at),
                    e.duration_ms as i64,
                    e.rows.map(|r| r as i64),
                    e.error
                ],
            )
            .map_err(store_error)?;
        Ok(self.db.last_insert_rowid())
    }

    /// Newest first. `q` matches anywhere in the SQL, case-insensitively; `%` and `_` in it are literal.
    pub fn search_history(&self, q: &str, limit: usize) -> Result<Vec<HistoryEntry>, CoreError> {
        let pattern = format!(
            "%{}%",
            q.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let mut stmt = self
            .db
            .prepare(
                "SELECT id, connection_id, sql, started_at, duration_ms, rows, error FROM history
                 WHERE sql LIKE ?1 ESCAPE '\\' ORDER BY started_at DESC, id DESC LIMIT ?2",
            )
            .map_err(store_error)?;
        let rows = stmt
            .query_map(rusqlite::params![pattern, limit as i64], |r| {
                let started: String = r.get(3)?;
                Ok(HistoryEntry {
                    id: r.get(0)?,
                    connection_id: r.get(1)?,
                    sql: r.get(2)?,
                    started_at: DateTime::parse_from_rfc3339(&started)
                        .map(|t| t.with_timezone(&Utc))
                        .unwrap_or_default(),
                    duration_ms: r.get::<_, i64>(4)? as u64,
                    rows: r.get::<_, Option<i64>>(5)?.map(|n| n as u64),
                    error: r.get(6)?,
                })
            })
            .map_err(store_error)?;
        rows.collect::<Result<_, _>>().map_err(store_error)
    }

    pub fn clear_history(&self) -> Result<(), CoreError> {
        self.db
            .execute("DELETE FROM history", [])
            .map_err(store_error)?;
        Ok(())
    }

    pub fn get_setting(&self, name: &str) -> Result<Option<String>, CoreError> {
        use rusqlite::OptionalExtension;
        self.db
            .query_row("SELECT value FROM settings WHERE name = ?1", [name], |r| {
                r.get(0)
            })
            .optional()
            .map_err(store_error)
    }

    pub fn set_setting(&self, name: &str, value: &str) -> Result<(), CoreError> {
        self.db
            .execute(
                "INSERT INTO settings (name, value) VALUES (?1, ?2) ON CONFLICT (name) DO UPDATE SET value = ?2",
                [name, value],
            )
            .map_err(store_error)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("studio.sqlite")).unwrap();
        (dir, store)
    }

    fn sample_connection() -> SavedConnection {
        SavedConnection {
            id: None,
            name: "My app (staging)".into(),
            profile: "dev".into(),
            region: "us-east-1".into(),
            cluster_id: "abc123".into(),
            host: "abc123.dsql.us-east-1.on.aws".into(),
            role: "admin".into(),
            schema: Some("myapp_staging".into()),
            production: false,
            read_only: true,
        }
    }

    fn entry(sql: &str, minute: u32, error: Option<&str>) -> HistoryEntry {
        HistoryEntry {
            id: None,
            connection_id: Some(1),
            sql: sql.into(),
            started_at: Utc.with_ymd_and_hms(2026, 11, 5, 10, minute, 0).unwrap(),
            duration_ms: 12,
            rows: error.is_none().then_some(3),
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn store_roundtrip_connection() {
        let (_dir, store) = store();
        let id = store.save_connection(&sample_connection()).unwrap();
        let listed = store.list_connections().unwrap();
        assert_eq!(
            listed,
            vec![SavedConnection {
                id: Some(id),
                ..sample_connection()
            }]
        );

        // Saving with an id updates in place.
        let renamed = SavedConnection {
            id: Some(id),
            name: "Renamed".into(),
            production: true,
            ..sample_connection()
        };
        assert_eq!(store.save_connection(&renamed).unwrap(), id);
        assert_eq!(store.list_connections().unwrap(), vec![renamed]);

        store.delete_connection(id).unwrap();
        assert!(store.list_connections().unwrap().is_empty());
    }

    #[test]
    fn store_history_search() {
        let (_dir, store) = store();
        store
            .add_history(&entry("select * from users", 1, None))
            .unwrap();
        store
            .add_history(&entry("select * from orders", 2, None))
            .unwrap();
        store
            .add_history(&entry(
                "delete from users where id = 1",
                3,
                Some("permission denied"),
            ))
            .unwrap();
        store
            .add_history(&entry("select '100%_done'", 4, None))
            .unwrap();

        let users: Vec<String> = store
            .search_history("users", 10)
            .unwrap()
            .into_iter()
            .map(|e| e.sql)
            .collect();
        assert_eq!(
            users,
            ["delete from users where id = 1", "select * from users"],
            "newest first"
        );
        assert_eq!(store.search_history("", 2).unwrap().len(), 2);
        // % and _ are literal characters, not wildcards.
        assert_eq!(store.search_history("%_", 10).unwrap().len(), 1);
        let failed = &store.search_history("delete", 10).unwrap()[0];
        assert_eq!(failed.error.as_deref(), Some("permission denied"));
        assert_eq!(failed.rows, None);
        assert_eq!(
            failed.started_at,
            Utc.with_ymd_and_hms(2026, 11, 5, 10, 3, 0).unwrap()
        );

        store.clear_history().unwrap();
        assert!(store.search_history("", 10).unwrap().is_empty());
    }

    #[test]
    fn store_settings() {
        let (_dir, store) = store();
        assert_eq!(store.get_setting("last_regions").unwrap(), None);
        store.set_setting("last_regions", "us-east-1").unwrap();
        store
            .set_setting("last_regions", "us-east-1,eu-west-1")
            .unwrap();
        assert_eq!(
            store.get_setting("last_regions").unwrap().as_deref(),
            Some("us-east-1,eu-west-1")
        );
    }

    #[test]
    fn store_reopens_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("studio.sqlite");
        let id = Store::open(&path)
            .unwrap()
            .save_connection(&sample_connection())
            .unwrap();
        assert_eq!(
            Store::open(&path).unwrap().list_connections().unwrap()[0].id,
            Some(id)
        );
    }

    #[test]
    fn store_holds_no_secret_columns() {
        let (_dir, store) = store();
        let mut stmt = store
            .db
            .prepare("select m.name, p.name from sqlite_master m join pragma_table_info(m.name) p where m.type = 'table'")
            .unwrap();
        let columns: Vec<(String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert!(!columns.is_empty());
        for (table, column) in columns {
            let c = column.to_ascii_lowercase();
            for word in ["token", "secret", "password", "key"] {
                assert!(
                    !c.contains(word),
                    "{table}.{column} looks like it holds a secret"
                );
            }
        }
    }
}
