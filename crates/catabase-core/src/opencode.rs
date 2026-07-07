use serde::{Deserialize, Serialize};

use crate::CoreError;

#[derive(Clone)]
pub struct OpenCodeClient {
    base_url: String,
    client: reqwest::Client,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHealth {
    pub state: String,
    pub message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCompletionRequest {
    pub prefix: String,
    pub suffix: String,
    pub language: String,
    pub position: usize,
    pub schema_context: Option<String>,
    pub model: Option<String>,
}

impl Default for OpenCodeClient {
    fn default() -> Self {
        Self::new("http://127.0.0.1:4096")
    }
}

impl OpenCodeClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn health(&self) -> AiHealth {
        match self
            .client
            .get(format!("{}/health", self.base_url))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => AiHealth {
                state: "on".to_string(),
                message: None,
            },
            Ok(response) => AiHealth {
                state: "error".to_string(),
                message: Some(format!("OpenCode returned {}", response.status())),
            },
            Err(error) => AiHealth {
                state: "off".to_string(),
                message: Some(error.to_string()),
            },
        }
    }

    pub async fn models(&self) -> Result<Vec<String>, CoreError> {
        let response = self
            .client
            .get(format!("{}/models", self.base_url))
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(Vec::new());
        }
        let value = response.json::<serde_json::Value>().await?;
        Ok(extract_models(&value))
    }

    pub async fn complete(
        &self,
        request: AiCompletionRequest,
    ) -> Result<Option<String>, CoreError> {
        let response = self
            .client
            .post(format!("{}/complete", self.base_url))
            .json(&request)
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        let value = response.json::<serde_json::Value>().await?;
        Ok(value
            .get("completion")
            .or_else(|| value.get("text"))
            .and_then(|value| value.as_str())
            .map(ToString::to_string))
    }
}

fn extract_models(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                item.as_str().map(ToString::to_string).or_else(|| {
                    item.get("id")
                        .and_then(|id| id.as_str())
                        .map(ToString::to_string)
                })
            })
            .collect(),
        serde_json::Value::Object(object) => object
            .get("models")
            .map(extract_models)
            .unwrap_or_else(Vec::new),
        _ => Vec::new(),
    }
}
