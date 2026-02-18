# OpenTracker

OpenTracker is a local macOS activity intelligence system that:

1. Samples the active app/window every 5 minutes.
2. Reads Chrome history for domain-level daily activity.
3. Generates daily reports in Markdown and JSON.
4. Serves a local dashboard and API at `http://127.0.0.1:<api_port>`.

This README is a full installation-to-operations guide.

## Table of Contents

- [What OpenTracker Collects](#what-opentracker-collects)
- [System Requirements](#system-requirements)
- [Install](#install)
- [First-Time Setup (Onboarding)](#first-time-setup-onboarding)
- [Run Modes](#run-modes)
- [Dashboard Usage](#dashboard-usage)
- [Generate Reports](#generate-reports)
- [OpenClaw Integration (via OpenTracker REST API)](#openclaw-integration-via-opentracker-rest-api)
- [Configuration Reference](#configuration-reference)
- [Local API Reference](#local-api-reference)
- [File Locations](#file-locations)
- [Troubleshooting](#troubleshooting)
- [Upgrade and Uninstall](#upgrade-and-uninstall)
- [Development Notes](#development-notes)

## What OpenTracker Collects

OpenTracker stores data locally in SQLite and report files.

- Active window samples:
  - Timestamp
  - App name
  - Window title (if Accessibility permission is granted)
  - Category
  - Duration (`300` seconds per sample)
- Chrome visits:
  - Date
  - Domain
  - Category
  - Visit duration (seconds from Chrome History DB)
- Daily report metadata:
  - Date
  - Generation timestamp
  - Markdown/JSON report paths

No remote upload is required for core functionality.

## System Requirements

- macOS (primary supported platform)
  - `launchd` daemon management is macOS-only.
  - Window-title collection uses AppleScript + Accessibility permission.
- Rust toolchain (`cargo`)
- Google Chrome installed (for Chrome history analysis)
- Optional: `terminal-notifier` for richer macOS notifications
  - Fallback AppleScript dialog works without it.

## Install

### 1. Clone and build

```bash
git clone https://github.com/jinyounghwa/OpenTracker.git
cd OpenTracker
cargo install --path .
```

If `OpenTracker` is not found after install, ensure Cargo bin path is on `PATH`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

### 2. Verify CLI

```bash
OpenTracker --help
```

You should see commands such as `onboard`, `status`, `dashboard`, `report`, `service`, etc.

## First-Time Setup (Onboarding)

Run onboarding:

```bash
OpenTracker onboard --install-daemon
```

Onboarding flow:

1. Requests macOS Accessibility permission (needed for window titles).
2. Lets you pick a Chrome profile.
3. Sets daily report time (`HH:MM`, local time).
4. Sets report output directory.
5. Installs and starts launchd daemon (when selected).

If you skip daemon installation, you can still run in foreground mode later.

## Run Modes

### A. Daemon mode (recommended for daily use)

```bash
OpenTracker start
```

- If daemon is installed, this loads launchd service.
- If daemon is not installed, it falls back to foreground service.

Stop/restart daemon:

```bash
OpenTracker stop
OpenTracker restart
```

### B. Foreground service mode

```bash
OpenTracker service
```

Use this for debugging or manual runtime.

## Dashboard Usage

Open the dashboard:

```bash
OpenTracker dashboard
```

- Starts backend if needed.
- Opens browser on macOS.
- Prints URL (default `http://127.0.0.1:7890`).

Dashboard sections:

- Collector Status
- Report Schedule (CRON preview + save time)
- Latest Daily Report summary
- Category Breakdown (Today / 7 Days)
- Latest Top Domains
- Latest Anomalies
- Recent Reports (view/download MD/JSON)
- Category Mapping Editor
- Activity Explorer

### Report schedule from dashboard

The dashboard updates `report_time` (daily local time).
Internally, OpenTracker converts it to a daily CRON expression (`MM HH * * *`).

Example:

- `23:30` -> `30 23 * * *`

Runtime schedule changes are picked up automatically (without restarting service).

## Generate Reports

### Generate today’s report

```bash
OpenTracker report
```

### Generate report for a specific date

```bash
OpenTracker report --date 2026-02-18
```

On success, CLI prints Markdown and JSON file paths.

## OpenClaw Integration (via OpenTracker REST API)

OpenClaw can call OpenTracker local REST APIs as tools and answer user questions in Telegram/WhatsApp/Discord.

Important:

- For this integration, OpenTracker `ai.api_key` is **not required**.
- OpenTracker only needs to expose its local API.
- OpenClaw handles LLM/provider auth and tool orchestration.

### 1. Run OpenTracker API

```bash
OpenTracker start
OpenTracker status
```

Default API base URL:

```text
http://127.0.0.1:7890
```

### 2. Have OpenClaw call OpenTracker endpoints

Core endpoints for chat answers:

- `GET /api/v1/activities?from=YYYY-MM-DD&to=YYYY-MM-DD`
- `GET /api/v1/report/latest` (optional, if latest daily report exists)
- `GET /api/v1/report/:date` (optional, if date-specific report exists)

Example API calls (today `2026-02-18`, yesterday `2026-02-17`):

```bash
curl -s "http://127.0.0.1:7890/api/v1/activities?from=2026-02-18&to=2026-02-18"
curl -s "http://127.0.0.1:7890/api/v1/activities?from=2026-02-17&to=2026-02-17"
```

### 3. Conversation flow example (Telegram)

1. User: `오늘 개발 얼마나 했어?`
2. OpenClaw tool calls OpenTracker REST API (`/api/v1/activities`) for today and yesterday.
3. OpenClaw aggregates `activities[].app_name` + `duration_sec` (for development-focused answer).
4. OpenClaw replies:

```text
오늘 Xcode 2시간 14분, VSCode 1시간 32분입니다.
어제보다 40분 적네요.
```

### 4. Quick troubleshooting

- API connection fails: ensure `OpenTracker start` is running and check `api_port`.
- Empty/weak answer: ensure activity data is being collected (`OpenTracker doctor` / `OpenTracker status`).
- If using only REST integration with OpenClaw, keep OpenTracker AI enrichment disabled:

```bash
OpenTracker config set ai.enabled false
```

## Configuration Reference

Set a value:

```bash
OpenTracker config set <key> <value>
```

Get a value:

```bash
OpenTracker config get <key>
```

### Supported set keys

| Key | Alias | Example | Notes |
|---|---|---|---|
| `polling_seconds` | `collector.interval_seconds` | `OpenTracker config set polling_seconds 300` | Fixed to `300` (5 min). Other values are rejected. |
| `report_time` | `report.time` | `OpenTracker config set report_time 23:30` | `HH:MM`, local time. |
| `report_dir` | `report.dir` | `OpenTracker config set report_dir ~/Documents/OpenTracker/reports` | Output folder for reports. |
| `chrome_profiles` | `chrome.profiles` | `OpenTracker config set chrome_profiles "Default,Profile 1"` | Comma-separated Chrome profile names. |
| `api_port` | `api.port` | `OpenTracker config set api_port 7890` | Dashboard/API port. |
| `retention_days` | `retention.days` | `OpenTracker config set retention_days 90` | Activity retention window. |
| `notify_on_report` | `report.notify` | `OpenTracker config set notify_on_report true` | macOS notification after report generation. |
| `ai_enabled` | `ai.enabled` | `OpenTracker config set ai.enabled true` | Enables OpenTracker's internal AI enrichment during report generation. Not required for OpenClaw REST integration. |
| `ai_api_key` | `ai.api_key` | `OpenTracker config set ai.api_key <KEY>` | API key used as Bearer token (`OPENTRACKER_AI_API_KEY` also supported). Used only when `ai.enabled=true`. |
| `ai_api_base_url` | `ai.base_url` | `OpenTracker config set ai.base_url https://api.openai.com/v1` | OpenAI-compatible base URL (no trailing `/chat/completions`). Used only when `ai.enabled=true`. |
| `ai_model` | `ai.model` | `OpenTracker config set ai.model gpt-4o-mini` | Model name sent by OpenTracker internal AI client. Used only when `ai.enabled=true`. |
| `ai_timeout_seconds` | `ai.timeout_seconds` | `OpenTracker config set ai.timeout_seconds 20` | AI API timeout (minimum 5 seconds). Used only when `ai.enabled=true`. |

## Local API Reference

Base URL: `http://127.0.0.1:<api_port>`

### Health and status

- `GET /api/v1/status`

### Reports

- `GET /api/v1/reports?limit=7`
- `GET /api/v1/report/latest`
- `GET /api/v1/report/:date`
- `GET /api/v1/report/:date/markdown`
- `GET /api/v1/report/:date/download/markdown`
- `GET /api/v1/report/:date/download/json`

### Activities

- `GET /api/v1/activities?from=YYYY-MM-DD&to=YYYY-MM-DD`

### Categories

- `GET /api/v1/categories`
- `PUT /api/v1/categories`

### Report schedule settings

- `GET /api/v1/settings/report-schedule`
- `PUT /api/v1/settings/report-schedule`

Request body example:

```json
{
  "report_time": "23:30"
}
```

Response example:

```json
{
  "saved": true,
  "report_time": "23:30",
  "cron_expression": "30 23 * * *"
}
```

## File Locations

Default paths:

- Config: `~/.OpenTracker/config.json`
- Categories: `~/.OpenTracker/categories.json`
- Database: `~/.OpenTracker/db/activity.db`
- Reports: `~/Documents/OpenTracker/reports/`
- launchd plist: `~/Library/LaunchAgents/com.OpenTracker.daemon.plist`
- launchd logs (plist default):
  - `/tmp/OpenTracker.log`
  - `/tmp/OpenTracker.err.log`

## Troubleshooting

### 1. `OpenTracker` command not found

- Ensure `~/.cargo/bin` is in `PATH`.
- Reinstall: `cargo install --path . --force`.

### 2. Dashboard/API not reachable

```bash
OpenTracker status
OpenTracker doctor
OpenTracker start
```

If port conflict exists, change:

```bash
OpenTracker config set api_port 7891
```

### 3. No window titles in activity

- macOS Accessibility permission is likely missing.
- Re-run onboarding:

```bash
OpenTracker onboard
```

### 4. Chrome categories look empty

- Verify profile path:

```bash
OpenTracker config get chrome_profiles
OpenTracker doctor
```

- Ensure selected Chrome profile has a valid `History` DB.

### 5. Report schedule changed but not instantly reflected

- Schedule updates are detected at runtime.
- Allow a short delay (about tens of seconds) for next scheduler refresh.

## Upgrade and Uninstall

### Upgrade

```bash
cargo install --path . --force
```

Or:

```bash
OpenTracker update
```

(`update` prints the install command; it does not auto-upgrade by itself.)

### Uninstall helper

```bash
OpenTracker uninstall
```

This unloads/removes daemon plist guidance and prints cleanup instructions.

To remove binary:

```bash
cargo uninstall opentracker
```

To remove local data (optional):

```bash
rm -rf ~/.OpenTracker ~/Documents/OpenTracker/reports
```

## Development Notes

- Main binary: `OpenTracker` (`src/main.rs`)
- Embedded dashboard assets: `frontend/dist` (served by Rust binary)
- Source dashboard file: `frontend/src/index.html`
- If editing dashboard manually, sync source to dist:

```bash
cp frontend/src/index.html frontend/dist/index.html
```

- Build/test:

```bash
cargo check
cargo test
```

## License

MIT

## Author

- Jin Younghwa
- [GitHub](https://github.com/jinyounghwa)
- [Email](mailto:[EMAIL_ADDRESS])
- [LinkedIn](https://www.linkedin.com/in/younghwa-jin-05619643/)
