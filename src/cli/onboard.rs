use crate::collector::chrome;
use crate::config::{Config, default_report_dir, expand_home, parse_hhmm};
use crate::daemon;
use crate::db::Database;
use anyhow::{Context, Result};
use dialoguer::{Confirm, Input, Select, theme::ColorfulTheme};
use std::process::Command;

pub fn run_onboarding(install_daemon_flag: bool) -> Result<Config> {
    println!("──────────────────────────────────────────");
    println!("  Welcome to OpenTracker onboarding.");
    println!("──────────────────────────────────────────");

    let theme = ColorfulTheme::default();

    println!("\n[1/5] macOS Accessibility permission");
    println!("  Accessibility permission is required for window tracking.");

    let should_open = Confirm::with_theme(&theme)
        .with_prompt("  Open System Settings now?")
        .default(true)
        .interact()
        .context("Failed to read permission prompt input")?;

    if should_open {
        open_accessibility_settings();
        println!("  -> Opened System Settings > Privacy & Security > Accessibility");
    }

    let confirmed = Confirm::with_theme(&theme)
        .with_prompt("  Did you grant permission?")
        .default(true)
        .interact()
        .context("Failed to read permission confirmation input")?;

    if confirmed {
        println!("  ✓ Permission confirmed");
    } else {
        println!("  ! Continuing without permission (app name only)");
    }

    println!("\n[2/5] Select Chrome profile");
    let profiles = {
        let mut detected = chrome::detect_chrome_profiles();
        if detected.is_empty() {
            detected.push("Default".to_string());
        }
        detected
    };

    let selected_index = Select::with_theme(&theme)
        .with_prompt("  Select profile to track")
        .default(0)
        .items(&profiles)
        .interact()
        .context("Failed to select Chrome profile")?;

    let selected_profile = profiles
        .get(selected_index)
        .cloned()
        .unwrap_or_else(|| "Default".to_string());
    println!("  ✓ Selected profile: {}", selected_profile);

    println!("\n[3/5] Set report generation time");
    let report_time: String = Input::with_theme(&theme)
        .with_prompt("  Enter daily report generation time")
        .default("23:30".to_string())
        .validate_with(|input: &String| -> std::result::Result<(), &str> {
            parse_hhmm(input)
                .map(|_| ())
                .map_err(|_| "Use HH:MM format (example: 23:30)")
        })
        .interact_text()
        .context("Failed to read report time")?;
    println!("  ✓ Report will be generated daily at {report_time}");

    println!("\n[4/5] Report output directory");
    let default_report_dir = default_report_dir().display().to_string();
    let report_dir_input: String = Input::with_theme(&theme)
        .with_prompt("  Folder where reports will be saved")
        .default(default_report_dir)
        .interact_text()
        .context("Failed to read report directory")?;

    let report_dir = expand_home(&report_dir_input);
    println!("  ✓ {}", report_dir.display());

    println!("\n[5/5] Install background daemon");
    println!("  Register a launchd service so OpenTracker starts automatically after reboot.");

    let install_daemon = if install_daemon_flag {
        true
    } else {
        Confirm::with_theme(&theme)
            .with_prompt("  Install daemon now?")
            .default(true)
            .interact()
            .context("Failed to read daemon install input")?
    };

    let config = Config {
        report_time,
        report_dir,
        chrome_profiles: vec![selected_profile],
        ..Config::default()
    };

    config.ensure_bootstrap_files()?;
    config.save()?;
    let _ = Database::open(&config.db_path)?;

    if install_daemon {
        let plist_path = daemon::install(&config)?;
        daemon::load(&config)?;
        println!("  ✓ Daemon installed ({})", plist_path.display());
    } else {
        println!("  ✓ Skipped daemon installation");
    }

    println!("\n──────────────────────────────────────────");
    println!("  Onboarding complete!");
    println!("  Collection has started.");
    println!("  Run OpenTracker status to check current state.");
    println!("──────────────────────────────────────────");

    Ok(config)
}

#[cfg(target_os = "macos")]
fn open_accessibility_settings() {
    let _ = Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status();
}

#[cfg(not(target_os = "macos"))]
fn open_accessibility_settings() {}
