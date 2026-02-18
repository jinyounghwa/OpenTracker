mod ai;
mod analyzer;
mod api;
mod cli;
mod collector;
mod config;
mod daemon;
mod db;
mod scheduler;

use crate::analyzer::categorizer::CategoryRules;
use crate::cli::onboard::run_onboarding;
use crate::cli::{AiCommands, Cli, Commands, ConfigCommands};
use crate::collector::chrome;
use crate::config::{Config, FIXED_POLLING_SECONDS};
use crate::db::Database;
use anyhow::{Context, Result, bail};
use chrono::{Local, NaiveDate};
use clap::Parser;
use std::fs;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tokio::signal;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use url::Url;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse()?))
        .with_target(false)
        .compact()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Onboard { install_daemon } => {
            let _ = run_onboarding(install_daemon)?;
            Ok(())
        }
        Commands::Config { command } => handle_config_command(command),
        Commands::Status => handle_status(),
        Commands::Doctor => handle_doctor(),
        Commands::Start => handle_start().await,
        Commands::Stop => handle_stop(),
        Commands::Restart => handle_restart(),
        Commands::Dashboard => handle_dashboard(),
        Commands::Report { date } => handle_report(date),
        Commands::Ai { command } => handle_ai_command(command),
        Commands::Service => {
            let config = load_config()?;
            run_service(config).await
        }
        Commands::Update => {
            println!("Update to latest version: cargo install --path . --force");
            Ok(())
        }
        Commands::Uninstall => handle_uninstall(),
    }
}

fn handle_config_command(command: ConfigCommands) -> Result<()> {
    match command {
        ConfigCommands::Set { key, value } => {
            let mut config = load_or_default_config()?;
            config.set_value(&key, &value)?;
            config.ensure_bootstrap_files()?;
            config.save()?;

            let masked = if key.contains("api_key") {
                "***hidden***".to_string()
            } else {
                value
            };
            println!("Config saved: {key} = {masked}");
            Ok(())
        }
        ConfigCommands::Get { key } => {
            let config = load_config()?;
            let value = config
                .get_value(&key)
                .with_context(|| format!("Unsupported config key: {key}"))?;

            println!("{value}");
            Ok(())
        }
    }
}

fn handle_status() -> Result<()> {
    let config = load_config()?;
    let database = Database::open(&config.db_path)?;
    let daemon_status = daemon::status(&config)?;

    println!("OpenTracker status");
    println!("- daemon_label: {}", config.daemon_label);
    println!("- daemon_installed: {}", daemon_status.installed);
    println!("- daemon_loaded: {}", daemon_status.loaded);
    println!(
        "- last_collected_at: {}",
        database
            .latest_activity_timestamp()?
            .map(|timestamp| timestamp.to_string())
            .unwrap_or_else(|| "none".to_string())
    );
    println!(
        "- latest_report_date: {}",
        database
            .latest_report_meta()?
            .map(|meta| meta.date)
            .unwrap_or_else(|| "none".to_string())
    );

    Ok(())
}

fn handle_doctor() -> Result<()> {
    let config_path = Config::config_path()?;
    let mut issues = Vec::new();

    if config_path.exists() {
        println!("[OK] config.json found: {}", config_path.display());
    } else {
        println!("[WARN] config.json not found: {}", config_path.display());
        issues.push("config missing".to_string());
    }

    let config = load_or_default_config()?;

    match Database::open(&config.db_path) {
        Ok(_) => println!("[OK] SQLite reachable: {}", config.db_path.display()),
        Err(error) => {
            println!("[WARN] SQLite check failed: {error}");
            issues.push("db unreachable".to_string());
        }
    }

    if config.report_dir.exists() {
        println!("[OK] report dir exists: {}", config.report_dir.display());
    } else {
        println!("[WARN] report dir missing: {}", config.report_dir.display());
        issues.push("report dir missing".to_string());
    }

    if let Err(error) = config.parse_report_time() {
        println!("[WARN] invalid report_time setting: {error}");
        issues.push("invalid report_time".to_string());
    } else {
        println!("[OK] report_time format valid: {}", config.report_time);
    }

    if config.ai_enabled {
        if ai::has_api_key(&config) {
            println!("[OK] AI API key is configured");
        } else {
            println!("[WARN] AI is enabled but API key is missing");
            issues.push("ai api key missing".to_string());
        }
    } else {
        println!("[OK] AI feature disabled");
    }

    let window_access = collector::window::accessibility_window_access_available();
    if window_access {
        println!("[OK] window title collection available (Accessibility likely granted)");
    } else {
        println!("[WARN] window title collection unavailable (Accessibility may be missing)");
        issues.push("accessibility missing".to_string());
    }

    let missing_profiles = config
        .chrome_profiles
        .iter()
        .filter(|profile| !chrome::profile_history_path(profile).exists())
        .cloned()
        .collect::<Vec<_>>();

    if missing_profiles.is_empty() {
        println!("[OK] Chrome profile paths verified");
    } else {
        println!(
            "[WARN] Missing Chrome profile paths: {}",
            missing_profiles.join(", ")
        );
        issues.push("chrome profile missing".to_string());
    }

    if issues.is_empty() {
        println!("doctor result: no issues");
    } else {
        println!("doctor result: {} warning(s)", issues.len());
    }

    Ok(())
}

