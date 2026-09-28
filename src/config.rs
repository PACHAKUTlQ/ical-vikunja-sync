use anyhow::{Context, Result, bail};
use serde::Deserialize;
use std::{collections::HashSet, fs, path::Path};
use url::Url;

#[derive(Debug, Deserialize)]
pub struct Config {
    pub vikunja: VikunjaConfig,
    #[serde(default)]
    pub feeds: Vec<FeedConfig>,
}

#[derive(Debug, Deserialize)]
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
}

fn default_timeout() -> u64 {
    30
}

fn default_future_months() -> i64 {
    4
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

        let base = Url::parse(&self.vikunja.url)
            .with_context(|| "vikunja.url must be an absolute HTTP(S) URL")?;
        if !matches!(base.scheme(), "http" | "https") {
            bail!("vikunja.url must use HTTP or HTTPS");
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
        }

        Ok(())
    }
}
