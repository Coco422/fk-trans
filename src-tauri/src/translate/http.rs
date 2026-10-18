//! Shared HTTP client for every translation provider.
//!
//! `reqwest` defaults to *no* timeout at all. Before this module existed, a hung
//! upstream (a stalled TCP connection, a proxy that accepts and never answers)
//! would keep the translation pipeline's single-flight gate occupied forever, so
//! every later middle-click was silently dropped as "previous pipeline still
//! running" while the popup sat in its loading state. A request must always be
//! able to end on its own.

use reqwest::{Client, ClientBuilder};
use std::sync::OnceLock;
use std::time::Duration;

/// Timeouts applied to a client. Split out so tests can build a client with a
/// short budget and exercise the exact same construction path as production.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    pub connect: Duration,
    pub request: Duration,
}

/// Production budget. The request ceiling is deliberately generous because a
/// local model on CPU can legitimately take a while — it exists to guarantee the
/// pipeline gate gets released, not to bound normal latency.
pub const DEFAULT_TIMEOUTS: Timeouts = Timeouts {
    connect: Duration::from_secs(10),
    request: Duration::from_secs(60),
};

pub fn build_client(timeouts: Timeouts) -> Client {
    configured_builder(timeouts)
        .build()
        .expect("building an HTTP client with the default TLS backend cannot fail")
}

fn configured_builder(timeouts: Timeouts) -> ClientBuilder {
    Client::builder()
        .connect_timeout(timeouts.connect)
        .timeout(timeouts.request)
}

/// Build a client with an explicit budget, sharing the production connection
/// pool. `Client` is reference-counted internally, so callers get a clone.
pub fn shared_client_with(timeouts: Timeouts) -> Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    if timeouts == DEFAULT_TIMEOUTS {
        return CLIENT.get_or_init(|| build_client(timeouts)).clone();
    }
    build_client(timeouts)
}

/// The process-wide client. Providers used to call `Client::new()` on every
/// construction — and `reload_provider` rebuilds a provider on every settings
/// save — so the connection pool was thrown away each time.
pub fn shared_client() -> Client {
    shared_client_with(DEFAULT_TIMEOUTS)
}

use crate::translate::provider::TranslateError;
use reqwest::StatusCode;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

fn stable_hash(value: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// Names the top-level keys of a JSON object. Used to describe a response we
/// could not make sense of without quoting it.
pub fn top_level_keys(json: &serde_json::Value) -> String {
    json.as_object()
        .map(|object| object.keys().cloned().collect::<Vec<_>>().join(","))
        .unwrap_or_else(|| "non_object".to_string())
}

/// A description of a body that is safe to show a user.
///
/// Deliberately not the body. Four of the six providers used to interpolate the
/// whole upstream response into the error, which then travelled to the popup
/// verbatim — and an error payload can quote the text the user selected, which
/// is exactly what this app is supposed to keep to itself.
pub fn describe_body(body: &str) -> String {
    format!("{} bytes, hash {:016x}", body.len(), stable_hash(body))
}

/// Pull a diagnostic message out of an upstream JSON error payload.
fn extract_api_error(json: &serde_json::Value) -> Option<String> {
    let error = json.get("error")?;
    if let Some(message) = error.as_str() {
        return Some(message.to_string());
    }

    let message = error
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("upstream API error");
    let mut parts = vec![message.to_string()];
    for (label, key) in [("code", "code"), ("type", "type")] {
        if let Some(value) = error.get(key).and_then(|value| value.as_str()) {
            parts.push(format!("{label}={value}"));
        }
    }
    Some(parts.join(" "))
}

/// The message for a non-success status.
///
/// A JSON error payload is mined for its own words — providers say useful things
/// there ("insufficient quota", "invalid api key") and it is their message, not
/// a document we just read. Anything else is described, never quoted: an HTML
/// error page or a stack trace is noise at best.
pub fn error_message(status: StatusCode, body: &str) -> String {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        if let Some(message) = extract_api_error(&json) {
            return format!("HTTP {status}: {message}");
        }
    }
    format!("HTTP {status}: non-JSON response, {}", describe_body(body))
}