async fn handle_start() -> Result<()> {
    let config = load_config()?;
    let daemon_status = daemon::status(&config)?;

    if daemon_status.installed {
        daemon::load(&config)?;
        println!("launchd daemon started");
        Ok(())
    } else {
        println!("launchd daemon is not installed. Running foreground service (Ctrl+C to stop).");
        run_service(config).await
    }
}

fn handle_stop() -> Result<()> {
    let config = load_config()?;
    daemon::unload(&config)?;
    println!("launchd daemon stopped");
    Ok(())
}

fn handle_restart() -> Result<()> {
    let config = load_config()?;
    daemon::restart(&config)?;
    println!("launchd daemon restarted");
    Ok(())
}

fn handle_dashboard() -> Result<()> {
    let config = load_or_default_config()?;
    ensure_dashboard_backend(&config)?;
    let url = format!("http://127.0.0.1:{}", config.api_port);

    #[cfg(target_os = "macos")]
    {
        Command::new("open")
            .arg(&url)
            .status()
            .context("Failed to open browser")?;
    }

    println!("Dashboard URL: {url}");
    Ok(())
}

fn handle_report(date: Option<String>) -> Result<()> {
    let config = load_config()?;
    let target_date = parse_optional_date(date)?;

    run_daily_pipeline(&config, target_date)
}

fn handle_ai_command(command: AiCommands) -> Result<()> {
    match command {
        AiCommands::Test {
            key,
            base_url,
            model,
        } => {
            let mut config = load_or_default_config()?;

            if let Some(value) = key {
                config.ai_api_key = Some(value);
            }
            if let Some(value) = base_url {
                config.ai_api_base_url = value;
            }
            if let Some(value) = model {
                config.ai_model = value;
            }

            let response = ai::test_connection(&config)?;
            println!("AI API connection successful");
            println!("{response}");

            Ok(())
        }
    }
}

fn handle_uninstall() -> Result<()> {
    let config = load_or_default_config()?;

    let _ = daemon::unload(&config);

    if let Ok(plist_path) = daemon::plist_path(&config) {
        if plist_path.exists() {
            let _ = fs::remove_file(&plist_path);
            println!("Removed daemon plist: {}", plist_path.display());
        }
    }

    println!("Remove binary: cargo uninstall opentracker");
    println!("Remove data (optional): rm -rf ~/.OpenTracker ~/Documents/OpenTracker/reports");

    Ok(())
}

async fn run_service(config: Config) -> Result<()> {
    config.ensure_bootstrap_files()?;
    let _ = Database::open(&config.db_path)?;

    let shared_config = Arc::new(config);
    let collector_config = Arc::clone(&shared_config);
    let collector_rules = Arc::new(load_category_rules(&shared_config)?);

    let scheduler_config = Arc::clone(&shared_config);
    let scheduler_schedule_fallback = Arc::clone(&shared_config);

    let api_config = Arc::clone(&shared_config);

    info!("OpenTracker service started");

    tokio::select! {
        collector_result = collector::run_activity_collector(collector_config, collector_rules) => {
            collector_result?;
        }
        scheduler_result = scheduler::run_cron_scheduler(move || {
            let report_time = Config::load()
                .map(|runtime| runtime.report_time)
                .unwrap_or_else(|_| scheduler_schedule_fallback.report_time.clone());

            scheduler::cron_from_report_time(&report_time)
        }, move |date| {
            let config = Arc::clone(&scheduler_config);
            async move {
                let runtime_config = Config::load().unwrap_or_else(|_| (*config).clone());
                run_daily_pipeline(&runtime_config, date)
            }
        }) => {
            scheduler_result?;
        }
        api_result = api::run_server(api_config) => {
            api_result?;
        }
        _ = signal::ctrl_c() => {
            info!("shutdown signal received");
        }
    }

    Ok(())
}

