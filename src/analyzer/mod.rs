pub mod categorizer;
pub mod report;

use crate::analyzer::report::{DailyReport, SavedReport};
use crate::config::Config;
use crate::db::Database;
use anyhow::Result;
use chrono::{NaiveDate, Utc};
use std::collections::HashSet;

pub fn generate_and_store_report(
    config: &Config,
    date: NaiveDate,
    ai_insights: Vec<String>,
) -> Result<(DailyReport, SavedReport)> {
    let database = Database::open(&config.db_path)?;
    let activities = database.activities_for_date(date)?;
    let domains = database.chrome_visits_for_date(date)?;

    let mut report = report::build_daily_report(date, &activities, &domains);

    let mut seen = HashSet::new();
    report.anomalies = report
        .anomalies
        .into_iter()
        .chain(
            ai_insights
                .into_iter()
                .map(|entry| format!("AI insight: {entry}")),
        )
        .filter(|entry| seen.insert(entry.clone()))
        .collect::<Vec<_>>();

    let saved = report::save_report_files(&report, &config.report_dir)?;

    database.upsert_report_meta(
        date,
        Utc::now().timestamp(),
        &saved.markdown_path.display().to_string(),
        &saved.json_path.display().to_string(),
    )?;

    Ok((report, saved))
}
