use crate::analyzer::categorizer::CategoryRules;
use crate::api::get_embedded_asset;
use crate::config::Config;
use crate::daemon;
use crate::db::{ActivityRow, Database};
use crate::scheduler;
use anyhow::{Context, Result};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use chrono::{Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::Path as FsPath;
use std::sync::Arc;

#[derive(Clone)]
pub struct ApiState {
    pub config: Arc<Config>,
}

pub fn router(state: ApiState) -> Router {
    Router::new()
        .route("/api/v1/status", get(status))
        .route("/api/v1/reports", get(report_list))
        .route("/api/v1/report/latest", get(report_latest))
        .route("/api/v1/report/:date", get(report_by_date))
        .route("/api/v1/report/:date/markdown", get(report_markdown))
        .route(
            "/api/v1/report/:date/download/markdown",
            get(report_download_markdown),
        )
        .route(
            "/api/v1/report/:date/download/json",
            get(report_download_json),
        )
        .route("/api/v1/activities", get(activities))
        .route(
            "/api/v1/settings/report-schedule",
            get(report_schedule_get).put(report_schedule_put),
        )
        .route(
            "/api/v1/categories",
            get(categories_get).put(categories_put),
        )
        .fallback(get(static_assets))
        .with_state(state)
}

#[derive(Debug, Deserialize)]
struct ActivitiesQuery {
    from: Option<String>,
    to: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReportsQuery {
    limit: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ActivitiesPayload {
    from: String,
    to: String,
    count: usize,
    activities: Vec<ActivityRow>,
}

#[derive(Debug, Serialize)]
struct ReportsPayload {
    reports: Vec<ReportView>,
}

#[derive(Debug, Serialize)]
struct ReportSchedulePayload {
    report_time: String,
    cron_expression: String,
}

#[derive(Debug, Deserialize)]
struct ReportScheduleUpdatePayload {
    report_time: String,
}

#[derive(Debug, Serialize)]
struct ReportView {
    date: String,
    generated_at: i64,
    markdown_url: String,
    json_url: String,
    markdown_download_url: String,
    json_download_url: String,
}

#[derive(Debug, Serialize)]
struct StatusPayload {
    daemon: String,
    daemon_loaded: bool,
    last_collected_at: Option<i64>,
    latest_report_date: Option<String>,
    api_port: u16,
}

async fn status(State(state): State<ApiState>) -> ApiResult<Json<StatusPayload>> {
    let database = Database::open(&state.config.db_path)?;
    let daemon_status = daemon::status(&state.config)?;

    let payload = StatusPayload {
        daemon: daemon_status.details,
        daemon_loaded: daemon_status.loaded,
        last_collected_at: database.latest_activity_timestamp()?,
        latest_report_date: database.latest_report_meta()?.map(|meta| meta.date),
        api_port: state.config.api_port,
    };

    Ok(Json(payload))
}

async fn report_list(
    State(state): State<ApiState>,
    Query(query): Query<ReportsQuery>,
) -> ApiResult<Json<ReportsPayload>> {
    let limit = query.limit.unwrap_or(7).clamp(1, 90);
    let database = Database::open(&state.config.db_path)?;
    let reports = database
        .list_reports(limit)?
        .into_iter()
        .map(|meta| ReportView {
            date: meta.date.clone(),
            generated_at: meta.generated_at,
            markdown_url: format!("/api/v1/report/{}/markdown", meta.date),
            json_url: format!("/api/v1/report/{}", meta.date),
            markdown_download_url: format!("/api/v1/report/{}/download/markdown", meta.date),
            json_download_url: format!("/api/v1/report/{}/download/json", meta.date),
        })
        .collect::<Vec<_>>();

    Ok(Json(ReportsPayload { reports }))
}

async fn report_latest(State(state): State<ApiState>) -> ApiResult<Json<Value>> {
    let database = Database::open(&state.config.db_path)?;

    let latest = database
        .latest_report_meta()?
        .context("No reports have been generated yet")?;

    let report = load_json(FsPath::new(&latest.json_path))?;
    Ok(Json(report))
}

async fn report_by_date(
    State(state): State<ApiState>,
    Path(date): Path<String>,
) -> ApiResult<Json<Value>> {
    let target_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .with_context(|| "Invalid date format. Example: 2026-02-18")?;

    let database = Database::open(&state.config.db_path)?;
    let report_meta = database
        .report_meta(target_date)?
        .with_context(|| format!("No report found for date: {target_date}"))?;

    let report = load_json(FsPath::new(&report_meta.json_path))?;
    Ok(Json(report))
}

async fn report_markdown(
    State(state): State<ApiState>,
    Path(date): Path<String>,
) -> ApiResult<Response> {
    let target_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .with_context(|| "Invalid date format. Example: 2026-02-18")?;

    let database = Database::open(&state.config.db_path)?;
    let report_meta = database
        .report_meta(target_date)?
        .with_context(|| format!("No report found for date: {target_date}"))?;

    let markdown = fs::read_to_string(&report_meta.md_path)
        .with_context(|| format!("Failed to read Markdown report: {}", report_meta.md_path))?;

    let mut response = Response::new(markdown.into_response().into_body());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );

    Ok(response)
}

async fn report_download_markdown(
    State(state): State<ApiState>,
    Path(date): Path<String>,
) -> ApiResult<Response> {
    let target_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .with_context(|| "Invalid date format. Example: 2026-02-18")?;
    let database = Database::open(&state.config.db_path)?;
    let report_meta = database
        .report_meta(target_date)?
        .with_context(|| format!("No report found for date: {target_date}"))?;

    let markdown = fs::read_to_string(&report_meta.md_path)
        .with_context(|| format!("Failed to read Markdown report: {}", report_meta.md_path))?;
    let filename = format!("{}.md", target_date.format("%Y-%m-%d"));

    let mut response = Response::new(markdown.into_response().into_body());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/markdown; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))?,
    );

    Ok(response)
}