fn run_daily_pipeline(config: &Config, date: NaiveDate) -> Result<()> {
    let rules = load_category_rules(config)?;
    let visits = chrome::sync_chrome_visits_for_date(config, &rules, date)?;
    let enrichment = ai::enrich_chrome_visits(config, date, &visits).unwrap_or_else(|error| {
        warn!(error = %error, "AI enrichment failed. fallback to rule-based categorization");
        ai::AiEnrichment {
            visits: visits.clone(),
            insights: Vec::new(),
        }
    });
    let mut database = Database::open(&config.db_path)?;
    database.replace_chrome_visits_for_date(date, &enrichment.visits)?;

    let (report, saved) = analyzer::generate_and_store_report(config, date, enrichment.insights)?;

    if config.notify_on_report {
        send_macos_notification(&report.date, &saved.markdown_path);
    }

    println!("Report generated: {}", report.date);
    println!("- Markdown: {}", saved.markdown_path.display());
    println!("- JSON: {}", saved.json_path.display());

    Ok(())
}

fn parse_optional_date(input: Option<String>) -> Result<NaiveDate> {
    input
        .as_deref()
        .map(|date| {
            NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .with_context(|| format!("Invalid date format: {date}. Example: 2026-02-18"))
        })
        .transpose()?
        .map_or_else(|| Ok(Local::now().date_naive()), Ok)
}

fn load_category_rules(config: &Config) -> Result<CategoryRules> {
    CategoryRules::load(&config.categories_path).with_context(|| {
        format!(
            "Failed to load category rules: {}",
            config.categories_path.display()
        )
    })
}

fn load_or_default_config() -> Result<Config> {
    Config::load().or_else(|_| {
        let config = Config::default();
        config.ensure_bootstrap_files()?;
        config.save()?;
        Ok(config)
    })
}

fn load_config() -> Result<Config> {
    let mut config = Config::load()
        .with_context(|| "Config file not found. Run `OpenTracker onboard` first.".to_string())?;

    if config.polling_seconds != FIXED_POLLING_SECONDS {
        config.polling_seconds = FIXED_POLLING_SECONDS;
    }

    Ok(config)
}

#[cfg(target_os = "macos")]
fn send_macos_notification(date: &str, report_file_path: &std::path::Path) {
    let body = format!("Report {} is ready.", date);
    let absolute_report_path = if report_file_path.is_absolute() {
        report_file_path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(report_file_path))
            .unwrap_or_else(|_| report_file_path.to_path_buf())
    };
    let report_dir = absolute_report_path
        .parent()
        .unwrap_or(absolute_report_path.as_path())
        .to_path_buf();
    let open_folder_command = format!("open {}", shell_quote_for_sh(&report_dir.to_string_lossy()));
    let file_url = Url::from_directory_path(&report_dir)
        .ok()
        .map(|url| url.to_string())
        .unwrap_or_else(|| format!("file://{}", report_dir.display()));

    let notified = Command::new("terminal-notifier")
        .args([
            "-title",
            "OpenTracker Report Ready",
            "-message",
            &body,
            "-actions",
            "View",
            "-execute",
            &open_folder_command,
            "-open",
            &file_url,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);

    if notified {
        return;
    }

    // Avoid `display notification` fallback because clicking it can reopen Script Editor/iCloud paths.
    // Show an interactive dialog and open the report output folder in Finder.
    let dialog_script = r#"
on run argv
    set folderPath to item 1 of argv
    set dialogText to item 2 of argv
    set selectedButton to button returned of (display dialog dialogText buttons {"Dismiss", "View"} default button "View" with title "OpenTracker Report Ready")
    if selectedButton is "View" then
        tell application "Finder"
            open POSIX file folderPath
            activate
        end tell
    end if
end run
"#;

    if let Err(error) = Command::new("osascript")
        .arg("-e")
        .arg(dialog_script)
        .arg(report_dir.to_string_lossy().to_string())
        .arg(body)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        warn!(error = %error, "failed to show fallback macOS report dialog");
    }
}

#[cfg(target_os = "macos")]
fn shell_quote_for_sh(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[cfg(not(target_os = "macos"))]
fn send_macos_notification(_date: &str, _report_file_path: &std::path::Path) {}

fn ensure_dashboard_backend(config: &Config) -> Result<()> {
    if is_port_open(config.api_port) {
        return Ok(());
    }

    let daemon_status = daemon::status(config)?;
    if daemon_status.installed {
        daemon::load(config)?;
        thread::sleep(Duration::from_millis(600));
        return Ok(());
    }

    let current_exe =
        std::env::current_exe().context("Failed to resolve current executable path")?;
    let mut command = Command::new(current_exe);
    command
        .arg("service")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .stdin(Stdio::null());

    command
        .spawn()
        .context("Failed to spawn dashboard backend process")?;
    thread::sleep(Duration::from_millis(900));

    if !is_port_open(config.api_port) {
        bail!(
            "Failed to start dashboard server. Run `OpenTracker start` or `OpenTracker service`."
        );
    }

    Ok(())
}

fn is_port_open(port: u16) -> bool {
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
}
