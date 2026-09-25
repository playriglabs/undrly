//! HTTP transport for SEC EDGAR (feature `http`).
//!
//! Conservative by design, following SEC's fair-access guidance: one request
//! per call, no concurrency, no retries, no redirects, a request timeout, a
//! response size limit, and a descriptive `User-Agent` naming who is making
//! the request and how to contact them. The response body is returned exactly
//! as received: no content encoding is negotiated, so the bytes are the
//! document SEC served.

use std::time::Duration;

use undrly_core::{Cik, Timestamp};

/// Base URL of SEC's JSON data APIs.
pub const DATA_BASE_URL: &str = "https://data.sec.gov";

const TIMEOUT: Duration = Duration::from_secs(30);
/// NVIDIA's submissions document is about 160 KB; the limit only guards
/// against unexpected responses.
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

/// A `User-Agent` suitable for SEC requests: printable ASCII naming the
/// requester and including a contact email, e.g.
/// `Example Corp admin@example.com`. SEC blocks undeclared automated access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecUserAgent(String);

impl SecUserAgent {
    pub fn new(value: &str) -> Result<Self, FetchError> {
        let valid = value.trim() == value
            && value.len() <= 256
            && value.bytes().all(|b| (b' '..=b'~').contains(&b))
            && value.contains(' ')
            && value.split(' ').any(|word| {
                word.split_once('@')
                    .is_some_and(|(user, domain)| !user.is_empty() && domain.contains('.'))
            });
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(FetchError::InvalidUserAgent(value.to_owned()))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("SEC User-Agent must name the requester and include a contact email, got `{0}`")]
    InvalidUserAgent(String),
    #[error("request to {url} failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{url} returned HTTP {status}")]
    Status { url: String, status: u16 },
    #[error("{url} returned more than {limit} bytes")]
    TooLarge { url: String, limit: usize },
}

/// A response exactly as received, with upstream request metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedRecord {
    /// The requested URL; identifies the upstream record.
    pub url: String,
    /// Response body bytes, unmodified.
    pub body: Vec<u8>,
    /// When the full body had been received.
    pub received_at: Timestamp,
    /// SEC's request id (`x-amzn-requestid`), when present.
    pub request_id: Option<String>,
    /// The response `Date` header, when present.
    pub date: Option<String>,
}

/// Fetches EDGAR documents. Holds no state beyond its HTTP client.
pub struct SecClient {
    http: reqwest::Client,
    base_url: String,
}

impl SecClient {
    pub fn new(user_agent: SecUserAgent) -> Result<Self, FetchError> {
        Self::with_base_url(user_agent, DATA_BASE_URL)
    }

    /// A client for another base URL (a local test server).
    pub fn with_base_url(
        user_agent: SecUserAgent,
        base_url: impl Into<String>,
    ) -> Result<Self, FetchError> {
        let base_url = base_url.into();
        let http = reqwest::Client::builder()
            .user_agent(user_agent.0)
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|source| FetchError::Request {
                url: base_url.clone(),
                source,
            })?;
        Ok(Self { http, base_url })
    }

    /// URL of a filer's submissions document.
    pub fn submissions_url(&self, cik: &Cik) -> String {
        format!("{}/submissions/CIK{}.json", self.base_url, cik.as_str())
    }

    /// Fetches a filer's submissions document. Any non-200 status is an error;
    /// the body of an error response is not returned.
    pub async fn fetch_submissions(&self, cik: &Cik) -> Result<FetchedRecord, FetchError> {
        let url = self.submissions_url(cik);
        let request_error = |source| FetchError::Request {
            url: url.clone(),
            source,
        };
        let mut response = self
            .http
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(request_error)?;
        let status = response.status().as_u16();
        if status != 200 {
            return Err(FetchError::Status { url, status });
        }
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let request_id = header("x-amzn-requestid");
        let date = header("date");
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                return Err(FetchError::TooLarge {
                    url,
                    limit: MAX_BODY_BYTES,
                });
            }
            body.extend_from_slice(&chunk);
        }
        Ok(FetchedRecord {
            url,
            body,
            received_at: Timestamp::now(),
            request_id,
            date,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_must_declare_requester_and_contact() {
        for valid in [
            "Example Corp admin@example.com",
            "Undrly research ops@undrly.xyz",
        ] {
            assert_eq!(SecUserAgent::new(valid).unwrap().as_str(), valid);
        }
        for invalid in [
            "",
            "admin@example.com",
            "Example Corp",
            "Example Corp admin@localhost",
            "Example Corp @example.com",
            " Example Corp admin@example.com",
            "Example Corp admin@example.com\n",
            "Exämple Corp admin@example.com",
        ] {
            assert!(SecUserAgent::new(invalid).is_err(), "{invalid:?}");
        }
    }

    #[test]
    fn submissions_url_uses_the_padded_cik() {
        let client =
            SecClient::new(SecUserAgent::new("Example Corp admin@example.com").unwrap()).unwrap();
        assert_eq!(
            client.submissions_url(&Cik::normalize("1045810").unwrap()),
            "https://data.sec.gov/submissions/CIK0001045810.json"
        );
    }
}
