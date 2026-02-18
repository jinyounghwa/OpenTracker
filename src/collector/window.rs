use chrono::Utc;

#[derive(Debug, Clone)]
pub struct WindowSample {
    pub recorded_at: i64,
    pub app_name: String,
    pub window_title: Option<String>,
}

pub fn collect_active_window() -> WindowSample {
    #[cfg(target_os = "macos")]
    {
        let app_name = run_osascript("tell application \"System Events\" to get name of first application process whose frontmost is true")
            .ok()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "Unknown".to_string());

        let window_title = run_osascript("tell application \"System Events\" to tell (first application process whose frontmost is true) to get name of front window")
            .ok()
            .filter(|value| !value.is_empty());

        return WindowSample {
            recorded_at: Utc::now().timestamp(),
            app_name,
            window_title,
        };
    }

    #[cfg(not(target_os = "macos"))]
    {
        WindowSample {
            recorded_at: Utc::now().timestamp(),
            app_name: "UnsupportedPlatform".to_string(),
            window_title: None,
        }
    }
}

pub fn accessibility_window_access_available() -> bool {
    collect_active_window().window_title.is_some()
}

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> std::io::Result<String> {
    let output = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(std::io::Error::other(stderr))
    }
}
