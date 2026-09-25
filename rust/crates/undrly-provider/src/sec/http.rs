//! HTTP transport for SEC EDGAR (feature `http`).
//!
//! Conservative by design, following SEC's fair-access guidance: one request
//! per call, no concurrency, no retries, no redirects, a request timeout, a
//! response size limit, and a descriptive `User-Agent` naming who is making
//! the request and how to contact them. The response body is returned exactly
//! as received: no content encoding is negotiated, so the bytes are the
//! document SEC served.

use undrly_core::Cik;

use crate::http::HttpClient;

/// Base URL of SEC's JSON data APIs.
pub const DATA_BASE_URL: &str = "https://data.sec.gov";

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

pub use crate::http::{FetchError, FetchedRecord};

/// Fetches EDGAR documents.
pub struct SecClient {
    http: HttpClient,
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
        Ok(Self {
            http: HttpClient::new(&user_agent.0)?,
            base_url: base_url.into(),
        })
    }

    /// URL of a filer's submissions document.
    pub fn submissions_url(&self, cik: &Cik) -> String {
        format!("{}/submissions/CIK{}.json", self.base_url, cik.as_str())
    }

    /// Fetches a filer's submissions document. Any non-200 status is an error;
    /// the body of an error response is not returned.
    pub async fn fetch_submissions(&self, cik: &Cik) -> Result<FetchedRecord, FetchError> {
        self.http.get(&self.submissions_url(cik), &[]).await
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