async fn report_download_json(
    State(state): State<ApiState>,
    Path(date): Path<String>,
) -> ApiResult<Response> {
    let target_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .with_context(|| "Invalid date format. Example: 2026-02-18")?;
    let database = Database::open(&state.config.db_path)?;
    let report_meta = database
        .report_meta(target_date)?
        .with_context(|| format!("No report found for date: {target_date}"))?;

    let content = fs::read_to_string(&report_meta.json_path)
        .with_context(|| format!("Failed to read report JSON file: {}", report_meta.json_path))?;
    let filename = format!("{}.json", target_date.format("%Y-%m-%d"));

    let mut response = Response::new(content.into_response().into_body());
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))?,
    );

    Ok(response)
}

async fn activities(
    State(state): State<ApiState>,
    Query(query): Query<ActivitiesQuery>,
) -> ApiResult<Json<ActivitiesPayload>> {
    let from_date = query
        .from
        .as_deref()
        .map(parse_date)
        .transpose()?
        .unwrap_or_else(|| Local::now().date_naive());

    let to_date = query
        .to
        .as_deref()
        .map(parse_date)
        .transpose()?
        .unwrap_or(from_date);

    let from_ts = from_date
        .and_hms_opt(0, 0, 0)
        .context("Failed to create from timestamp")?
        .and_local_timezone(Local)
        .single()
        .context("Failed to convert from timestamp to local time")?
        .timestamp();

    let to_ts = (to_date + Duration::days(1))
        .and_hms_opt(0, 0, 0)
        .context("Failed to create to timestamp")?
        .and_local_timezone(Local)
        .single()
        .context("Failed to convert to timestamp to local time")?
        .timestamp()
        - 1;

    let database = Database::open(&state.config.db_path)?;
    let records = database.activities_between(from_ts, to_ts)?;

    let payload = ActivitiesPayload {
        from: from_date.format("%Y-%m-%d").to_string(),
        to: to_date.format("%Y-%m-%d").to_string(),
        count: records.len(),
        activities: records,
    };

    Ok(Json(payload))
}

