use crate::config::Config;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct DaemonStatus {
    pub installed: bool,
    pub loaded: bool,
    pub details: String,
}

pub fn install(config: &Config) -> Result<PathBuf> {
    let plist_path = plist_path(config)?;
    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create LaunchAgents directory: {}",
                parent.display()
            )
        })?;
    }

    let binary_path =
        std::env::current_exe().context("Failed to resolve current executable path")?;
    let plist = render_plist(&config.daemon_label, &binary_path);

    fs::write(&plist_path, plist)
        .with_context(|| format!("Failed to write launchd plist: {}", plist_path.display()))?;

    Ok(plist_path)
}

pub fn load(config: &Config) -> Result<()> {
    let plist = plist_path(config)?;
    if !plist.exists() {
        bail!("launchd plist not found: {}", plist.display());
    }

    #[cfg(target_os = "macos")]
    {
        let domain = format!("gui/{}", user_id());

        let _ = run_launchctl(["bootout", &domain, plist.to_string_lossy().as_ref()]);
        run_launchctl(["bootstrap", &domain, plist.to_string_lossy().as_ref()])?;
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = config;
        bail!("launchd is only supported on macOS");
    }

    Ok(())
}

pub fn unload(config: &Config) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let plist = plist_path(config)?;
        let domain = format!("gui/{}", user_id());

        if plist.exists() {
            let _ = run_launchctl(["bootout", &domain, plist.to_string_lossy().as_ref()]);
        } else {
            let label = format!("{domain}/{}", config.daemon_label);
            let _ = run_launchctl(["bootout", &label]);
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = config;
        bail!("launchd is only supported on macOS");
    }

    Ok(())
}

pub fn restart(config: &Config) -> Result<()> {
    unload(config)?;
    load(config)
}

pub fn status(config: &Config) -> Result<DaemonStatus> {
    let plist = plist_path(config)?;
    let installed = plist.exists();

    #[cfg(target_os = "macos")]
    {
        let domain = format!("gui/{}/{}", user_id(), config.daemon_label);
        let details = run_launchctl(["print", &domain]);

        return Ok(match details {
            Ok(output) => DaemonStatus {
                installed,
                loaded: true,
                details: output,
            },
            Err(error) => DaemonStatus {
                installed,
                loaded: false,
                details: error.to_string(),
            },
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(DaemonStatus {
            installed,
            loaded: false,
            details: "launchd is only available on macOS".to_string(),
        })
    }
}

pub fn plist_path(config: &Config) -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to resolve HOME directory")?;
    Ok(home
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{}.plist", config.daemon_label)))
}

fn render_plist(label: &str, binary: &PathBuf) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{binary}</string>
    <string>service</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/tmp/OpenTracker.log</string>
  <key>StandardErrorPath</key>
  <string>/tmp/OpenTracker.err.log</string>
</dict>
</plist>
"#,
        binary = binary.display()
    )
}

#[cfg(target_os = "macos")]
fn run_launchctl<const N: usize>(args: [&str; N]) -> Result<String> {
    let output = Command::new("launchctl")
        .args(args)
        .output()
        .with_context(|| "Failed to execute launchctl")?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        bail!("launchctl error: {stderr}");
    }
}

#[cfg(target_os = "macos")]
fn user_id() -> u32 {
    unsafe { libc::geteuid() }
}
