use anyhow::{Context, Result, bail};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Deserializer, de::Error as _};
use std::{collections::HashSet, fs, path::Path};
use url::Url;

const MAX_PATTERN_BYTES: usize = 16 * 1024;
const REGEX_SIZE_LIMIT: usize = 1024 * 1024;
const REGEX_DFA_SIZE_LIMIT: usize = 1024 * 1024;

#[derive(Deserialize)]
pub struct Config {
    pub vikunja: VikunjaConfig,
    #[serde(default)]
    pub feeds: Vec<FeedConfig>,
}

#[derive(Deserialize)]
pub struct VikunjaConfig {
    pub url: String,
    pub token: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FeedConfig {
    pub id: String,
    pub source: String,
    pub project: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub window_past_months: i64,
    #[serde(default = "default_future_months")]
    pub window_future_months: i64,
    pub retention_days: Option<i64>,
    #[serde(default)]
    pub label_rules: Vec<LabelRule>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct LabelRule {
    pub label: String,
    #[serde(deserialize_with = "deserialize_pattern")]
    pub pattern: Regex,
    #[serde(default = "default_match_fields")]
    pub fields: Vec<MatchField>,
    #[serde(default)]
    pub mode: MatchMode,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MatchField {
    Summary,
    Description,
    Location,
}

#[derive(Debug, Default, Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum MatchMode {
    #[default]
    Or,
    And,
}

fn default_timeout() -> u64 {
    30
}

fn default_future_months() -> i64 {
    4
}

fn default_match_fields() -> Vec<MatchField> {
    vec![
        MatchField::Summary,
        MatchField::Description,
        MatchField::Location,
    ]
}

fn deserialize_pattern<'de, D>(deserializer: D) -> Result<Regex, D::Error>
where
    D: Deserializer<'de>,
{
    let pattern = String::deserialize(deserializer)?;
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(D::Error::custom(format!(
            "label pattern exceeds {MAX_PATTERN_BYTES} bytes"
        )));
    }

    RegexBuilder::new(&pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .dfa_size_limit(REGEX_DFA_SIZE_LIMIT)
        .build()
        .map_err(D::Error::custom)
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read configuration {}", path.display()))?;
        let config: Self = env_toml::from_str(&text)
            .with_context(|| format!("cannot parse configuration {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        if self.vikunja.token.trim().is_empty() {
            bail!("vikunja.token must not be empty");
        }
        if self.vikunja.timeout_seconds == 0 {
            bail!("vikunja.timeout_seconds must be positive");
        }

        let base = Url::parse(&self.vikunja.url)
            .with_context(|| "vikunja.url must be an absolute HTTP(S) URL")?;
        if !matches!(base.scheme(), "http" | "https") || base.host_str().is_none() {
            bail!("vikunja.url must be an absolute HTTP(S) URL");
        }
        if !base.username().is_empty() || base.password().is_some() {
            bail!("vikunja.url must not contain credentials");
        }
        if base.query().is_some() || base.fragment().is_some() {
            bail!("vikunja.url must not contain a query or fragment");
        }

        let mut ids = HashSet::new();
        for feed in &self.feeds {
            if feed.id.is_empty()
                || !feed
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            {
                bail!(
                    "feed id {:?} must contain only ASCII letters, digits, hyphens, or underscores",
                    feed.id
                );
            }
            if feed.project.trim().is_empty() {
                bail!("feed {} has an empty project name", feed.id);
            }
            if !ids.insert(&feed.id) {
                bail!("duplicate feed id: {}", feed.id);
            }
            if feed.window_past_months < 0 || feed.window_future_months < 0 {
                bail!("feed {} has a negative materialization window", feed.id);
            }
            if feed.retention_days.is_some_and(|days| days < 0) {
                bail!("feed {} has a negative retention period", feed.id);
            }

            for (index, rule) in feed.label_rules.iter().enumerate() {
                rule.validate().with_context(|| {
                    format!("invalid label rule {} for feed {}", index + 1, feed.id)
                })?;
            }
        }

        Ok(())
    }
}

impl LabelRule {
    fn validate(&self) -> Result<()> {
        if self.label.trim().is_empty() {
            bail!("label must not be empty");
        }
        if self.label != self.label.trim() {
            bail!("label must not have leading or trailing whitespace");
        }
        if self.label.chars().count() > 250 {
            bail!("label must contain at most 250 characters");
        }
        if self.label.chars().any(char::is_control) {
            bail!("label must not contain control characters");
        }
        if self.fields.is_empty() {
            bail!("fields must contain at least one field");
        }

        let mut fields = HashSet::new();
        if self.fields.iter().any(|field| !fields.insert(*field)) {
            bail!("fields must not contain duplicates");
        }

        Ok(())
    }
}
