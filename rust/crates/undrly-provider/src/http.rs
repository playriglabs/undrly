//! HTTP transport shared by real sources (feature `http`). The only network
//! code in the workspace.
//!
//! Conservative by design: one request per call, no concurrency, no retries,
//! no redirects, a request timeout, a response size limit, and a declared
//! `User-Agent`. The response body is returned exactly as received: no
//! content encoding is negotiated, so the bytes are the document the source
//! served. Credentials are sent as headers and never appear in the returned
//! record.

use std::time::Duration;

use undrly_core::Timestamp;

const TIMEOUT: Duration = Duration::from_secs(30);
/// Guards against unexpected responses; the largest expected document
/// (SEC submissions) is well under 1 MB.
const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

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
    /// The response contains a credential sent with the request; it is
    /// discarded, never stored.
    #[error("{url}: the response echoes a request credential; discarded")]
    CredentialEchoed { url: String },
}

/// A response exactly as received, with request metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedRecord {
    /// The requested URL.
    pub url: String,
    /// Identifies the upstream record: the URL for a GET, `POST <url> <body>`
    /// for a POST. Never contains credentials.
    pub record_key: String,
    /// Response body bytes, unmodified.
    pub body: Vec<u8>,
    /// When the full body had been received.
    pub received_at: Timestamp,
    /// The upstream's request id header (`x-amzn-requestid`, `x-request-id`),
    /// when present.
    pub request_id: Option<String>,
    /// The response `Date` header, when present.
    pub date: Option<String>,
    /// Wall-clock duration of the request, for latency reporting.
    pub elapsed: Duration,
}

pub struct HttpClient {
    http: reqwest::Client,
}

impl HttpClient {
    pub fn new(user_agent: &str) -> Result<Self, FetchError> {
        let http = reqwest::Client::builder()
            .user_agent(user_agent)
            .timeout(TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|source| FetchError::Request {
                url: String::new(),
                source,
            })?;
        Ok(Self { http })
    }

    /// GET `url` with extra `headers` (e.g. credentials).
    pub async fn get(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<FetchedRecord, FetchError> {
        self.get_accepting(url, "application/json", headers).await
    }

    /// GET `url` accepting the media type `accept` (XML, CSV, or a vendor
    /// type such as BNM's `application/vnd.BNM.API.v1+json`).
    pub async fn get_accepting(
        &self,
        url: &str,
        accept: &str,
        headers: &[(&str, &str)],
    ) -> Result<FetchedRecord, FetchError> {
        let mut request = self.http.get(url).header(reqwest::header::ACCEPT, accept);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        fetch(request, url, url.to_owned()).await
    }

    /// GET `url`, identified as `record_key` (a URL with credentials
    /// removed). The response is discarded if it contains `secret`.
    pub async fn get_secret_query(
        &self,
        url: &str,
        record_key: &str,
        secret: &str,
    ) -> Result<FetchedRecord, FetchError> {
        let request = self
            .http
            .get(url)
            .header(reqwest::header::ACCEPT, "application/json");
        // Errors carry the URL; report the credential-free key instead.
        let mut fetched = match fetch(request, record_key, record_key.to_owned()).await {
            Ok(f) => f,
            Err(FetchError::Request { url, source }) => {
                return Err(FetchError::Request {
                    url,
                    source: source.without_url(),
                });
            }
            Err(e) => return Err(e),
        };
        if !secret.is_empty()
            && fetched
                .body
                .windows(secret.len())
                .any(|w| w == secret.as_bytes())
        {
            return Err(FetchError::CredentialEchoed {
                url: record_key.to_owned(),
            });
        }
        fetched.url = record_key.to_owned();
        Ok(fetched)
    }

    /// POST a JSON `body` to `url`, identified as `record_key` (for a body too
    /// long to be its own key, e.g. a batched JSON-RPC request that a short
    /// key determines exactly).
    pub async fn post_json_keyed(
        &self,
        url: &str,
        body: &str,
        record_key: &str,
    ) -> Result<FetchedRecord, FetchError> {
        let request = self
            .http
            .post(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_owned());
        fetch(request, url, record_key.to_owned()).await
    }

    /// POST a JSON `body` to `url`.
    pub async fn post_json(&self, url: &str, body: &str) -> Result<FetchedRecord, FetchError> {
        let request = self
            .http
            .post(url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.to_owned());
        fetch(request, url, format!("POST {url} {body}")).await
    }
}

async fn fetch(
    request: reqwest::RequestBuilder,
    url: &str,
    record_key: String,
) -> Result<FetchedRecord, FetchError> {
    let url = url.to_owned();
    let request_error = |source| FetchError::Request {
        url: url.clone(),
        source,
    };
    let started = std::time::Instant::now();
    let mut response = request.send().await.map_err(request_error)?;
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
    let request_id = header("x-amzn-requestid").or_else(|| header("x-request-id"));
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
        record_key,
        body,
        received_at: Timestamp::now(),
        request_id,
        date,
        elapsed: started.elapsed(),
    })
}
