use anyhow::{Context, Result, anyhow, bail};
use chrono::NaiveTime;
use dirs::home_dir;
use serde::{Deserialize, Serialize};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const APP_DIR: &str = ".OpenTracker";
const CONFIG_FILE: &str = "config.json";
const CATEGORIES_FILE: &str = "categories.json";
const DEFAULT_REPORT_TIME: &str = "23:30";
pub const FIXED_POLLING_SECONDS: u64 = 300;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub polling_seconds: u64,
    pub chrome_profiles: Vec<String>,
    pub report_time: String,
    pub report_dir: PathBuf,
    pub categories_path: PathBuf,
    pub db_path: PathBuf,
    pub api_port: u16,
    pub retention_days: u32,
    pub notify_on_report: bool,
    pub daemon_label: String,
    pub ai_enabled: bool,
    pub ai_api_key: Option<String>,
    pub ai_api_base_url: String,
    pub ai_model: String,
    pub ai_timeout_seconds: u64,
}

impl Default for Config {
    fn default() -> Self {
        let root = default_root_dir();
        let report_dir = default_report_dir();

        Self {
            polling_seconds: FIXED_POLLING_SECONDS,
            chrome_profiles: vec!["Default".to_string()],
            report_time: DEFAULT_REPORT_TIME.to_string(),
            report_dir,
            categories_path: root.join(CATEGORIES_FILE),
            db_path: root.join("db").join("activity.db"),
            api_port: 7890,
            retention_days: 90,
            notify_on_report: true,
            daemon_label: "com.OpenTracker.daemon".to_string(),
            ai_enabled: true,
            ai_api_key: None,
            ai_api_base_url: "https://api.openai.com/v1".to_string(),
            ai_model: "gpt-4o-mini".to_string(),
            ai_timeout_seconds: 20,
        }
    }
}

impl Config {
    pub fn root_dir() -> Result<PathBuf> {
        Ok(default_root_dir())
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(default_root_dir().join(CONFIG_FILE))
    }

    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        let content = fs::read_to_string(&config_path)
            .with_context(|| format!("Failed to read config file: {}", config_path.display()))?;

