use crate::analyzer::categorizer::CategoryRules;
use crate::config::Config;
use crate::db::ChromeVisitInput;
use anyhow::{Context, Result};
use chrono::NaiveDate;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use url::Url;

pub fn sync_chrome_visits_for_date(
    config: &Config,
    rules: &CategoryRules,
    date: NaiveDate,
) -> Result<Vec<ChromeVisitInput>> {
    let entries = config
        .chrome_profiles
        .iter()
        .map(|profile| profile_history_path(profile))
        .filter(|path| path.exists())
        .map(|path| collect_profile_visits(path.as_path(), date))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .fold(HashMap::new(), |mut acc, (domain, duration_sec)| {
            let entry = acc.entry(domain).or_insert(0_i64);
            *entry += duration_sec;
            acc
        });

    let visits = entries
        .into_iter()
        .map(|(domain, duration_sec)| ChromeVisitInput {
            category: rules.categorize_domain(&domain),
            domain,
            duration_sec,
        })
        .collect::<Vec<_>>();

    Ok(visits)
}

pub fn profile_history_path(profile: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library")
        .join("Application Support")
        .join("Google")
        .join("Chrome")
        .join(profile)
        .join("History")
}

fn collect_profile_visits(path: &Path, date: NaiveDate) -> Result<Vec<(String, i64)>> {
    let temp_dir = std::env::temp_dir().join(format!(
        "OpenTracker-chrome-{}",
        chrono::Utc::now().timestamp_millis()
    ));

    fs::create_dir_all(&temp_dir).with_context(|| {
        format!(
            "Failed to create Chrome temp directory: {}",
            temp_dir.display()
        )
    })?;

    let temp_db = temp_dir.join("History");
    fs::copy(path, &temp_db).with_context(|| {
        format!(
            "Failed to copy Chrome History DB: {} -> {}",
            path.display(),
            temp_db.display()
        )
    })?;

    let result = read_history_visits(&temp_db, date);
    let _ = fs::remove_dir_all(&temp_dir);

    result
}

fn read_history_visits(path: &Path, date: NaiveDate) -> Result<Vec<(String, i64)>> {
    let conn = Connection::open(path).with_context(|| {
        format!(
            "Failed to open temporary Chrome History DB: {}",
            path.display()
        )
    })?;

    let query = r#"
        SELECT urls.url, COALESCE(visits.visit_duration, 0)
        FROM visits
        JOIN urls ON visits.url = urls.id
        WHERE DATE(datetime((visits.visit_time / 1000000) - 11644473600, 'unixepoch', 'localtime')) = ?1
    "#;

    let date_str = date.format("%Y-%m-%d").to_string();
    let mut statement = conn
        .prepare(query)
        .context("Failed to prepare Chrome History query")?;

    let rows = statement
        .query_map(params![date_str], |row| {
            let raw_url: String = row.get(0)?;
            let duration_raw: i64 = row.get(1)?;

            Ok((raw_url, duration_raw))
        })?
        .collect::<Result<Vec<_>, _>>()
        .context("Failed to read Chrome History rows")?;

    let visits = rows
        .into_iter()
        .filter_map(|(raw_url, duration_raw)| {
            extract_domain(&raw_url).map(|domain| {
                let seconds = (duration_raw / 1_000_000).max(0);
                (domain, seconds)
            })
        })
        .filter(|(_, seconds)| *seconds > 0)
        .collect::<Vec<_>>();

    Ok(visits)
}

fn extract_domain(raw_url: &str) -> Option<String> {
    Url::parse(raw_url)
        .ok()
        .and_then(|url| url.host_str().map(ToOwned::to_owned))
        .map(|host| host.trim_start_matches("www.").to_lowercase())
}

pub fn detect_chrome_profiles() -> Vec<String> {
    let chrome_root = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library")
        .join("Application Support")
        .join("Google")
        .join("Chrome");

    fs::read_dir(chrome_root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            if path.is_dir() && path.join("History").exists() {
                path.file_name()
                    .map(|name| name.to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
}
