//! A file-backed scripted HTTP transport for real-process, network-free
//! child-process tests and manual demonstrations.
//!
//! Mirrors [`fieldnotes_msgraph::testing::ScriptedTransport`] exactly, except
//! the script is loaded from a file path rather than constructed in the same
//! process: a spawned child process cannot share a parent test process's
//! in-memory `Vec`, so the sanitized recorded fixture has to cross the
//! process boundary as a file instead. Selected only by `main`, only when
//! [`crate::constants::FIXTURE_SCRIPT_ENV`] is set -- production always
//! takes the real [`fieldnotes_msgraph::UreqTransport`] branch instead. See
//! this crate's final report for why a real [`fieldnotes_msgraph::UreqTransport`]
//! itself cannot be pointed at a local fixture server for this purpose.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;

use fieldnotes_msgraph::{GraphHttpRequest, GraphHttpResponse, HttpTransport, TransportError};
use serde::Deserialize;

/// One scripted response, as recorded in a fixture script file: a JSON array
/// of these, read once in full and answered in order.
#[derive(Debug, Deserialize)]
struct ScriptedResponse {
    status: u16,
    #[serde(default)]
    headers: Vec<(String, String)>,
    body: serde_json::Value,
}

/// Why a fixture script could not be loaded.
#[derive(Debug)]
pub(crate) struct ScriptLoadError(String);

impl fmt::Display for ScriptLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ScriptLoadError {}

/// Answers a fixed, ordered script of responses read from one file, instead
/// of making a network call.
pub(crate) struct FileScriptedTransport {
    responses: RefCell<VecDeque<ScriptedResponse>>,
}

impl FileScriptedTransport {
    /// Reads and parses the whole script file up front, so a malformed
    /// fixture fails fast at startup rather than mid-run.
    pub(crate) fn load(path: &str) -> Result<Self, ScriptLoadError> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| ScriptLoadError(format!("{path}: {error}")))?;
        let responses: Vec<ScriptedResponse> = serde_json::from_str(&text)
            .map_err(|error| ScriptLoadError(format!("{path}: {error}")))?;
        Ok(FileScriptedTransport {
            responses: RefCell::new(responses.into_iter().collect()),
        })
    }
}

impl HttpTransport for FileScriptedTransport {
    fn execute(&self, _request: &GraphHttpRequest) -> Result<GraphHttpResponse, TransportError> {
        let mut queue = self.responses.borrow_mut();
        let Some(next) = queue.pop_front() else {
            return Err(TransportError::new(
                "the fixture script has no further recorded response",
            ));
        };
        let body = serde_json::to_vec(&next.body)
            .map_err(|error| TransportError::new(error.to_string()))?;
        let mut headers = next.headers;
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        {
            headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
        }
        Ok(GraphHttpResponse::new(next.status, headers, body))
    }
}

#[cfg(test)]
mod tests {
    use super::FileScriptedTransport;
    use fieldnotes_msgraph::testing::FakeRetryClock;
    use fieldnotes_msgraph::{AccessToken, GraphClient, GraphError, GraphRequest};
    use fieldnotes_test_support::CountingRandom;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Item {
        #[allow(dead_code)]
        value: u32,
    }

    /// Drives a real [`GraphClient`] over [`FileScriptedTransport`], which is
    /// the same seam `main` selects when
    /// [`crate::constants::FIXTURE_SCRIPT_ENV`] is set -- exercised here
    /// through the public client API, since [`fieldnotes_msgraph`]'s own
    /// request/response constructors are private to that crate.
    #[test]
    fn a_script_file_answers_its_recorded_responses_in_order() {
        let temp = fieldnotes_test_support::TempDir::new("fixture-transport")
            .unwrap_or_else(|error| panic!("temp dir: {error}"));
        let script_path = temp.path().join("script.json");
        std::fs::write(
            &script_path,
            r#"[
                {"status": 200, "body": {"value": []}},
                {"status": 429, "headers": [["Retry-After", "0"]], "body": {"error": {"code": "throttled"}}},
                {"status": 200, "body": {"value": [{"value": 1}]}}
            ]"#,
        )
        .unwrap_or_else(|error| panic!("write script: {error}"));

        let transport = FileScriptedTransport::load(
            script_path.to_str().unwrap_or_else(|| panic!("utf8 path")),
        )
        .unwrap_or_else(|error| panic!("load: {error}"));
        let client = GraphClient::new(transport, FakeRetryClock::new(0), CountingRandom::new(0));
        let token = AccessToken::new("FIXTURE-NOT-A-REAL-TOKEN");

        // The first scripted response is empty, which one page fetch
        // consumes and confirms the script is read in order.
        let first: Vec<_> = client
            .list::<Item>(&token, GraphRequest::new("/first"), "first page")
            .collect();
        assert_eq!(first.len(), 0);

        // The second scripted response is a 429 the client retries past
        // (`Retry-After: 0`), landing on the third scripted response.
        let second: Vec<Result<Item, GraphError>> = client
            .list::<Item>(&token, GraphRequest::new("/second"), "second page")
            .collect();
        assert_eq!(second.len(), 1);
        assert!(second[0].is_ok());
    }
}
