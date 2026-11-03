//! Fixed-endpoint sharing through Dalibo's public PEV2 service.
//!
//! API source: https://github.com/dalibo/explain.dalibo.com/blob/main/app/__init__.py
//! POST /new.json takes plan/query/title/comment and returns { id, deleteKey }.
//! Never follow upload redirects or interpret a server-supplied URL as trusted.

use std::time::Duration;

use catabase_core::CoreError;
use serde::{Deserialize, Serialize};

const SUBMIT_URL: &str = "https://explain.dalibo.com/new.json";
const PLAN_PREFIX: &str = "https://explain.dalibo.com/plan/";
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

#[derive(Serialize)]
struct Submission<'a> {
    plan: &'a str,
    query: &'a str,
    title: &'a str,
    comment: &'a str,
}

#[derive(Deserialize)]
struct SubmittedPlan {
    id: String,
}

fn valid_plan_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

pub(super) fn validate_plan_url(url: &str) -> Result<(), CoreError> {
    match url.strip_prefix(PLAN_PREFIX) {
        Some(id) if valid_plan_id(id) => Ok(()),
        _ => Err(CoreError::message(
            "Only Dalibo plan links can be opened here.",
        )),
    }
}

fn submitted_plan_url(body: &[u8]) -> Result<String, CoreError> {
    let response: SubmittedPlan = serde_json::from_slice(body)
        .map_err(|_| CoreError::message("Dalibo returned an invalid sharing response."))?;
    if !valid_plan_id(&response.id) {
        return Err(CoreError::message(
            "Dalibo returned an invalid plan identifier.",
        ));
    }
    Ok(format!("{PLAN_PREFIX}{}", response.id))
}

fn request_error(error: reqwest::Error) -> CoreError {
    // Do not include response bodies, submitted SQL, or plan contents in errors.
    // A failed response does not prove the server did not store the upload.
    if error.is_timeout() {
        CoreError::message(
            "Dalibo sharing timed out. The service may already have received the plan.",
        )
    } else {
        CoreError::message(
            "Could not complete sharing with Dalibo. Check your connection and retry sharing the saved plan.",
        )
    }
}

pub(super) async fn share_plan(
    plan: &str,
    query: &str,
    title: &str,
    comments: &str,
) -> Result<String, CoreError> {
    if plan.trim().is_empty() {
        return Err(CoreError::message("There is no execution plan to share."));
    }
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("Catabase/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(request_error)?;
    let mut response = client
        .post(SUBMIT_URL)
        .json(&Submission {
            plan,
            query,
            title,
            comment: comments,
        })
        .send()
        .await
        .map_err(request_error)?;
    if !response.status().is_success() {
        return Err(CoreError::message(format!(
            "Dalibo sharing failed (HTTP {}). The execution plan is still available locally.",
            response.status().as_u16()
        )));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(CoreError::message(
            "Dalibo returned an oversized sharing response.",
        ));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(request_error)? {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(CoreError::message(
                "Dalibo returned an oversized sharing response.",
            ));
        }
        body.extend_from_slice(&chunk);
    }
    submitted_plan_url(&body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_matches_the_service_field_names() {
        let submission = Submission {
            plan: "[{\"Plan\":{}}]",
            query: "SELECT 1",
            title: "",
            comment: "optional comment",
        };
        assert_eq!(
            serde_json::to_value(submission).unwrap(),
            serde_json::json!({"plan":"[{\"Plan\":{}}]", "query":"SELECT 1", "title":"", "comment":"optional comment"})
        );
    }

    #[test]
    fn response_id_is_converted_into_a_fixed_service_link() {
        let url = submitted_plan_url(br#"{"id":"g2d8b1eg1fa6g868","deleteKey":"unused-secret"}"#)
            .unwrap();
        assert_eq!(url, "https://explain.dalibo.com/plan/g2d8b1eg1fa6g868");
        assert!(validate_plan_url(&url).is_ok());
        for body in [
            br#"{"id":""}"#.as_slice(),
            br#"{"id":"https://elsewhere.example/plan"}"#,
            br#"{"id":"../about"}"#,
            br#"{"id":"abc?delete=yes"}"#,
            br#"{"id":3}"#,
            br#"{"url":"https://elsewhere.example"}"#,
            b"<html>private echoed content</html>",
        ] {
            let error = submitted_plan_url(body).unwrap_err().to_string();
            assert!(!error.contains("private echoed content"));
        }
    }

    #[test]
    fn opening_links_cannot_target_other_hosts_or_service_actions() {
        for url in [
            "http://explain.dalibo.com/plan/abc",
            "https://explain.dalibo.com.evil.example/plan/abc",
            "https://explain.dalibo.com@evil.example/plan/abc",
            "https://explain.dalibo.com/plan/abc/delete-key",
            "https://explain.dalibo.com/plan/abc?command=delete",
            "https://explain.dalibo.com/plan/abc#fragment",
            "https://explain.dalibo.com/plan/../about",
            "https://explain.dalibo.com/plan/%2fabout",
            "https://explain.dalibo.com/plan/abc\n",
            "javascript:alert(1)",
        ] {
            assert!(validate_plan_url(url).is_err(), "{url}");
        }
    }

    #[tokio::test]
    async fn empty_plan_does_not_make_a_request() {
        assert!(share_plan("  ", "SELECT 1", "", "").await.is_err());
    }

    #[tokio::test]
    #[ignore = "requires explicit CATABASE_DALIBO_LIVE_TEST=1; publishes a synthetic SELECT 1 plan only"]
    async fn service_accepts_synthetic_plan_and_returns_a_public_link() {
        assert_eq!(
            std::env::var("CATABASE_DALIBO_LIVE_TEST").as_deref(),
            Ok("1")
        );
        let plan = r#"[{"Plan":{"Node Type":"Result","Startup Cost":0.0,"Total Cost":0.01,"Plan Rows":1,"Plan Width":4,"Actual Startup Time":0.001,"Actual Total Time":0.001,"Actual Rows":1,"Actual Loops":1,"Output":["1"]},"Planning Time":0.01,"Execution Time":0.01}]"#;
        let url = share_plan(
            plan,
            "SELECT 1",
            "Catabase integration check: synthetic SELECT 1",
            "Synthetic integration-test plan. No application data or database identifiers.",
        )
        .await
        .unwrap();
        validate_plan_url(&url).unwrap();
        println!("Synthetic Dalibo plan: {url}");
        let response = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap()
            .get(&url)
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
    }
}