pub struct HttpResponse {
    pub status: StatusCode,
    body: String,
}

impl HttpResponse {
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Parse the body, reporting its shape rather than its contents on failure.
    pub fn json(&self) -> Result<serde_json::Value, TranslateError> {
        serde_json::from_str(&self.body).map_err(|error| {
            TranslateError::Api(format!(
                "HTTP {} returned unparseable JSON ({error}); {}",
                self.status,
                describe_body(&self.body)
            ))
        })
    }
}

/// Map a non-success status to the right error variant.
///
/// A 429 is worth distinguishing: it is the one failure where waiting and trying
/// again is the right advice, and `TranslateError::RateLimited` existed without
/// ever being constructed until this did so.
fn status_error(status: StatusCode, body: &str) -> TranslateError {
    let message = error_message(status, body);
    if status == StatusCode::TOO_MANY_REQUESTS {
        TranslateError::RateLimited(message)
    } else {
        TranslateError::Api(message)
    }
}

/// Send a request, turning transport failures and non-success statuses into a
/// `TranslateError` whose message is safe to show.
///
/// This replaced six near-identical hand-written blocks, four of which were
/// character-for-character the same.
pub async fn send(request: reqwest::RequestBuilder) -> Result<HttpResponse, TranslateError> {
    let response = request
        .send()
        .await
        .map_err(|error| TranslateError::Network(error.to_string()))?;

    let status = response.status();
    // A read failure is reported rather than silently degraded to an empty body,
    // which is how `unwrap_or_default()` here turned into "successfully
    // translated to nothing".
    let body = response
        .text()
        .await
        .map_err(|error| TranslateError::Network(format!("could not read the response: {error}")))?;

    if !status.is_success() {
        return Err(status_error(status, &body));
    }

    Ok(HttpResponse { status, body })
}

