use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection};

use super::jobs::{Job, JobStatus};

pub struct JobDb {
    conn: Mutex<Connection>,
}

impl JobDb {
    pub fn init(data_dir: &Path) -> Result<Self, String> {
        std::fs::create_dir_all(data_dir).map_err(|e| format!("failed to create data dir: {e}"))?;
        let db_path = data_dir.join("jobs.db");
        let conn =
            Connection::open(&db_path).map_err(|e| format!("failed to open database: {e}"))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS jobs (
                id          TEXT PRIMARY KEY,
                status      TEXT NOT NULL,
                output      TEXT,
                error       TEXT,
                message     TEXT NOT NULL,
                provider    TEXT NOT NULL,
                project     TEXT,
                channel     TEXT,
                created_at  TEXT NOT NULL,
                finished_at TEXT
            );",
        )
        .map_err(|e| format!("failed to create jobs table: {e}"))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, job: &Job) {
        let guard = match self.conn.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("db lock poisoned on insert: {e}");
                return;
            }
        };
        if let Err(e) = guard.execute(
            "INSERT OR IGNORE INTO jobs
             (id, status, output, error, message, provider, project, channel, created_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                job.id,
                status_to_str(&job.status),
                job.output,
                job.error,
                job.message,
                job.provider,
                job.project,
                job.channel,
                job.created_at,
                job.finished_at,
            ],
        ) {
            tracing::error!(job_id = %job.id, error = %e, "db insert failed");
        }
    }

    pub fn update_status(
        &self,
        id: &str,
        status: &JobStatus,
        output: Option<&str>,
        error: Option<&str>,
        finished_at: Option<&str>,
    ) {
        let guard = match self.conn.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("db lock poisoned on update_status: {e}");
                return;
            }
        };
        if let Err(e) = guard.execute(
            "UPDATE jobs SET status = ?1, output = ?2, error = ?3, finished_at = ?4 WHERE id = ?5",
            params![status_to_str(status), output, error, finished_at, id],
        ) {
            tracing::error!(job_id = %id, error = %e, "db update_status failed");
        }
    }

    pub fn load_all(&self) -> Vec<Job> {
        let guard = match self.conn.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("db lock poisoned on load_all: {e}");
                return vec![];
            }
        };
        let mut stmt = match guard.prepare(
            "SELECT id, status, output, error, message, provider, project, channel, \
             created_at, finished_at FROM jobs",
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::error!("db prepare failed on load_all: {e}");
                return vec![];
            }
        };
        let rows = stmt.query_map([], |row| {
            Ok(Job {
                id: row.get(0)?,
                status: str_to_status(&row.get::<_, String>(1)?),
                output: row.get(2)?,
                error: row.get(3)?,
                message: row.get(4)?,
                provider: row.get(5)?,
                project: row.get(6)?,
                channel: row.get(7)?,
                created_at: row.get(8)?,
                finished_at: row.get(9)?,
            })
        });
        match rows {
            Ok(iter) => iter.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                tracing::error!("db query_map failed on load_all: {e}");
                vec![]
            }
        }
    }

    pub fn mark_running_as_failed(&self) {
        let guard = match self.conn.lock() {
            Ok(g) => g,
            Err(e) => {
                tracing::error!("db lock poisoned on mark_running_as_failed: {e}");
                return;
            }
        };
        if let Err(e) = guard.execute(
            "UPDATE jobs SET status = 'failed' WHERE status = 'running'",
            [],
        ) {
            tracing::error!("db mark_running_as_failed failed: {e}");
        }
    }
}

fn status_to_str(s: &JobStatus) -> &'static str {
    match s {
        JobStatus::Queued => "queued",
        JobStatus::Running => "running",
        JobStatus::Done => "done",
        JobStatus::Flagged => "flagged",
        JobStatus::Failed => "failed",
    }
}

fn str_to_status(s: &str) -> JobStatus {
    match s {
        "running" => JobStatus::Running,
        "done" => JobStatus::Done,
        "flagged" => JobStatus::Flagged,
        "failed" => JobStatus::Failed,
        _ => JobStatus::Queued,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db() -> JobDb {
        let dir = std::env::temp_dir().join(format!("__kx_db_test_{}__", uuid::Uuid::new_v4()));
        JobDb::init(&dir).unwrap()
    }

    fn make_job(id: &str) -> Job {
        Job {
            id: id.to_string(),
            status: JobStatus::Queued,
            output: None,
            error: None,
            message: "test".to_string(),
            provider: "claude-code".to_string(),
            project: None,
            channel: None,
            created_at: "2026-04-03T00:00:00Z".to_string(),
            finished_at: None,
        }
    }

    #[test]
    fn insert_and_load_all() {
        let db = temp_db();
        db.insert(&make_job("j1"));
        db.insert(&make_job("j2"));
        let jobs = db.load_all();
        assert_eq!(jobs.len(), 2);
    }

    #[test]
    fn update_status_changes_status() {
        let db = temp_db();
        db.insert(&make_job("j3"));
        db.update_status(
            "j3",
            &JobStatus::Done,
            Some("result"),
            None,
            Some("2026-04-03T01:00:00Z"),
        );
        let jobs = db.load_all();
        let j = jobs.iter().find(|j| j.id == "j3").unwrap();
        assert_eq!(j.status, JobStatus::Done);
        assert_eq!(j.output, Some("result".to_string()));
        assert_eq!(j.finished_at, Some("2026-04-03T01:00:00Z".to_string()));
    }

    #[test]
    fn mark_running_as_failed_transitions() {
        let db = temp_db();
        let mut job = make_job("j4");
        job.status = JobStatus::Running;
        db.insert(&job);
        db.mark_running_as_failed();
        let jobs = db.load_all();
        let j = jobs.iter().find(|j| j.id == "j4").unwrap();
        assert_eq!(j.status, JobStatus::Failed);
    }

    #[test]
    fn insert_duplicate_is_ignored() {
        let db = temp_db();
        db.insert(&make_job("j5"));
        db.insert(&make_job("j5"));
        let jobs = db.load_all();
        assert_eq!(jobs.len(), 1);
    }

    #[test]
    fn str_to_status_unknown_defaults_queued() {
        assert_eq!(str_to_status("unknown"), JobStatus::Queued);
    }

    #[test]
    fn status_round_trip() {
        for status in &[
            JobStatus::Queued,
            JobStatus::Running,
            JobStatus::Done,
            JobStatus::Flagged,
            JobStatus::Failed,
        ] {
            assert_eq!(&str_to_status(status_to_str(status)), status);
        }
    }
}
