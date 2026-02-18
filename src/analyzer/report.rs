use crate::db::{ActivityRow, ChromeVisitRow};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportMetric {
    pub name: String,
    pub seconds: u64,
    pub minutes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyReport {
    pub date: String,
    pub generated_at: String,
    pub total_seconds: u64,
    pub total_minutes: u64,
    pub active_window_seconds: u64,
    pub active_window_minutes: u64,
    pub chrome_history_seconds: u64,
    pub chrome_history_minutes: u64,
    pub categories_seconds: BTreeMap<String, u64>,
    pub categories: BTreeMap<String, u64>,
    pub chrome_categories_seconds: BTreeMap<String, u64>,
    pub chrome_categories: BTreeMap<String, u64>,
    pub top_apps: Vec<ReportMetric>,
    pub top_domains: Vec<ReportMetric>,
    pub anomalies: Vec<String>,
}

#[derive(Debug)]
pub struct SavedReport {
    pub markdown_path: PathBuf,
    pub json_path: PathBuf,
}

pub fn build_daily_report(
    date: NaiveDate,
    activities: &[ActivityRow],
    domains: &[ChromeVisitRow],
) -> DailyReport {
    let generated_at: DateTime<Utc> = Utc::now();

    let activity_total_seconds = activities
        .iter()
        .map(|activity| activity.duration_sec.max(0))
        .sum::<i64>();
    let raw_domain_seconds = domains.iter().fold(HashMap::new(), |mut acc, visit| {
        let entry = acc.entry(visit.domain.clone()).or_insert(0_i64);
        *entry += visit.duration_sec.max(0);
        acc
    });
    let domain_seconds = raw_domain_seconds
        .into_iter()
        .map(|(domain, seconds)| (domain, seconds.max(0)))
        .collect::<HashMap<_, _>>();
    let domain_total_seconds = domain_seconds.values().sum::<i64>();

    let activity_category_seconds = activities.iter().fold(HashMap::new(), |mut acc, activity| {
        let entry = acc.entry(activity.category.clone()).or_insert(0_i64);
        *entry += activity.duration_sec.max(0);
        acc
    });
    let domain_category_seconds = domains.iter().fold(HashMap::new(), |mut acc, visit| {
        let entry = acc.entry(visit.category.clone()).or_insert(0_i64);
        *entry += visit.duration_sec.max(0);
        acc
    });

    let app_seconds = activities.iter().fold(HashMap::new(), |mut acc, activity| {
        let entry = acc.entry(activity.app_name.clone()).or_insert(0_i64);
        *entry += activity.duration_sec.max(0);
        acc
    });

    let categories = canonical_categories()
        .into_iter()
        .map(|category| {
            let seconds = activity_category_seconds
                .get(category)
                .copied()
                .unwrap_or_default();
            (category.to_string(), sec_to_min(seconds))
        })
        .collect::<BTreeMap<_, _>>();
    let chrome_categories = canonical_categories()
        .into_iter()
        .map(|category| {
            let seconds = domain_category_seconds
                .get(category)
                .copied()
                .unwrap_or_default();
            (category.to_string(), sec_to_min(seconds))
        })
        .collect::<BTreeMap<_, _>>();
    let categories_seconds = canonical_categories()
        .into_iter()
        .map(|category| {
            let seconds = activity_category_seconds
                .get(category)
                .copied()
                .unwrap_or_default()
                .max(0) as u64;
            (category.to_string(), seconds)
        })
        .collect::<BTreeMap<_, _>>();
    let chrome_categories_seconds = canonical_categories()
        .into_iter()
        .map(|category| {
            let seconds = domain_category_seconds
                .get(category)
                .copied()
                .unwrap_or_default()
                .max(0) as u64;
            (category.to_string(), seconds)
        })
        .collect::<BTreeMap<_, _>>();

    let top_apps = top_n_metrics(app_seconds, 5);
    let top_domains = top_n_metrics(domain_seconds, 10);

    let anomalies = detect_anomalies(
        &categories_seconds,
        &top_domains,
        activity_total_seconds.max(0) as u64,
        domain_total_seconds.max(0) as u64,
    );

    DailyReport {
        date: date.format("%Y-%m-%d").to_string(),
        generated_at: generated_at.to_rfc3339(),
        total_seconds: activity_total_seconds.max(0) as u64,
        total_minutes: sec_to_min(activity_total_seconds),
        active_window_seconds: activity_total_seconds.max(0) as u64,
        active_window_minutes: sec_to_min(activity_total_seconds),
        chrome_history_seconds: domain_total_seconds.max(0) as u64,
        chrome_history_minutes: sec_to_min(domain_total_seconds),
        categories_seconds,
        categories,
        chrome_categories_seconds,
        chrome_categories,
        top_apps,
        top_domains,
        anomalies,
    }
}

pub fn render_markdown(report: &DailyReport) -> String {
    let productivity_seconds = report
        .categories_seconds
        .get("development")
        .copied()
        .unwrap_or_default()
        + report
            .categories_seconds
            .get("research")
            .copied()
            .unwrap_or_default();

    let productivity_ratio = if report.active_window_seconds == 0 {
        0.0
    } else {
        (productivity_seconds as f64 / report.active_window_seconds as f64) * 100.0
    };

    let most_used_app = report
        .top_apps
        .first()
        .map(|metric| {
            format!(
                "{} ({})",
                metric.name,
                format_duration_seconds(metric.seconds)
            )
        })
        .unwrap_or_else(|| "None".to_string());

    let category_rows = canonical_categories()
        .iter()
        .map(|category| {
            let seconds = report
                .categories_seconds
                .get(*category)
                .copied()
                .unwrap_or_default();
            let ratio = if report.active_window_seconds == 0 {
                0.0
            } else {
                (seconds as f64 / report.active_window_seconds as f64) * 100.0
            };

            format!(
                "| {} | {} | {:.0}% |",
                localized_category_name(category),
                format_duration_seconds(seconds),
                ratio
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let app_rows = list_metrics(&report.top_apps);
    let domain_rows = list_metrics(&report.top_domains);
    let anomaly_rows = if report.anomalies.is_empty() {
        "- No notable anomaly detected".to_string()
    } else {
        report
            .anomalies
            .iter()
            .map(|entry| format!("- {entry}"))
            .collect::<Vec<_>>()
            .join("\n")
    };

    format!(
        "# Daily Activity Report - {}\n\n## Summary\n- Active window tracked time: {}\n- Chrome history tracked time: {}\n- Productivity ratio (development + research): {:.0}%\n- Most used app: {}\n\n## Time by Category (Active Window Tracking)\n| Category | Time | Ratio |\n|----------|------|-------|\n{}\n\n## Top Apps (5)\n{}\n\n## Top Domains (10, Chrome History)\n{}\n\n## Anomalies\n{}\n",
        report.date,
        format_duration_seconds(report.active_window_seconds),
        format_duration_seconds(report.chrome_history_seconds),
        productivity_ratio,
        most_used_app,
        category_rows,
        app_rows,
        domain_rows,
        anomaly_rows
    )
}

pub fn save_report_files(report: &DailyReport, report_dir: &Path) -> Result<SavedReport> {
    fs::create_dir_all(report_dir).with_context(|| {
        format!(
            "Failed to create report directory: {}",
            report_dir.display()
        )
    })?;

    let date = report.date.clone();
    let markdown_path = report_dir.join(format!("{date}.md"));
    let json_path = report_dir.join(format!("{date}.json"));

    fs::write(&markdown_path, render_markdown(report)).with_context(|| {
        format!(
            "Failed to write Markdown report: {}",
            markdown_path.display()
        )
    })?;

    let json_content =
        serde_json::to_string_pretty(report).context("Failed to serialize report JSON")?;
    fs::write(&json_path, json_content)
        .with_context(|| format!("Failed to write JSON report: {}", json_path.display()))?;

    Ok(SavedReport {
        markdown_path,
        json_path,
    })
}

fn canonical_categories() -> Vec<&'static str> {
    vec![
        "development",
        "research",
        "communication",
        "entertainment",
        "sns",
        "shopping",
        "other",
    ]
}

fn localized_category_name(raw: &str) -> &'static str {
    match raw {
        "development" => "Development",
        "research" => "Research",
        "communication" => "Communication",
        "entertainment" => "Entertainment",
        "sns" => "SNS",
        "shopping" => "Shopping",
        _ => "Other",
    }
}

fn top_n_metrics(source: HashMap<String, i64>, n: usize) -> Vec<ReportMetric> {
    let mut items = source
        .into_iter()
        .map(|(name, seconds)| ReportMetric {
            name,
            seconds: seconds.max(0) as u64,
            minutes: sec_to_min(seconds),
        })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        right
            .seconds
            .cmp(&left.seconds)
            .then_with(|| left.name.cmp(&right.name))
    });
    items.into_iter().take(n).collect()
}

