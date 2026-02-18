use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRules {
    pub apps: HashMap<String, String>,
    pub domains: HashMap<String, String>,
}

impl CategoryRules {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read categories file: {}", path.display()))?;
        let parsed: Self = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse categories file: {}", path.display()))?;

        Ok(parsed.normalized())
    }

    pub fn normalize_category(raw: &str) -> String {
        match raw.trim().to_lowercase().as_str() {
            "development" | "dev" | "개발" => "development".to_string(),
            "communication" | "커뮤니케이션" => "communication".to_string(),
            "research" | "리서치" => "research".to_string(),
            "entertainment" | "엔터테인먼트" => "entertainment".to_string(),
            "sns" => "sns".to_string(),
            "shopping" | "쇼핑" => "shopping".to_string(),
            "other" | "기타" => "other".to_string(),
            _ => "other".to_string(),
        }
    }

    pub fn categorize_app(&self, app_name: &str) -> String {
        let normalized = app_name.trim().to_lowercase();

        self.apps
            .get(&normalized)
            .cloned()
            .or_else(|| {
                self.apps
                    .iter()
                    .find(|(key, _)| normalized.contains(*key))
                    .map(|(_, value)| value.clone())
            })
            .map(|value| Self::normalize_category(&value))
            .unwrap_or_else(|| "other".to_string())
    }

    pub fn categorize_domain(&self, domain: &str) -> String {
        let normalized = domain.trim().to_lowercase();

        self.domains
            .iter()
            .find(|(rule, _)| domain_matches(&normalized, rule))
            .map(|(_, value)| Self::normalize_category(value))
            .unwrap_or_else(|| "other".to_string())
    }

    fn normalized(self) -> Self {
        let apps = self
            .apps
            .into_iter()
            .map(|(key, value)| (key.trim().to_lowercase(), Self::normalize_category(&value)))
            .collect::<HashMap<_, _>>();

        let domains = self
            .domains
            .into_iter()
            .map(|(key, value)| {
                (
                    key.trim().trim_start_matches("www.").to_lowercase(),
                    Self::normalize_category(&value),
                )
            })
            .collect::<HashMap<_, _>>();

        Self { apps, domains }
    }
}

fn domain_matches(domain: &str, rule: &str) -> bool {
    let normalized_rule = rule.trim().trim_start_matches("www.").to_lowercase();
    domain == normalized_rule || domain.ends_with(&format!(".{normalized_rule}"))
}

#[cfg(test)]
mod tests {
    use super::CategoryRules;
    use std::collections::HashMap;

    #[test]
    fn categorize_domain_with_subdomain() {
        let rules = CategoryRules {
            apps: HashMap::new(),
            domains: HashMap::from([("github.com".to_string(), "development".to_string())]),
        };

        assert_eq!(rules.categorize_domain("docs.github.com"), "development");
    }
}
