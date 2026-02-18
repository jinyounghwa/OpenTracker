use crate::analyzer::categorizer::CategoryRules;
use crate::config::Config;
use crate::db::ChromeVisitInput;
use anyhow::{Context, Result, anyhow, bail};
use chrono::NaiveDate;
use reqwest::blocking::Client;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct AiEnrichment {
    pub visits: Vec<ChromeVisitInput>,
    pub insights: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AiClassificationPayload {
    domain_categories: Option<Vec<DomainCategoryItem>>,
    insights: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct DomainCategoryItem {
    domain: String,
    category: String,
}

pub fn enrich_chrome_visits(
    config: &Config,
    date: NaiveDate,
    visits: &[ChromeVisitInput],
) -> Result<AiEnrichment> {
    if !config.ai_enabled || visits.is_empty() {
        return Ok(AiEnrichment {
            visits: visits.to_vec(),
            insights: Vec::new(),
        });
    }

    let api_key = resolve_api_key(config);
    if api_key.is_none() {
        return Ok(AiEnrichment {
            visits: visits.to_vec(),
            insights: Vec::new(),
        });
    }

    let top_domains = visits
        .iter()
        .map(|visit| {
            json!({
                "domain": visit.domain,
                "duration_sec": visit.duration_sec,
                "current_category": visit.category,
            })
        })
        .collect::<Vec<_>>();

    let user_payload = json!({
        "date": date.format("%Y-%m-%d").to_string(),
        "domains": top_domains,
        "allowed_categories": ["development", "research", "communication", "entertainment", "sns", "shopping", "other"],
        "instruction": "Unknown domains can be recategorized if confidence is high.",
    });

    let system_prompt = r#"You are a strict activity classifier. Return JSON only: {"domain_categories":[{"domain":"example.com","category":"research"}],"insights":["..."]}. Categories must be one of development,research,communication,entertainment,sns,shopping,other."#;

    let content = chat_completion(
        config,
        api_key.as_deref().unwrap_or_default(),
        system_prompt,
        &user_payload.to_string(),
    )?;

    let parsed = parse_ai_payload(&content)?;

    let domain_map = parsed
        .domain_categories
        .unwrap_or_default()
        .into_iter()
        .map(|item| {
            (
                item.domain.trim().to_lowercase(),
                CategoryRules::normalize_category(&item.category),
            )
        })
        .collect::<HashMap<_, _>>();

    let recategorized_visits = visits
        .iter()
        .map(|visit| {
            let mapped_category = domain_map
                .get(&visit.domain.trim().to_lowercase())
                .cloned()
                .unwrap_or_else(|| visit.category.clone());

            ChromeVisitInput {
                domain: visit.domain.clone(),
                duration_sec: visit.duration_sec,
                category: mapped_category,
            }
        })
        .collect::<Vec<_>>();

    let insights = parsed
        .insights
        .unwrap_or_default()
        .into_iter()
        .map(|entry| entry.trim().to_string())
        .filter(|entry| !entry.is_empty())
        .take(8)
        .collect::<Vec<_>>();

    Ok(AiEnrichment {
        visits: recategorized_visits,
        insights,
    })
}

pub fn test_connection(config: &Config) -> Result<String> {
    let api_key = resolve_api_key(config).context(
        "AI API key is missing. Set `OpenTracker config set ai.api_key <KEY>` or `OPENTRACKER_AI_API_KEY`.",
    )?;

    let system_prompt =
        "Return exactly one short sentence in Korean indicating AI API connectivity is healthy.";
    let user_prompt = "Health check for OpenTracker.";

    chat_completion(config, &api_key, system_prompt, user_prompt)
}

pub fn has_api_key(config: &Config) -> bool {
    resolve_api_key(config).is_some()
}

fn resolve_api_key(config: &Config) -> Option<String> {
    std::env::var("OPENTRACKER_AI_API_KEY")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            config
                .ai_api_key
                .clone()
                .filter(|value| !value.trim().is_empty())
        })
}

fn chat_completion(config: &Config, api_key: &str, system: &str, user: &str) -> Result<String> {
    let base_url = config.ai_api_base_url.clone();
    let model = config.ai_model.clone();
    let timeout_seconds = config.ai_timeout_seconds.max(5);
    let api_key = api_key.to_string();
    let system = system.to_string();
    let user = user.to_string();

    std::thread::spawn(move || {
        chat_completion_blocking(&base_url, &model, timeout_seconds, &api_key, &system, &user)
    })
    .join()
    .map_err(|_| anyhow!("AI worker thread panicked"))?
}

fn chat_completion_blocking(
    base_url: &str,
    model: &str,
    timeout_seconds: u64,
    api_key: &str,
    system: &str,
    user: &str,
) -> Result<String> {
    if api_key.trim().is_empty() {
        bail!("AI API key is empty");
    }

    let endpoint = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {api_key}"))
            .context("Failed to build Authorization header")?,
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(timeout_seconds))
        .default_headers(headers)
        .build()
        .context("Failed to create AI HTTP client")?;

    let request_body = json!({
        "model": model,
        "temperature": 0.1,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });

    let response = client
        .post(endpoint)
        .json(&request_body)
        .send()
        .context("AI API request failed")?;

    let status = response.status();
    let body = response.text().context("Failed to read AI response body")?;

    if !status.is_success() {
        bail!("AI API error {}: {}", status, body);
    }

    let parsed: ChatCompletionResponse = serde_json::from_str(&body)
        .with_context(|| format!("Failed to parse AI response: {body}"))?;

    parsed
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
        .map(|content| content.trim().to_string())
        .filter(|content| !content.is_empty())
        .ok_or_else(|| anyhow!("AI response did not include message.content"))
}

fn parse_ai_payload(content: &str) -> Result<AiClassificationPayload> {
    let extracted = extract_json_block(content);
    serde_json::from_str(&extracted)
        .with_context(|| format!("Failed to parse AI JSON payload. content: {content}"))
}

fn extract_json_block(content: &str) -> String {
    let fenced = content.split("```").map(str::trim).find_map(|block| {
        block
            .strip_prefix("json")
            .map(str::trim)
            .or_else(|| block.starts_with('{').then_some(block))
    });

    match fenced {
        Some(block) => block.to_string(),
        None => {
            let first = content.find('{');
            let last = content.rfind('}');

            match (first, last) {
                (Some(start), Some(end)) if end > start => content[start..=end].to_string(),
                _ => content.trim().to_string(),
            }
        }
    }
}
