use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, params};

#[derive(Debug, Clone)]
pub struct Row {
    pub key: String,
    pub task_id: Option<i64>,
    pub tombstone: bool,
    pub start: DateTime<Utc>,
    pub end: Option<DateTime<Utc>>,
    pub hash: String,
}

pub struct Database { connection: Connection }

impl Database {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }
        let connection = Connection::open(path)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS feed_state (
                feed_id TEXT PRIMARY KEY, project_id INTEGER, disabled INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS occurrence (
                feed_id TEXT NOT NULL, occurrence_key TEXT NOT NULL, task_id INTEGER,
                tombstone INTEGER NOT NULL DEFAULT 0, start TEXT NOT NULL, end TEXT,
                content_hash TEXT NOT NULL, PRIMARY KEY(feed_id, occurrence_key)
            );")?;
        Ok(Self { connection })
    }

    pub fn feed_state(&self, feed_id: &str) -> Result<Option<(Option<i64>, bool)>> {
        Ok(self.connection.query_row(
            "SELECT project_id, disabled FROM feed_state WHERE feed_id=?1", [feed_id],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
        ).optional()?)
    }

    pub fn ensure_feed(&self, feed_id: &str) -> Result<()> {
        self.connection.execute("INSERT OR IGNORE INTO feed_state(feed_id) VALUES(?1)", [feed_id])?;
        Ok(())
    }

    pub fn project(&self, feed_id: &str, project_id: i64) -> Result<()> {
        self.connection.execute("UPDATE feed_state SET project_id=?2 WHERE feed_id=?1", params![feed_id, project_id])?;
        Ok(())
    }

    pub fn disable(&self, feed_id: &str) -> Result<()> {
        self.connection.execute("UPDATE feed_state SET disabled=1 WHERE feed_id=?1", [feed_id])?;
        Ok(())
    }

    pub fn configured_feed_ids(&self) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare("SELECT feed_id FROM feed_state ORDER BY feed_id")?;
        Ok(statement.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn get(&self, feed_id: &str, key: &str) -> Result<Option<Row>> {
        self.connection.query_row("SELECT occurrence_key, task_id, tombstone, start, end, content_hash FROM occurrence WHERE feed_id=?1 AND occurrence_key=?2", params![feed_id, key], decode).optional().map_err(Into::into)
    }

    pub fn rows(&self, feed_id: &str) -> Result<Vec<Row>> {
        let mut statement = self.connection.prepare("SELECT occurrence_key, task_id, tombstone, start, end, content_hash FROM occurrence WHERE feed_id=?1")?;
        Ok(statement.query_map([feed_id], decode)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn save(&self, feed_id: &str, key: &str, task_id: Option<i64>, tombstone: bool, start: DateTime<Utc>, end: Option<DateTime<Utc>>, hash: &str) -> Result<()> {
        self.connection.execute("INSERT INTO occurrence(feed_id, occurrence_key, task_id, tombstone, start, end, content_hash) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(feed_id, occurrence_key) DO UPDATE SET task_id=excluded.task_id, tombstone=excluded.tombstone, start=excluded.start, end=excluded.end, content_hash=excluded.content_hash", params![feed_id, key, task_id, tombstone as i64, start.to_rfc3339(), end.map(|v| v.to_rfc3339()), hash])?;
        Ok(())
    }
}

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    let start: String = row.get(3)?;
    let end: Option<String> = row.get(4)?;
    Ok(Row {
        key: row.get(0)?,
        task_id: row.get(1)?,
        tombstone: row.get::<_, i64>(2)? != 0,
        start: start.parse().map_err(|_| rusqlite::Error::InvalidColumnType(3, "start".into(), rusqlite::types::Type::Text))?,
        end: end.map(|value| value.parse().map_err(|_| rusqlite::Error::InvalidColumnType(4, "end".into(), rusqlite::types::Type::Text))).transpose()?,
        hash: row.get(5)?,
    })
}
