use anyhow::Result;
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use std::time::Duration;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: String,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "Vikunja returned {}: {}", self.status, self.body)
    }
}
impl std::error::Error for ApiError {}

#[derive(Clone)]
pub struct Vikunja {
    client: Client,
    base_url: String,
    token: String,
}

impl Vikunja {
    pub fn new(base_url: &str, token: &str, timeout: u64) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout))
            .user_agent("ical-vikunja-sync/0.1")
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.to_owned(),
        })
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        markdown: bool,
    ) -> Result<Value, ApiError> {
        let url = format!("{}{}", self.base_url, path);
        let mut request = self.client.request(method, url).bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        if markdown {
            request = request.header("X-Vikunja-Format", "markdown");
        }
        let response = request.send().await.map_err(|error| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            body: error.to_string(),
        })?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(ApiError { status, body });
        }
        if body.trim().is_empty() {
            return Ok(Value::Null);
        }
        serde_json::from_str(&body).map_err(|error| ApiError {
            status,
            body: format!("invalid JSON response: {error}"),
        })
    }

    pub async fn project(&self, id: i64) -> Result<Value, ApiError> {
        self.request(
            Method::GET,
            &format!("/projects/{id}?format=markdown"),
            None,
            false,
        )
        .await
    }

    pub async fn create_project(&self, title: &str) -> Result<i64, ApiError> {
        let value = self
            .request(
                Method::POST,
                "/projects?format=markdown",
                Some(json!({
                    "title": title,
                    "identifier": "ICAL",
                    "description": "Tasks imported from an iCalendar feed."
                })),
                true,
            )
            .await?;
        value
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: "project response has no numeric id".into(),
            })
    }

    pub async fn task(&self, id: i64) -> Result<Value, ApiError> {
        self.request(
            Method::GET,
            &format!("/tasks/{id}?format=markdown"),
            None,
            false,
        )
        .await
    }

    pub async fn create_task(&self, project: i64, payload: Value) -> Result<i64, ApiError> {
        let value = self
            .request(
                Method::POST,
                &format!("/projects/{project}/tasks?format=markdown"),
                Some(payload),
                true,
            )
            .await?;
        value
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| ApiError {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                body: "task response has no numeric id".into(),
            })
    }

    pub async fn update_task(&self, id: i64, payload: Value) -> Result<(), ApiError> {
        self.request(Method::PATCH, &format!("/tasks/{id}"), Some(payload), true)
            .await
            .map(|_| ())
    }

    pub async fn delete_task(&self, id: i64) -> Result<(), ApiError> {
        self.request(Method::DELETE, &format!("/tasks/{id}"), None, false)
            .await
            .map(|_| ())
    }
}

pub fn is_not_found(error: &ApiError) -> bool {
    error.status == StatusCode::NOT_FOUND
}
