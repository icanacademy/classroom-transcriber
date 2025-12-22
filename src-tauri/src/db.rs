use rusqlite::{Connection, Result as SqliteResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Recording {
    pub id: String,
    pub student_id: String,
    pub audio_path: String,
    pub transcript: Option<String>,
    pub duration_seconds: f64,
    pub recorded_at: String,
    pub synced: bool,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn new(data_dir: &PathBuf) -> SqliteResult<Self> {
        std::fs::create_dir_all(data_dir).ok();
        let db_path = data_dir.join("transcriber.db");
        let conn = Connection::open(db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS recordings (
                id TEXT PRIMARY KEY,
                student_id TEXT NOT NULL,
                audio_path TEXT NOT NULL,
                transcript TEXT,
                duration_seconds REAL,
                recorded_at TEXT NOT NULL,
                synced INTEGER DEFAULT 0
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    pub fn save_recording(&self, recording: &Recording) -> SqliteResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO recordings (id, student_id, audio_path, transcript, duration_seconds, recorded_at, synced)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                &recording.id,
                &recording.student_id,
                &recording.audio_path,
                &recording.transcript,
                recording.duration_seconds,
                &recording.recorded_at,
                recording.synced as i32,
            ),
        )?;
        Ok(())
    }

    pub fn get_all_recordings(&self) -> SqliteResult<Vec<Recording>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, student_id, audio_path, transcript, duration_seconds, recorded_at, synced
             FROM recordings ORDER BY recorded_at DESC"
        )?;

        let recordings = stmt.query_map([], |row| {
            Ok(Recording {
                id: row.get(0)?,
                student_id: row.get(1)?,
                audio_path: row.get(2)?,
                transcript: row.get(3)?,
                duration_seconds: row.get(4)?,
                recorded_at: row.get(5)?,
                synced: row.get::<_, i32>(6)? != 0,
            })
        })?;

        recordings.collect()
    }

    pub fn get_unsynced_recordings(&self) -> SqliteResult<Vec<Recording>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, student_id, audio_path, transcript, duration_seconds, recorded_at, synced
             FROM recordings WHERE synced = 0 AND transcript IS NOT NULL"
        )?;

        let recordings = stmt.query_map([], |row| {
            Ok(Recording {
                id: row.get(0)?,
                student_id: row.get(1)?,
                audio_path: row.get(2)?,
                transcript: row.get(3)?,
                duration_seconds: row.get(4)?,
                recorded_at: row.get(5)?,
                synced: row.get::<_, i32>(6)? != 0,
            })
        })?;

        recordings.collect()
    }

    pub fn mark_synced(&self, id: &str) -> SqliteResult<()> {
        self.conn.execute(
            "UPDATE recordings SET synced = 1 WHERE id = ?1",
            [id],
        )?;
        Ok(())
    }

    pub fn delete_recording(&self, id: &str) -> SqliteResult<()> {
        self.conn.execute("DELETE FROM recordings WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> SqliteResult<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query([key])?;

        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> SqliteResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2)",
            [key, value],
        )?;
        Ok(())
    }
}
