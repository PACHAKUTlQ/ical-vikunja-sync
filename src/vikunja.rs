use anyhow::Result;
use reqwest::{Client, Method, StatusCode, header::CONTENT_TYPE, redirect::Policy};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use std::time::Duration;

const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
const MAX_ERROR_CHARACTERS: usize = 2048;
const PAGE_SIZE: u64 = 100;
const MAX_LABEL_PAGES: u64 = 10_000;

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub body: String,
}

impl ApiError {
    fn invalid_response(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_GATEWAY,
            body: message.into(),
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(output, "Vikunja returned {}: {}", self.status, self.body)
    }
}

impl std::error::Error for ApiError {}

#[derive(Debug, Deserialize)]
pub struct Label {
    pub id: i64,
    pub title: String,
}

#[derive(Deserialize)]
struct LabelPage {
    #[serde(deserialize_with = "deserialize_labels")]
    items: Vec<Label>,
    page: u64,
    total_pages: u64,
}

fn deserialize_labels<'de, D>(deserializer: D) -> Result<Vec<Label>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<Vec<Label>>::deserialize(deserializer)?.unwrap_or_default())
}

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
            .redirect(Policy::none())
            .user_agent(concat!("ical-vikunja-sync/", env!("CARGO_PKG_VERSION")))
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
        let is_patch = method == Method::PATCH;
        let mut request = self.client.request(method, url).bearer_auth(&self.token);

        if let Some(body) = body {
            request = request.json(&body);
            if is_patch {
                request = request.header(CONTENT_TYPE, "application/merge-patch+json");
            }
        }

        if markdown {
            request = request.header("X-Vikunja-Format", "markdown");
        }

        let mut response = request.send().await.map_err(|error| ApiError {
            status: StatusCode::BAD_GATEWAY,
            body: format!("request failed: {}", error.without_url()),
        })?;

        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(ApiError::invalid_response(
                "response exceeds the permitted size",
            ));
        }

        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| ApiError {
            status,
            body: format!("cannot read response: {}", error.without_url()),
        })? {
            if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
                return Err(ApiError::invalid_response(
                    "response exceeds the permitted size",
                ));
            }
            body.extend_from_slice(&chunk);
        }

        if !status.is_success() {
            let text = String::from_utf8_lossy(&body);
            let mut excerpt: String = text.chars().take(MAX_ERROR_CHARACTERS).collect();
            if text.chars().count() > MAX_ERROR_CHARACTERS {
                excerpt.push_str(" [truncated]");
            }
            return Err(ApiError {
                status,
                body: excerpt,
            });
        }

        if body.iter().all(u8::is_ascii_whitespace) {
            return Ok(Value::Null);
        }

        serde_json::from_slice(&body).map_err(|error| ApiError {
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
                    "description": "Tasks imported from an iCalendar feed."
                })),
                true,
            )
            .await?;

        response_id(&value, "project")
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

        response_id(&value, "task")
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

    pub async fn labels(&self) -> Result<Vec<Label>, ApiError> {
        self.list_labels("/labels").await
    }

    pub async fn create_label(&self, title: &str) -> Result<i64, ApiError> {
        let value = self
            .request(
                Method::POST,
                "/labels",
                Some(json!({ "title": title })),
                false,
            )
            .await?;

        let label: Label = serde_json::from_value(value).map_err(|error| {
            ApiError::invalid_response(format!("invalid label response: {error}"))
        })?;
        validate_label(&label)?;
        if label.title != title {
            return Err(ApiError::invalid_response(
                "created label title differs from the requested title",
            ));
        }

        Ok(label.id)
    }

    pub async fn task_labels(&self, task_id: i64) -> Result<Vec<Label>, ApiError> {
        self.list_labels(&format!("/tasks/{task_id}/labels")).await
    }

    pub async fn attach_label(&self, task_id: i64, label_id: i64) -> Result<(), ApiError> {
        self.request(
            Method::POST,
            &format!("/tasks/{task_id}/labels"),
            Some(json!({ "label_id": label_id })),
            false,
        )
        .await
        .map(|_| ())
    }

    pub async fn detach_label(&self, task_id: i64, label_id: i64) -> Result<(), ApiError> {
        self.request(
            Method::DELETE,
            &format!("/tasks/{task_id}/labels/{label_id}"),
            None,
            false,
        )
        .await
        .map(|_| ())
    }

    async fn list_labels(&self, path: &str) -> Result<Vec<Label>, ApiError> {
        let mut labels = Vec::new();

        for number in 1..=MAX_LABEL_PAGES {
            let value = self
                .request(
                    Method::GET,
                    &format!("{path}?page={number}&per_page={PAGE_SIZE}"),
                    None,
                    false,
                )
                .await?;
            let page: LabelPage = serde_json::from_value(value).map_err(|error| {
                ApiError::invalid_response(format!("invalid label pagination response: {error}"))
            })?;

            if page.page != number {
                return Err(ApiError::invalid_response(
                    "label response has an unexpected page number",
                ));
            }
            if page.total_pages > MAX_LABEL_PAGES {
                return Err(ApiError::invalid_response(
                    "label pagination exceeds the permitted page count",
                ));
            }
            if page.total_pages == 0 && (number != 1 || !page.items.is_empty()) {
                return Err(ApiError::invalid_response(
                    "label response has inconsistent pagination",
                ));
            }
            if page.total_pages > 0 && number > page.total_pages {
                return Err(ApiError::invalid_response(
                    "label pagination changed while being read",
                ));
            }
            if number < page.total_pages && page.items.is_empty() {
                return Err(ApiError::invalid_response(
                    "label response has an empty non-final page",
                ));
            }

            for label in &page.items {
                validate_label(label)?;
            }
            labels.extend(page.items);

            if number >= page.total_pages {
                return Ok(labels);
            }
        }

        Err(ApiError::invalid_response(
            "label pagination did not terminate",
        ))
    }
}

fn response_id(value: &Value, resource: &str) -> Result<i64, ApiError> {
    value
        .get("id")
        .and_then(Value::as_i64)
        .filter(|id| *id > 0)
        .ok_or_else(|| {
            ApiError::invalid_response(format!("{resource} response has no positive numeric id"))
        })
}

fn validate_label(label: &Label) -> Result<(), ApiError> {
    if label.id <= 0 || label.title.trim().is_empty() {
        return Err(ApiError::invalid_response(
            "label response has an invalid id or title",
        ));
    }

    Ok(())
}

pub fn is_not_found(error: &ApiError) -> bool {
    error.status == StatusCode::NOT_FOUND
}
