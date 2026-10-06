use serde_json::Value as Json;

use crate::error::CliError;

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub base: String,
    pub token: Option<String>,
    pub cloud: bool,
    pub project_id: Option<String>,
}

pub struct ApiFailure {
    pub status: u16,
    pub code: Option<String>,
    pub message: String,
}

pub fn request(
    endpoint: &Endpoint,
    method: &str,
    path: &str,
    body: Option<&Json>,
) -> Result<Json, ApiFailure> {
    let url = format!("{}{path}", endpoint.base.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new().build();
    let mut call = agent.request(method, &url);
    if let Some(token) = &endpoint.token {
        call = call.set("Authorization", &format!("Bearer {token}"));
    }
    if let Some(project_id) = &endpoint.project_id {
        call = call.set("X-Trigora-Project-Id", project_id);
    }
    let result = if let Some(body) = body {
        call.set("Content-Type", "application/json")
            .send_json(body.clone())
    } else {
        call.call()
    };
    match result {
        Ok(response) => response.into_json().unwrap_or(Json::Null).pipe_ok(),
        Err(ureq::Error::Status(status, response)) => {
            let parsed: Json = response.into_json().unwrap_or(Json::Null);
            let error = parsed.get("error");
            Err(ApiFailure {
                status,
                code: error
                    .and_then(|value| value.get("code"))
                    .and_then(Json::as_str)
                    .map(str::to_string),
                message: error
                    .and_then(|value| value.get("message"))
                    .and_then(Json::as_str)
                    .unwrap_or("request failed")
                    .to_string(),
            })
        }
        Err(error) => {
            let reason = error.to_string();
            let message = if endpoint.cloud {
                format!("Could not reach Trigora Cloud at {url}. {reason}")
            } else {
                format!("Could not reach the local runtime at {url}. {reason}")
            };
            Err(ApiFailure {
                status: 0,
                code: None,
                message,
            })
        }
    }
}

trait PipeOk {
    fn pipe_ok(self) -> Result<Json, ApiFailure>;
}

impl PipeOk for Json {
    fn pipe_ok(self) -> Result<Json, ApiFailure> {
        Ok(self)
    }
}

pub fn runtime_failure(error: ApiFailure) -> CliError {
    let mut failure = CliError::new("Runtime request failed").message(error.message);
    if error.status == 0 {
        failure = failure.hint("Start `trigora dev` and try again.");
    }
    failure
}

pub fn cloud_failure(error: ApiFailure, step: &str) -> CliError {
    if error.code.as_deref() == Some("unauthorized") || error.code.as_deref() == Some("forbidden") {
        return CliError::new("Not authenticated")
            .message("TRIGORA_TOKEN is invalid or no longer active.")
            .hint("Set TRIGORA_TOKEN. Create a token at https://cloud.trigora.dev.");
    }
    if error.status == 0 {
        return CliError::new("Request failed").message(error.message);
    }
    CliError::new("Request failed")
        .detail("Step", step)
        .message(error.message)
}
