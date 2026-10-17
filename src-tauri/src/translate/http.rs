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

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard: the production budget must stay finite. `None`/zero
    /// would silently restore the "a hung upstream wedges the app" behaviour.
    #[test]
    fn default_timeouts_are_finite_and_ordered() {
        assert!(!DEFAULT_TIMEOUTS.connect.is_zero());
        assert!(!DEFAULT_TIMEOUTS.request.is_zero());
        assert!(
            DEFAULT_TIMEOUTS.connect <= DEFAULT_TIMEOUTS.request,
            "a connect budget longer than the whole request budget can never be used"
        );
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
