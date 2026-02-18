pub const CREATE_ACTIVITIES: &str = r#"
CREATE TABLE IF NOT EXISTS activities (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  recorded_at  INTEGER NOT NULL,
  app_name     TEXT NOT NULL,
  window_title TEXT,
  category     TEXT NOT NULL DEFAULT 'other',
  duration_sec INTEGER NOT NULL DEFAULT 0
);
"#;

pub const CREATE_CHROME_VISITS: &str = r#"
CREATE TABLE IF NOT EXISTS chrome_visits (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  date         TEXT NOT NULL,
  domain       TEXT NOT NULL,
  category     TEXT NOT NULL DEFAULT 'other',
  duration_sec INTEGER NOT NULL DEFAULT 0
);
"#;

pub const CREATE_REPORTS: &str = r#"
CREATE TABLE IF NOT EXISTS reports (
  id           INTEGER PRIMARY KEY AUTOINCREMENT,
  date         TEXT NOT NULL UNIQUE,
  generated_at INTEGER NOT NULL,
  md_path      TEXT NOT NULL,
  json_path    TEXT NOT NULL
);
"#;

pub const INDEX_ACTIVITIES_RECORDED_AT: &str =
    "CREATE INDEX IF NOT EXISTS idx_activities_recorded_at ON activities(recorded_at);";

pub const INDEX_CHROME_VISITS_DATE: &str =
    "CREATE INDEX IF NOT EXISTS idx_chrome_visits_date ON chrome_visits(date);";

pub const INDEX_REPORTS_DATE: &str =
    "CREATE INDEX IF NOT EXISTS idx_reports_date ON reports(date);";

pub fn schema_statements() -> Vec<&'static str> {
    vec![
        CREATE_ACTIVITIES,
        CREATE_CHROME_VISITS,
        CREATE_REPORTS,
        INDEX_ACTIVITIES_RECORDED_AT,
        INDEX_CHROME_VISITS_DATE,
        INDEX_REPORTS_DATE,
    ]
}