        let mut config: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse config file: {}", config_path.display()))?;
        config.polling_seconds = FIXED_POLLING_SECONDS;

        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        if let Some(parent) = config_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create config directory: {}", parent.display())
            })?;
        }

        let content = serde_json::to_string_pretty(self).context("Failed to serialize config")?;
        fs::write(&config_path, content)
            .with_context(|| format!("Failed to write config file: {}", config_path.display()))?;
        set_mode_600(&config_path)?;

        Ok(())
    }

    pub fn ensure_bootstrap_files(&self) -> Result<()> {
        let root = Self::root_dir()?;
        fs::create_dir_all(&root)
            .with_context(|| format!("Failed to create root directory: {}", root.display()))?;

        if let Some(parent) = self.db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create DB directory: {}", parent.display()))?;
        }

        fs::create_dir_all(&self.report_dir).with_context(|| {
            format!(
                "Failed to create report directory: {}",
                self.report_dir.as_path().display()
            )
        })?;

        if !self.categories_path.exists() {
            fs::write(
                &self.categories_path,
                include_str!("../assets/categories.json"),
            )
            .with_context(|| {
                format!(
                    "Failed to create default categories file: {}",
                    self.categories_path.display()
                )
            })?;
            set_mode_600(&self.categories_path)?;
        }

        Ok(())
    }

    pub fn parse_report_time(&self) -> Result<NaiveTime> {
        parse_hhmm(&self.report_time)
    }

    pub fn set_value(&mut self, key: &str, value: &str) -> Result<()> {
        let normalized = normalize_config_key(key);

        match normalized {
            "polling_seconds" => {
                let parsed = value
                    .parse::<u64>()
                    .map_err(|_| anyhow!("polling_seconds must be a number"))?;
                if parsed != FIXED_POLLING_SECONDS {
                    bail!("polling_seconds is fixed to 300 seconds (5 minutes)");
                }
                self.polling_seconds = parsed;
            }
            "report_time" => {
                parse_hhmm(value)?;
                self.report_time = value.to_string();
            }
            "report_dir" => {
                self.report_dir = expand_home(value);
            }
            "chrome_profiles" => {
                let profiles = value
                    .split(',')
                    .map(str::trim)
                    .filter(|part| !part.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();

                if profiles.is_empty() {
                    bail!("chrome_profiles requires at least one profile");
                }
                self.chrome_profiles = profiles;
            }
            "api_port" => {
                self.api_port = value
                    .parse::<u16>()
                    .map_err(|_| anyhow!("api_port must be a number"))?;
            }
            "retention_days" => {
                self.retention_days = value
                    .parse::<u32>()
                    .map_err(|_| anyhow!("retention_days must be a number"))?;
            }
            "notify_on_report" => {
                self.notify_on_report = value
                    .parse::<bool>()
                    .map_err(|_| anyhow!("notify_on_report must be true/false"))?;
            }
            "ai_enabled" => {
                self.ai_enabled = value
                    .parse::<bool>()
                    .map_err(|_| anyhow!("ai_enabled must be true/false"))?;
            }
            "ai_api_key" => {
                self.ai_api_key = (!value.trim().is_empty()).then_some(value.to_string());
            }
            "ai_api_base_url" => {
                self.ai_api_base_url = value.trim().trim_end_matches('/').to_string();
            }
            "ai_model" => {
                self.ai_model = value.trim().to_string();
            }
            "ai_timeout_seconds" => {
                self.ai_timeout_seconds = value
                    .parse::<u64>()
                    .map_err(|_| anyhow!("ai_timeout_seconds must be a number"))?
                    .max(5);
            }
            _ => {
                bail!(
                    "Unsupported config key: {key}. Supported keys: polling_seconds|collector.interval_seconds, report_time|report.time, report_dir|report.dir, chrome_profiles|chrome.profiles, api_port|api.port, retention_days|retention.days, notify_on_report|report.notify, ai_enabled|ai.enabled, ai_api_key|ai.api_key, ai_api_base_url|ai.base_url, ai_model|ai.model, ai_timeout_seconds|ai.timeout_seconds"
                );
            }
        }

        if normalized == "report_dir" {
            fs::create_dir_all(&self.report_dir).with_context(|| {
                format!(
                    "Failed to create report directory: {}",
                    self.report_dir.display()
                )
            })?;
        }

        Ok(())
    }

    pub fn get_value(&self, key: &str) -> Option<String> {
        match normalize_config_key(key) {
            "polling_seconds" => Some(self.polling_seconds.to_string()),
            "report_time" => Some(self.report_time.clone()),
            "report_dir" => Some(self.report_dir.display().to_string()),
            "categories_path" => Some(self.categories_path.display().to_string()),
            "db_path" => Some(self.db_path.display().to_string()),
            "chrome_profiles" => Some(self.chrome_profiles.join(",")),
            "api_port" => Some(self.api_port.to_string()),
            "retention_days" => Some(self.retention_days.to_string()),
            "notify_on_report" => Some(self.notify_on_report.to_string()),
            "daemon_label" => Some(self.daemon_label.clone()),
            "ai_enabled" => Some(self.ai_enabled.to_string()),
            "ai_api_key" => Some(
                self.ai_api_key
                    .as_ref()
                    .map(|_| "***set***".to_string())
                    .unwrap_or_else(|| "not_set".to_string()),
            ),
            "ai_api_base_url" => Some(self.ai_api_base_url.clone()),
            "ai_model" => Some(self.ai_model.clone()),
            "ai_timeout_seconds" => Some(self.ai_timeout_seconds.to_string()),
            _ => None,
        }
    }
}

fn normalize_config_key(key: &str) -> &str {
    match key {
        "polling_seconds" | "collector.interval_seconds" => "polling_seconds",
        "report_time" | "report.time" => "report_time",
        "report_dir" | "report.dir" => "report_dir",
        "chrome_profiles" | "chrome.profiles" => "chrome_profiles",
        "api_port" | "api.port" => "api_port",
        "retention_days" | "retention.days" => "retention_days",
        "notify_on_report" | "report.notify" => "notify_on_report",
        "ai_enabled" | "ai.enabled" => "ai_enabled",
        "ai_api_key" | "ai.api_key" => "ai_api_key",
        "ai_api_base_url" | "ai.base_url" => "ai_api_base_url",
        "ai_model" | "ai.model" => "ai_model",
        "ai_timeout_seconds" | "ai.timeout_seconds" => "ai_timeout_seconds",
        "categories_path" | "categories.path" => "categories_path",
        "db_path" | "db.path" => "db_path",
        "daemon_label" | "daemon.label" => "daemon_label",
        _ => key,
    }
}

pub fn parse_hhmm(value: &str) -> Result<NaiveTime> {
    NaiveTime::parse_from_str(value, "%H:%M")
        .with_context(|| format!("Invalid time format: {value}. Example: 23:30 (24-hour format)",))
}

pub fn expand_home(raw: &str) -> PathBuf {
    raw.strip_prefix("~/")
        .and_then(|stripped| home_dir().map(|home| home.join(stripped)))
        .unwrap_or_else(|| PathBuf::from(raw))
}

pub fn default_report_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Documents")
        .join("OpenTracker")
        .join("reports")
}

fn default_root_dir() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

fn set_mode_600(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("Failed to set file permissions: {}", path.display()))?;
    }

    Ok(())
}
