pub mod queries;

use anyhow::{Context, Result};
use chrono::{Duration, Local, NaiveDate, TimeZone};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct ActivityRow {
    pub id: i64,
    pub recorded_at: i64,
    pub app_name: String,
    pub window_title: Option<String>,
    pub category: String,
    pub duration_sec: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChromeVisitRow {
    pub id: i64,
    pub date: String,
    pub domain: String,
    pub category: String,
    pub duration_sec: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportMetaRow {
    pub id: i64,
    pub date: String,
    pub generated_at: i64,
    pub md_path: String,
    pub json_path: String,
}

#[derive(Debug, Clone)]
pub struct ChromeVisitInput {
    pub domain: String,
    pub category: String,
    pub duration_sec: i64,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create DB directory: {}", parent.display()))?;
        }

        let conn = Connection::open(path)
            .with_context(|| format!("Failed to open SQLite DB: {}", path.display()))?;

        let database = Self { conn };
        database.init_schema()?;

        Ok(database)
    }

    pub fn init_schema(&self) -> Result<()> {
        queries::schema_statements()
            .iter()
            .try_for_each(|statement| {
                self.conn
                    .execute(statement, [])
                    .context("Failed to initialize schema")
                    .map(|_| ())
            })
    }

    pub fn insert_activity(
        &self,
        recorded_at: i64,
        app_name: &str,
        window_title: Option<&str>,
        category: &str,
        duration_sec: i64,
    ) -> Result<()> {
        self.conn
            .execute(
                "INSERT INTO activities (recorded_at, app_name, window_title, category, duration_sec) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![recorded_at, app_name, window_title, category, duration_sec],
            )
            .context("Failed to insert activity")?;

        Ok(())
    }

    pub fn latest_activity_timestamp(&self) -> Result<Option<i64>> {
        let timestamp = self
            .conn
            .query_row(
                "SELECT recorded_at FROM activities ORDER BY recorded_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();

        Ok(timestamp)
    }

    pub fn activities_between(&self, from_ts: i64, to_ts: i64) -> Result<Vec<ActivityRow>> {
        let mut statement = self.conn.prepare(
            "SELECT id, recorded_at, app_name, window_title, category, duration_sec
             FROM activities
             WHERE recorded_at >= ?1 AND recorded_at <= ?2
             ORDER BY recorded_at ASC",
        )?;

        let rows = statement
            .query_map(params![from_ts, to_ts], |row| {
                Ok(ActivityRow {
                    id: row.get(0)?,
                    recorded_at: row.get(1)?,
                    app_name: row.get(2)?,
                    window_title: row.get(3)?,
                    category: row.get(4)?,
                    duration_sec: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to query activities")?;

        Ok(rows)
    }

    pub fn activities_for_date(&self, date: NaiveDate) -> Result<Vec<ActivityRow>> {
        let from = date
            .and_hms_opt(0, 0, 0)
            .context("Failed to build from timestamp")?;
        let to = (date + Duration::days(1))
            .and_hms_opt(0, 0, 0)
            .context("Failed to build to timestamp")?;

        let from_ts = Local
            .from_local_datetime(&from)
            .single()
            .context("Failed to convert from timestamp to local time")?
            .timestamp();
        let to_ts = Local
            .from_local_datetime(&to)
            .single()
            .context("Failed to convert to timestamp to local time")?
            .timestamp()
            - 1;

        self.activities_between(from_ts, to_ts)
    }

    pub fn replace_chrome_visits_for_date(
        &mut self,
        date: NaiveDate,
        visits: &[ChromeVisitInput],
    ) -> Result<()> {
        let date_str = date.format("%Y-%m-%d").to_string();
        let transaction = self
            .conn
            .transaction()
            .context("Failed to start transaction")?;

        transaction
            .execute(
                "DELETE FROM chrome_visits WHERE date = ?1",
                params![&date_str],
            )
            .context("Failed to delete existing Chrome visits")?;

        visits.iter().try_for_each(|visit| {
            transaction
                .execute(
                    "INSERT INTO chrome_visits (date, domain, category, duration_sec) VALUES (?1, ?2, ?3, ?4)",
                    params![&date_str, &visit.domain, &visit.category, visit.duration_sec],
                )
                .context("Failed to insert Chrome visit")
                .map(|_| ())
        })?;

        transaction
            .commit()
            .context("Failed to commit Chrome visits")?;
        Ok(())
    }

    pub fn chrome_visits_for_date(&self, date: NaiveDate) -> Result<Vec<ChromeVisitRow>> {
        let date_str = date.format("%Y-%m-%d").to_string();
        let mut statement = self.conn.prepare(
            "SELECT id, date, domain, category, duration_sec
             FROM chrome_visits
             WHERE date = ?1
             ORDER BY duration_sec DESC",
        )?;

        let rows = statement
            .query_map(params![date_str], |row| {
                Ok(ChromeVisitRow {
                    id: row.get(0)?,
                    date: row.get(1)?,
                    domain: row.get(2)?,
                    category: row.get(3)?,
                    duration_sec: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to query Chrome visits")?;

        Ok(rows)
    }

    pub fn report_meta(&self, date: NaiveDate) -> Result<Option<ReportMetaRow>> {
        let date_str = date.format("%Y-%m-%d").to_string();
        let row = self
            .conn
            .query_row(
                "SELECT id, date, generated_at, md_path, json_path FROM reports WHERE date = ?1",
                params![date_str],
                |row| {
                    Ok(ReportMetaRow {
                        id: row.get(0)?,
                        date: row.get(1)?,
                        generated_at: row.get(2)?,
                        md_path: row.get(3)?,
                        json_path: row.get(4)?,
                    })
                },
            )
            .ok();

        Ok(row)
    }

    pub fn latest_report_meta(&self) -> Result<Option<ReportMetaRow>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, date, generated_at, md_path, json_path FROM reports ORDER BY date DESC LIMIT 1",
                [],
                |row| {
                    Ok(ReportMetaRow {
                        id: row.get(0)?,
                        date: row.get(1)?,
                        generated_at: row.get(2)?,
                        md_path: row.get(3)?,
                        json_path: row.get(4)?,
                    })
                },
            )
            .ok();

        Ok(row)
    }

    pub fn list_reports(&self, limit: usize) -> Result<Vec<ReportMetaRow>> {
        let mut statement = self.conn.prepare(
            "SELECT id, date, generated_at, md_path, json_path
             FROM reports
             ORDER BY date DESC
             LIMIT ?1",
        )?;

        let rows = statement
            .query_map(params![limit as i64], |row| {
                Ok(ReportMetaRow {
                    id: row.get(0)?,
                    date: row.get(1)?,
                    generated_at: row.get(2)?,
                    md_path: row.get(3)?,
                    json_path: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to list reports")?;

        Ok(rows)
    }

    pub fn upsert_report_meta(
        &self,
        date: NaiveDate,
        generated_at: i64,
        md_path: &str,
        json_path: &str,
    ) -> Result<()> {
        let date_str = date.format("%Y-%m-%d").to_string();
        self.conn
            .execute(
                "INSERT INTO reports (date, generated_at, md_path, json_path)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(date)
                 DO UPDATE SET generated_at=excluded.generated_at, md_path=excluded.md_path, json_path=excluded.json_path",
                params![date_str, generated_at, md_path, json_path],
            )
            .context("Failed to upsert report metadata")?;

        Ok(())
    }

    pub fn cleanup_old_activities(&self, retention_days: u32) -> Result<usize> {
        let threshold = (Local::now() - Duration::days(i64::from(retention_days))).timestamp();

        let deleted = self
            .conn
            .execute(
                "DELETE FROM activities WHERE recorded_at < ?1",
                params![threshold],
            )
            .context("Failed to clean up old activities")?;

        Ok(deleted)
    }
}