/// The extracted translation, or an error naming the provider and the shape that
/// disappointed it.
///
/// Five of the six providers used `.unwrap_or("")` on the response path, so a
/// malformed or blocked reply became `Ok("")` and the popup rendered a blank
/// success. A response that does not contain a translation is a failure.
pub fn require_translation(
    extracted: Option<String>,
    provider: &str,
    shape: impl FnOnce() -> String,
) -> Result<String, TranslateError> {
    match extracted {
        Some(text) if !text.trim().is_empty() => Ok(text),
        _ => Err(TranslateError::Api(format!(
            "{provider} response contained no translation ({})",
            shape()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard: the production budget must stay finite. `None`/zero
    /// would silently restore the "a hung upstream wedges the app" behaviour.
    #[test]
    fn default_timeouts_are_finite_and_ordered() {        assert!(!DEFAULT_TIMEOUTS.connect.is_zero());
        assert!(!DEFAULT_TIMEOUTS.request.is_zero());
        assert!(
            DEFAULT_TIMEOUTS.connect <= DEFAULT_TIMEOUTS.request,
            "a connect budget longer than the whole request budget can never be used"
        );
    }

    /// Providers do say useful things in their JSON error payloads, and it is
    /// their message rather than a document we happen to have read.
    #[test]
    fn a_json_error_payload_is_mined_for_its_own_message() {
        let message = error_message(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"insufficient quota","code":"quota_exhausted"}}"#,
        );

        assert!(message.contains("429"), "got: {message}");
        assert!(message.contains("insufficient quota"), "got: {message}");
        assert!(message.contains("code=quota_exhausted"), "got: {message}");
    }

    /// The regression this replaced: four providers interpolated the whole
    /// response body into the error, which then travelled verbatim to the popup.
    /// A non-JSON body is described, never quoted.
    #[test]
    fn a_non_json_error_body_is_never_quoted_back() {
        // No fragment below may appear in the HTTP status's own reason phrase, so
        // a match can only have come from the body.
        let body = "<html><body>quota exhausted for account-42; trace=deadbeef</body></html>";
        let message = error_message(StatusCode::BAD_GATEWAY, body);

        assert!(message.contains("502"), "got: {message}");
        for fragment in ["<html>", "quota exhausted", "account-42", "deadbeef"] {
            assert!(
                !message.contains(fragment),
                "the body leaked into the message via {fragment:?}: {message}"
            );
        }
        assert!(message.contains("bytes"), "got: {message}");
    }

    #[test]
    fn describe_body_reports_size_and_hash_not_content() {
        let described = describe_body("the quick brown fox");

        assert!(!described.contains("quick"), "got: {described}");
        assert!(described.contains("19 bytes"), "got: {described}");
        assert!(described.contains("hash"), "got: {described}");
    }

    #[test]
    fn the_same_body_always_describes_to_the_same_hash() {
        assert_eq!(describe_body("same"), describe_body("same"));
        assert_ne!(describe_body("one"), describe_body("other"));
    }

    /// The regression: `.unwrap_or("")` made a malformed or blocked reply look
    /// like a successful empty translation.
    #[test]
    fn a_response_without_a_translation_is_an_error() {
        for missing in [None, Some(String::new()), Some("   \n ".to_string())] {
            let error = require_translation(missing, "openai", || "top_level_keys=foo".to_string())
                .expect_err("an empty translation must not be success");
            let message = error.to_string();
            assert!(message.contains("openai"), "got: {message}");
            assert!(message.contains("no translation"), "got: {message}");
            assert!(message.contains("top_level_keys=foo"), "got: {message}");
        }
    }

    #[test]
    fn a_response_with_a_translation_is_returned_unchanged() {
        let text = require_translation(Some("你好，世界".to_string()), "openai", || unreachable!())
            .expect("a real translation");
        assert_eq!(text, "你好，世界");
    }

    /// `RateLimited` was a variant nothing ever constructed, so `kind()` could
    /// never report "rate_limited" and the diagnostics could never say the one
    /// thing a user should do about it: wait.
    #[test]
    fn a_429_is_classified_as_rate_limited() {
        let error = status_error(
            StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"slow down"}}"#,
        );

        assert_eq!(error.kind(), "rate_limited");
        assert!(error.to_string().contains("slow down"), "got: {error}");
    }

    #[test]
    fn other_failures_are_plain_api_errors() {
        for status in [
            StatusCode::UNAUTHORIZED,
            StatusCode::BAD_GATEWAY,
            StatusCode::INTERNAL_SERVER_ERROR,
        ] {
            let error = status_error(status, "{}");
            assert_eq!(error.kind(), "api", "{status} should not be rate_limited");
        }
    }

    /// Regression guard: a request against an upstream that accepts the
    /// connection and then never answers must fail on its own.
    ///
    /// Before the timeout was configured this call never returned, which is what
    /// made the pipeline gate stay occupied permanently.
    #[tokio::test]
    async fn request_times_out_against_a_silent_upstream() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("read loopback addr");

        // Accept connections and deliberately never write a response.
        std::thread::spawn(move || {
            let mut held = Vec::new();
            while let Ok(stream) = listener.accept() {
                held.push(stream);
            }
        });

        let budget = Timeouts {
            connect: Duration::from_secs(2),
            request: Duration::from_millis(700),
        };
        let hard_ceiling = budget.request + Duration::from_secs(5);

        let started = std::time::Instant::now();
        let outcome = tokio::time::timeout(
            hard_ceiling,
            shared_client_with(budget)
                .get(format!("http://{addr}/"))
                .send(),
        )
        .await;
        let elapsed = started.elapsed();

        let error = match outcome {
            Err(_) => panic!(
                "request to a silent upstream hung for {hard_ceiling:?}; \
                 the shared client is not enforcing a request timeout"
            ),
            Ok(Ok(response)) => panic!("expected a timeout, got HTTP {}", response.status()),
            Ok(Err(error)) => error,
        };

        assert!(error.is_timeout(), "expected a timeout error, got: {error}");
        assert!(
            elapsed < budget.request + Duration::from_secs(2),
            "request ended after {elapsed:?}, far past the {:?} budget",
            budget.request
        );
    }
}