async fn report_schedule_get(
    State(state): State<ApiState>,
) -> ApiResult<Json<ReportSchedulePayload>> {
    let config = Config::load().unwrap_or_else(|_| state.config.as_ref().clone());
    let cron_expression = scheduler::cron_from_report_time(&config.report_time)?;

    Ok(Json(ReportSchedulePayload {
        report_time: config.report_time,
        cron_expression,
    }))
}

async fn report_schedule_put(
    State(state): State<ApiState>,
    Json(payload): Json<ReportScheduleUpdatePayload>,
) -> ApiResult<Json<Value>> {
    let mut config = Config::load().unwrap_or_else(|_| state.config.as_ref().clone());
    let normalized_time = payload.report_time.trim().to_string();

    config
        .set_value("report_time", &normalized_time)
        .map_err(|error| ApiError::BadRequest(error.to_string()))?;
    config.save()?;

    let cron_expression = scheduler::cron_from_report_time(&config.report_time)?;

    Ok(Json(json!({
        "saved": true,
        "report_time": config.report_time,
        "cron_expression": cron_expression
    })))
}

async fn categories_get(State(state): State<ApiState>) -> ApiResult<Json<Value>> {
    let content = fs::read_to_string(&state.config.categories_path).with_context(|| {
        format!(
            "Failed to read categories file: {}",
            state.config.categories_path.display()
        )
    })?;
    let categories: Value =
        serde_json::from_str(&content).context("Failed to parse categories JSON")?;

    Ok(Json(categories))
}

async fn categories_put(
    State(state): State<ApiState>,
    Json(payload): Json<Value>,
) -> ApiResult<Json<Value>> {
    serde_json::from_value::<CategoryRules>(payload.clone())
        .map_err(|error| ApiError::BadRequest(format!("Invalid categories schema: {error}")))?;

    let pretty =
        serde_json::to_string_pretty(&payload).context("Failed to serialize categories JSON")?;
    fs::write(&state.config.categories_path, pretty).with_context(|| {
        format!(
            "Failed to save categories file: {}",
            state.config.categories_path.display()
        )
    })?;

    Ok(Json(json!({
        "saved": true,
        "path": state.config.categories_path.display().to_string()
    })))
}

async fn static_assets(uri: Uri) -> ApiResult<Response> {
    let path = uri.path();

    match get_embedded_asset(path) {
        Some((bytes, mime)) => {
            let mut response = Response::new(bytes.into_response().into_body());
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_str(&mime)?);
            Ok(response)
        }
        None => Err(ApiError::NotFound("Static asset not found".to_string())),
    }
}

fn parse_date(input: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .with_context(|| format!("Invalid date format: {input}. Example: 2026-02-18"))
}

fn load_json(path: &FsPath) -> Result<Value> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read report JSON file: {}", path.display()))?;

    let payload = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse report JSON file: {}", path.display()))?;

    Ok(payload)
}

type ApiResult<T> = std::result::Result<T, ApiError>;

#[derive(Debug)]
enum ApiError {
    BadRequest(String),
    NotFound(String),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for ApiError {
    fn from(value: anyhow::Error) -> Self {
        Self::Internal(value)
    }
}

impl From<axum::http::header::InvalidHeaderValue> for ApiError {
    fn from(value: axum::http::header::InvalidHeaderValue) -> Self {
        Self::Internal(value.into())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            ApiError::BadRequest(message) => {
                (StatusCode::BAD_REQUEST, Json(json!({ "error": message }))).into_response()
            }
            ApiError::NotFound(message) => {
                (StatusCode::NOT_FOUND, Json(json!({ "error": message }))).into_response()
            }
            ApiError::Internal(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": error.to_string() })),
            )
                .into_response(),
        }
    }
}
