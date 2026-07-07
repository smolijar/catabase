use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ConnectionFields, CoreError};

const KEYRING_SERVICE: &str = "catabase";
const RESULT_JSON_CAP_BYTES: usize = 256 * 1024;
const KEYRING_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct Storage {
    conn: Arc<Mutex<Connection>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub pool_size: u32,
    pub ai_model: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionInput {
    pub id: Option<String>,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: Option<String>,
    pub ssl: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionRecord {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: Option<String>,
    pub ssl: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewHistoryEntry {
    pub connection_id: Option<String>,
    pub query: String,
    pub duration_ms: u128,
    pub row_count: Option<usize>,
    pub is_error: bool,
    pub error_message: Option<String>,
    pub result_json: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub connection_id: Option<String>,
    pub query: String,
    pub duration_ms: i64,
    pub row_count: Option<i64>,
    pub result_size: Option<i64>,
    pub is_error: bool,
    pub error_message: Option<String>,
    pub has_result: bool,
    pub executed_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryPage {
    pub entries: Vec<HistoryEntry>,
    pub total: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryStats {
    pub total: i64,
    pub errors: i64,
    pub last_executed_at: Option<String>,
}

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CoreError> {
        let conn = Connection::open(path)?;
        let storage = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        storage.migrate()?;
        Ok(storage)
    }

    pub fn in_memory() -> Result<Self, CoreError> {
        let storage = Self {
            conn: Arc::new(Mutex::new(Connection::open_in_memory()?)),
        };
        storage.migrate()?;
        Ok(storage)
    }

    pub fn get_settings(&self) -> Result<Settings, CoreError> {
        let conn = self.lock()?;
        let pool_size = setting_value(&conn, "pool_size")?
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(5)
            .clamp(1, 32);
        let ai_model = setting_value(&conn, "ai_model")?.filter(|value| !value.is_empty());
        Ok(Settings {
            pool_size,
            ai_model,
        })
    }

    pub fn set_pool_size(&self, pool_size: u32) -> Result<u32, CoreError> {
        let normalized = pool_size.clamp(1, 32);
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES ('pool_size', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![normalized.to_string()],
        )?;
        Ok(normalized)
    }

    pub fn set_ai_model(&self, model: Option<String>) -> Result<Option<String>, CoreError> {
        let normalized = model.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(trimmed)
        });
        let conn = self.lock()?;
        match &normalized {
            Some(model) => {
                conn.execute(
                    "INSERT INTO settings (key, value) VALUES ('ai_model', ?1)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![model],
                )?;
            }
            None => {
                conn.execute("DELETE FROM settings WHERE key = 'ai_model'", [])?;
            }
        }
        Ok(normalized)
    }

    pub fn list_connections(&self) -> Result<Vec<ConnectionRecord>, CoreError> {
        let conn = self.lock()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, host, port, database, username, password_fallback, ssl,
                    is_active, created_at, updated_at
             FROM connections
             ORDER BY is_active DESC, updated_at DESC, name ASC",
        )?;
        let rows = stmt
            .query_map([], row_to_connection)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().map(resolve_connection_password).collect())
    }

    pub fn active_connection(&self) -> Result<Option<ConnectionRecord>, CoreError> {
        let conn = self.lock()?;
        let record = conn
            .query_row(
                "SELECT id, name, host, port, database, username, password_fallback, ssl,
                        is_active, created_at, updated_at
                 FROM connections WHERE is_active = 1 LIMIT 1",
                [],
                row_to_connection,
            )
            .optional()?;
        Ok(record.map(resolve_connection_password))
    }

    pub fn save_connection(&self, input: ConnectionInput) -> Result<ConnectionRecord, CoreError> {
        validate_connection(&input)?;

        let now = Utc::now().to_rfc3339();
        let id = input
            .id
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let keyring_verified = input
            .password
            .as_deref()
            .map(|password| set_and_verify_keyring_password(&id, password))
            .unwrap_or(false);
        let fallback_password = input.password.clone().filter(|_| !keyring_verified);

        {
            let mut conn = self.lock()?;
            let tx = conn.transaction()?;
            let existing = tx
                .query_row(
                    "SELECT created_at, is_active, password_fallback FROM connections WHERE id = ?1",
                    params![id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, i64>(1)? == 1,
                            row.get::<_, Option<String>>(2)?,
                        ))
                    },
                )
                .optional()?;
            let is_new = existing.is_none();
            let created_at = existing
                .as_ref()
                .map(|(created_at, _, _)| created_at.clone())
                .unwrap_or_else(|| now.clone());
            let is_active = existing
                .as_ref()
                .map(|(_, is_active, _)| *is_active)
                .unwrap_or(true);
            let stored_fallback =
                fallback_password.or_else(|| existing.and_then(|(_, _, password)| password));

            if is_new {
                tx.execute("UPDATE connections SET is_active = 0", [])?;
            }

            tx.execute(
                "INSERT INTO connections (
                    id, name, host, port, database, username, ssl, password_fallback,
                    is_active, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    host = excluded.host,
                    port = excluded.port,
                    database = excluded.database,
                    username = excluded.username,
                    ssl = excluded.ssl,
                    password_fallback = excluded.password_fallback,
                    is_active = excluded.is_active,
                    updated_at = excluded.updated_at",
                params![
                    id,
                    input.name.trim(),
                    input.host.trim(),
                    input.port,
                    input.database.trim(),
                    input.username.trim(),
                    bool_to_i64(input.ssl),
                    stored_fallback,
                    bool_to_i64(is_active || is_new),
                    created_at,
                    now,
                ],
            )?;
            tx.commit()?;
        }

        self.get_connection(&id)
            .and_then(|record| record.ok_or_else(|| CoreError::message("connection was not saved")))
    }

    pub fn get_connection(&self, id: &str) -> Result<Option<ConnectionRecord>, CoreError> {
        let conn = self.lock()?;
        let record = conn
            .query_row(
                "SELECT id, name, host, port, database, username, password_fallback, ssl,
                        is_active, created_at, updated_at
                 FROM connections WHERE id = ?1",
                params![id],
                row_to_connection,
            )
            .optional()?;
        Ok(record.map(resolve_connection_password))
    }

    pub fn activate_connection(&self, id: &str) -> Result<ConnectionRecord, CoreError> {
        let changed = {
            let mut conn = self.lock()?;
            let tx = conn.transaction()?;
            tx.execute("UPDATE connections SET is_active = 0", [])?;
            let changed = tx.execute(
                "UPDATE connections SET is_active = 1, updated_at = ?2 WHERE id = ?1",
                params![id, Utc::now().to_rfc3339()],
            )?;
            tx.commit()?;
            changed
        };
        if changed == 0 {
            return Err(CoreError::message("connection not found"));
        }
        self.get_connection(id)?
            .ok_or_else(|| CoreError::message("connection not found"))
    }

    pub fn delete_connection(&self, id: &str) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM connections WHERE id = ?1", params![id])?;
        let _ = delete_keyring_password(id);
        Ok(())
    }

    pub fn save_draft(&self, tab_kind: &str, content: &str) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO script_drafts (tab_kind, content, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(tab_kind) DO UPDATE SET
                content = excluded.content,
                updated_at = excluded.updated_at",
            params![tab_kind, content, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn load_draft(&self, tab_kind: &str) -> Result<Option<String>, CoreError> {
        let conn = self.lock()?;
        Ok(conn
            .query_row(
                "SELECT content FROM script_drafts WHERE tab_kind = ?1",
                params![tab_kind],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn insert_history(&self, entry: NewHistoryEntry) -> Result<i64, CoreError> {
        let serialized = entry
            .result_json
            .map(|value| serde_json::to_string(&value))
            .transpose()?;
        let result_size = serialized.as_ref().map(|value| value.len() as i64);
        let stored_result = serialized.filter(|value| value.len() <= RESULT_JSON_CAP_BYTES);
        let conn = self.lock()?;
        conn.execute(
            "INSERT INTO query_history (
                connection_id, query, duration_ms, row_count, result_size, is_error,
                error_message, result_json, executed_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                entry.connection_id,
                entry.query,
                i64::try_from(entry.duration_ms).unwrap_or(i64::MAX),
                entry.row_count.map(|value| value as i64),
                result_size,
                bool_to_i64(entry.is_error),
                entry.error_message,
                stored_result,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn history_page(
        &self,
        search: Option<String>,
        offset: u32,
        limit: u32,
    ) -> Result<HistoryPage, CoreError> {
        let conn = self.lock()?;
        let normalized_limit = limit.clamp(1, 200);
        let search = search.and_then(|value| {
            let trimmed = value.trim().to_string();
            (!trimmed.is_empty()).then_some(format!("%{trimmed}%"))
        });

        let total = match &search {
            Some(search) => conn.query_row(
                "SELECT COUNT(*) FROM query_history WHERE query LIKE ?1 OR error_message LIKE ?1",
                params![search],
                |row| row.get(0),
            )?,
            None => conn.query_row("SELECT COUNT(*) FROM query_history", [], |row| row.get(0))?,
        };

        let entries = match &search {
            Some(search) => {
                let mut stmt = conn.prepare(HISTORY_SELECT_WITH_SEARCH)?;
                stmt.query_map(params![search, normalized_limit, offset], row_to_history)?
                    .collect::<Result<Vec<_>, _>>()?
            }
            None => {
                let mut stmt = conn.prepare(HISTORY_SELECT)?;
                stmt.query_map(params![normalized_limit, offset], row_to_history)?
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        Ok(HistoryPage { entries, total })
    }

    pub fn history_stats(&self) -> Result<HistoryStats, CoreError> {
        let conn = self.lock()?;
        conn.query_row(
            "SELECT COUNT(*), COALESCE(SUM(is_error), 0), MAX(executed_at) FROM query_history",
            [],
            |row| {
                Ok(HistoryStats {
                    total: row.get(0)?,
                    errors: row.get(1)?,
                    last_executed_at: row.get(2)?,
                })
            },
        )
        .map_err(CoreError::from)
    }

    pub fn history_result(&self, id: i64) -> Result<Option<serde_json::Value>, CoreError> {
        let conn = self.lock()?;
        let json: Option<String> = conn
            .query_row(
                "SELECT result_json FROM query_history WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        json.map(|value| serde_json::from_str(&value))
            .transpose()
            .map_err(CoreError::from)
    }

    pub fn history_delete(&self, id: i64) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM query_history WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn history_purge(&self) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute("DELETE FROM query_history", [])?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), CoreError> {
        let conn = self.lock()?;
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;

             CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS connections (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                host TEXT NOT NULL,
                port INTEGER NOT NULL DEFAULT 5432,
                database TEXT NOT NULL,
                username TEXT NOT NULL,
                ssl INTEGER NOT NULL DEFAULT 0,
                password_fallback TEXT,
                is_active INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS script_drafts (
                tab_kind TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                updated_at TEXT NOT NULL
             );

             CREATE TABLE IF NOT EXISTS query_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                connection_id TEXT,
                query TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                row_count INTEGER,
                result_size INTEGER,
                is_error INTEGER NOT NULL DEFAULT 0,
                error_message TEXT,
                result_json TEXT,
                executed_at TEXT NOT NULL
             );

             CREATE INDEX IF NOT EXISTS idx_history_executed
                ON query_history(executed_at DESC);",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('pool_size', '5')",
            [],
        )?;
        Ok(())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, CoreError> {
        self.conn
            .lock()
            .map_err(|_| CoreError::message("storage lock poisoned"))
    }
}

impl From<&ConnectionRecord> for ConnectionFields {
    fn from(record: &ConnectionRecord) -> Self {
        Self {
            name: record.name.clone(),
            host: record.host.clone(),
            port: record.port,
            database: record.database.clone(),
            username: record.username.clone(),
            password: record.password.clone(),
            ssl: record.ssl,
        }
    }
}

impl From<&ConnectionInput> for ConnectionFields {
    fn from(input: &ConnectionInput) -> Self {
        Self {
            name: input.name.clone(),
            host: input.host.clone(),
            port: input.port,
            database: input.database.clone(),
            username: input.username.clone(),
            password: input.password.clone(),
            ssl: input.ssl,
        }
    }
}

const HISTORY_SELECT: &str =
    "SELECT id, connection_id, query, duration_ms, row_count, result_size, is_error,
            error_message, result_json IS NOT NULL, executed_at
     FROM query_history
     ORDER BY executed_at DESC
     LIMIT ?1 OFFSET ?2";

const HISTORY_SELECT_WITH_SEARCH: &str =
    "SELECT id, connection_id, query, duration_ms, row_count, result_size, is_error,
            error_message, result_json IS NOT NULL, executed_at
     FROM query_history
     WHERE query LIKE ?1 OR error_message LIKE ?1
     ORDER BY executed_at DESC
     LIMIT ?2 OFFSET ?3";

fn setting_value(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    )
    .optional()
}

fn validate_connection(input: &ConnectionInput) -> Result<(), CoreError> {
    if input.name.trim().is_empty() {
        return Err(CoreError::message("connection name is required"));
    }
    if input.host.trim().is_empty() {
        return Err(CoreError::message("host is required"));
    }
    if input.database.trim().is_empty() {
        return Err(CoreError::message("database is required"));
    }
    if input.username.trim().is_empty() {
        return Err(CoreError::message("username is required"));
    }
    if input.port == 0 {
        return Err(CoreError::message("port is required"));
    }
    Ok(())
}

fn row_to_connection(row: &rusqlite::Row<'_>) -> Result<ConnectionRecord, rusqlite::Error> {
    Ok(ConnectionRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        host: row.get(2)?,
        port: row.get::<_, u16>(3)?,
        database: row.get(4)?,
        username: row.get(5)?,
        password: row.get(6)?,
        ssl: row.get::<_, i64>(7)? == 1,
        is_active: row.get::<_, i64>(8)? == 1,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn row_to_history(row: &rusqlite::Row<'_>) -> Result<HistoryEntry, rusqlite::Error> {
    Ok(HistoryEntry {
        id: row.get(0)?,
        connection_id: row.get(1)?,
        query: row.get(2)?,
        duration_ms: row.get(3)?,
        row_count: row.get(4)?,
        result_size: row.get(5)?,
        is_error: row.get::<_, i64>(6)? == 1,
        error_message: row.get(7)?,
        has_result: row.get::<_, i64>(8)? == 1,
        executed_at: row.get(9)?,
    })
}

fn resolve_connection_password(mut record: ConnectionRecord) -> ConnectionRecord {
    if let Some(password) = get_keyring_password(&record.id) {
        record.password = Some(password);
        return record;
    }
    record
}

fn set_and_verify_keyring_password(id: &str, password: &str) -> bool {
    let id = id.to_string();
    let password = password.to_string();
    run_keyring(move || {
        let entry = keyring::Entry::new(KEYRING_SERVICE, &id)
            .map_err(|error| CoreError::Keyring(error.to_string()))?;
        entry
            .set_password(&password)
            .map_err(|error| CoreError::Keyring(error.to_string()))?;
        let stored = entry
            .get_password()
            .map_err(|error| CoreError::Keyring(error.to_string()))?;
        Ok(stored == password)
    })
    .ok()
    .unwrap_or(false)
}

fn get_keyring_password(id: &str) -> Option<String> {
    let id = id.to_string();
    run_keyring(move || {
        keyring::Entry::new(KEYRING_SERVICE, &id)
            .map_err(|error| CoreError::Keyring(error.to_string()))?
            .get_password()
            .map_err(|error| CoreError::Keyring(error.to_string()))
    })
    .ok()
}

fn delete_keyring_password(id: &str) -> Result<(), CoreError> {
    let id = id.to_string();
    run_keyring(move || {
        keyring::Entry::new(KEYRING_SERVICE, &id)
            .map_err(|error| CoreError::Keyring(error.to_string()))?
            .delete_credential()
            .map_err(|error| CoreError::Keyring(error.to_string()))
    })
}

fn run_keyring<T, F>(operation: F) -> Result<T, CoreError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, CoreError> + Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(operation());
    });
    receiver
        .recv_timeout(KEYRING_TIMEOUT)
        .map_err(|_| CoreError::message("keyring timeout"))?
}

fn bool_to_i64(value: bool) -> i64 {
    if value { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_default_settings() {
        let storage = Storage::in_memory().unwrap();
        let settings = storage.get_settings().unwrap();

        assert_eq!(settings.pool_size, 5);
        assert_eq!(settings.ai_model, None);
    }

    #[test]
    fn normalizes_pool_size() {
        let storage = Storage::in_memory().unwrap();

        assert_eq!(storage.set_pool_size(0).unwrap(), 1);
        assert_eq!(storage.set_pool_size(999).unwrap(), 32);
    }

    #[test]
    fn stores_drafts_and_history() {
        let storage = Storage::in_memory().unwrap();
        storage.save_draft("playground", "select 1").unwrap();
        assert_eq!(
            storage.load_draft("playground").unwrap().as_deref(),
            Some("select 1")
        );

        let id = storage
            .insert_history(NewHistoryEntry {
                connection_id: None,
                query: "select 1".to_string(),
                duration_ms: 4,
                row_count: Some(1),
                is_error: false,
                error_message: None,
                result_json: Some(serde_json::json!({"rows": [{"?column?": 1}]})),
            })
            .unwrap();

        assert_eq!(storage.history_page(None, 0, 20).unwrap().total, 1);
        assert!(storage.history_result(id).unwrap().is_some());
    }

    #[test]
    fn saves_and_activates_connections_without_deadlock() {
        let storage = Storage::in_memory().unwrap();
        let first = storage
            .save_connection(ConnectionInput {
                id: None,
                name: "first".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                database: "postgres".to_string(),
                username: "postgres".to_string(),
                password: None,
                ssl: false,
            })
            .unwrap();
        let second = storage
            .save_connection(ConnectionInput {
                id: None,
                name: "second".to_string(),
                host: "localhost".to_string(),
                port: 5432,
                database: "postgres".to_string(),
                username: "postgres".to_string(),
                password: None,
                ssl: false,
            })
            .unwrap();

        assert!(second.is_active);
        assert_eq!(storage.activate_connection(&first.id).unwrap().id, first.id);
        assert_eq!(storage.active_connection().unwrap().unwrap().id, first.id);
    }
}