fn detect_anomalies(
    categories_seconds: &BTreeMap<String, u64>,
    top_domains: &[ReportMetric],
    active_window_seconds: u64,
    chrome_history_seconds: u64,
) -> Vec<String> {
    let entertainment_seconds = categories_seconds
        .get("entertainment")
        .copied()
        .unwrap_or_default();

    let entertainment_alert = (entertainment_seconds >= 90 * 60).then_some(format!(
        "Entertainment usage is high: {}",
        format_duration_seconds(entertainment_seconds)
    ));

    let youtube_alert = top_domains
        .iter()
        .find(|metric| metric.name.contains("youtube.com") && metric.seconds >= 60 * 60)
        .map(|metric| {
            format!(
                "YouTube session was unusually long: {}",
                format_duration_seconds(metric.seconds)
            )
        });

    let low_activity_alert = (active_window_seconds < 60 * 60)
        .then_some("Total tracked time is below 1 hour".to_string());

    let overlap_hint =
        (chrome_history_seconds > active_window_seconds && chrome_history_seconds > 0).then_some(
            "Chrome history durations can overlap across tabs, so web time may exceed active window time".to_string(),
        );

    [
        entertainment_alert,
        youtube_alert,
        low_activity_alert,
        overlap_hint,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
}

fn list_metrics(metrics: &[ReportMetric]) -> String {
    if metrics.is_empty() {
        return "- No data".to_string();
    }

    metrics
        .iter()
        .enumerate()
        .map(|(index, metric)| {
            format!(
                "{}. {} - {}",
                index + 1,
                metric.name,
                format_duration_seconds(metric.seconds)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn sec_to_min(seconds: i64) -> u64 {
    let safe_seconds = seconds.max(0) as u64;
    safe_seconds / 60
}

fn format_duration_seconds(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let remain_seconds = seconds % 60;

    if hours > 0 {
        if remain_seconds == 0 {
            format!("{hours}h {minutes}m")
        } else {
            format!("{hours}h {minutes}m {remain_seconds}s")
        }
    } else if minutes > 0 {
        if remain_seconds == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m {remain_seconds}s")
        }
    } else {
        format!("{remain_seconds}s")
    }
}
