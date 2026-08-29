use std::{collections::BTreeMap, fmt, time::Duration};

use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use url::Url;

use crate::SecretString;

pub const MAX_RESPONSE_BYTES: usize = 32 * 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HttpMethod {
    Post,
}

pub enum TransportAuth<'secret> {
    Bearer(&'secret SecretString),
}

impl fmt::Debug for TransportAuth<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Bearer([REDACTED])")
    }
}

pub struct HttpRequest<'secret> {
    pub method: HttpMethod,
    pub url: &'secret Url,
    pub auth: TransportAuth<'secret>,
    pub json_body: &'secret [u8],
}

impl fmt::Debug for HttpRequest<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpRequest")
            .field("method", &self.method)
            .field("origin", &self.url.origin().ascii_serialization())
            .field("path", &self.url.path())
            .field("auth", &self.auth)
            .field("json_body_bytes", &self.json_body.len())
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl fmt::Debug for HttpResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpResponse")
            .field("status", &self.status)
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("provider connection failed")]
    Connect,
    #[error("provider transport timed out")]
    Timeout,
    #[error("provider transport failed")]
    Other,
    #[error("provider response exceeded the bounded size")]
    ResponseTooLarge,
}

#[async_trait]
pub trait HttpTransport: Send + Sync {
    async fn execute(&self, request: HttpRequest<'_>) -> Result<HttpResponse, TransportError>;
}

#[derive(Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
}

impl ReqwestTransport {
    pub fn new(connect_timeout: Duration) -> Result<Self, TransportError> {
        let client = reqwest::Client::builder()
            .connect_timeout(connect_timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("interactive-llm-powered-npcs/2")
            .build()
            .map_err(|_| TransportError::Other)?;
        Ok(Self { client })
    }
}

impl fmt::Debug for ReqwestTransport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ReqwestTransport")
    }
}

#[async_trait]
impl HttpTransport for ReqwestTransport {
    async fn execute(&self, request: HttpRequest<'_>) -> Result<HttpResponse, TransportError> {
        let mut builder = match request.method {
            HttpMethod::Post => self.client.post(request.url.clone()),
        }
        .header(CONTENT_TYPE, "application/json");
        match request.auth {
            TransportAuth::Bearer(secret) => {
                builder = builder.bearer_auth(secret.expose());
            }
        }

        let response = builder
            .body(request.json_body.to_vec())
            .send()
            .await
            .map_err(classify_reqwest_error)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(TransportError::ResponseTooLarge);
        }
        let status = response.status().as_u16();
        let headers = sanitized_headers(response.headers());
        let body = response
            .bytes()
            .await
            .map_err(classify_reqwest_error)?
            .to_vec();
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(TransportError::ResponseTooLarge);
        }
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

fn classify_reqwest_error(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else if error.is_connect() {
        TransportError::Connect
    } else {
        TransportError::Other
    }
}

fn sanitized_headers(headers: &HeaderMap<HeaderValue>) -> BTreeMap<String, String> {
    ["retry-after", "x-request-id", "request-id"]
        .into_iter()
        .filter_map(|name| {
            let header_name = HeaderName::from_static(name);
            let value = headers
                .get(header_name)?
                .to_str()
                .ok()?
                .chars()
                .take(256)
                .collect::<String>();
            Some((name.to_owned(), value))
        })
        .collect()
}
