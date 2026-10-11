//! TV web route only (wasm32): the small part of `ureq`'s API that the core uses, done with synchronous
//! `XMLHttpRequest` (the page has no sockets). Call sites stay unchanged through `use crate::webhttp as ureq`.
//! Synchronous XHR on the page thread cannot set a timeout or a binary `responseType`, so bodies come back
//! as `x-user-defined` text and are turned back into bytes here.
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt;
use std::time::Duration;
use web_sys::XmlHttpRequest;

#[derive(Debug)]
pub enum Error {
    StatusCode(u16),
    Io(String),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::StatusCode(c) => write!(f, "http status: {c}"),
            Error::Io(s) => write!(f, "{s}"),
        }
    }
}
impl std::error::Error for Error {}

pub mod http {
    pub struct StatusCode(pub u16);
    impl StatusCode {
        pub fn as_u16(&self) -> u16 {
            self.0
        }
    }
    pub struct Response<B> {
        pub(super) status: u16,
        pub(super) body: B,
    }
    impl<B> Response<B> {
        pub fn status(&self) -> StatusCode {
            StatusCode(self.status)
        }
        pub fn body_mut(&mut self) -> &mut B {
            &mut self.body
        }
    }
}

pub struct Body(Vec<u8>);
pub struct BodyWithConfig<'a>(&'a mut Body, usize);
impl Body {
    pub fn read_json<T: DeserializeOwned>(&mut self) -> Result<T, Error> {
        serde_json::from_slice(&self.0).map_err(|e| Error::Io(e.to_string()))
    }
    pub fn read_to_string(&mut self) -> Result<String, Error> {
        String::from_utf8(std::mem::take(&mut self.0)).map_err(|e| Error::Io(e.to_string()))
    }
    pub fn read_to_vec(&mut self) -> Result<Vec<u8>, Error> {
        Ok(std::mem::take(&mut self.0))
    }
    pub fn with_config(&mut self) -> BodyWithConfig<'_> {
        BodyWithConfig(self, usize::MAX)
    }
}
impl BodyWithConfig<'_> {
    pub fn limit(self, n: u64) -> Self {
        BodyWithConfig(self.0, n as usize)
    }
    pub fn read_to_vec(self) -> Result<Vec<u8>, Error> {
        if self.0 .0.len() > self.1 {
            return Err(Error::Io("body over limit".into()));
        }
        self.0.read_to_vec()
    }
}

#[derive(Clone)]
pub struct Agent {
    status_as_error: bool,
}
pub struct Config(bool);
pub struct ConfigBuilder(bool);
impl Agent {
    pub fn config_builder() -> ConfigBuilder {
        ConfigBuilder(true)
    }
    pub fn get(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(self, "GET", url)
    }
    pub fn post(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(self, "POST", url)
    }
    pub fn delete(&self, url: &str) -> RequestBuilder {
        RequestBuilder::new(self, "DELETE", url)
    }
}
impl ConfigBuilder {
    pub fn http_status_as_error(self, on: bool) -> Self {
        ConfigBuilder(on)
    }
    /// Ignored: synchronous XHR on the page thread cannot time out.
    pub fn timeout_global(self, _: Option<Duration>) -> Self {
        self
    }
    pub fn timeout_connect(self, _: Option<Duration>) -> Self {
        self
    }
    pub fn build(self) -> Config {
        Config(self.0)
    }
}
impl From<Config> for Agent {
    fn from(c: Config) -> Agent {
        Agent { status_as_error: c.0 }
    }
}

/// `ureq::get` (default agent: an HTTP error status is an `Error`).
pub fn get(url: &str) -> RequestBuilder {
    Agent { status_as_error: true }.get(url)
}

pub struct RequestBuilder {
    status_as_error: bool,
    method: &'static str,
    url: String,
    headers: Vec<(String, String)>,
}
impl RequestBuilder {
    fn new(agent: &Agent, method: &'static str, url: &str) -> RequestBuilder {
        RequestBuilder { status_as_error: agent.status_as_error, method, url: url.to_string(), headers: vec![] }
    }
    pub fn header(mut self, k: &str, v: &str) -> Self {
        self.headers.push((k.to_string(), v.to_string()));
        self
    }
    /// Per-request config (only `timeout_global`, ignored).
    pub fn config(self) -> Self {
        self
    }
    pub fn timeout_global(self, _: Option<Duration>) -> Self {
        self
    }
    pub fn build(self) -> Self {
        self
    }
    pub fn call(self) -> Result<http::Response<Body>, Error> {
        self.run(None)
    }
    pub fn send_empty(self) -> Result<http::Response<Body>, Error> {
        self.run(None)
    }
    pub fn send_json(mut self, body: &impl Serialize) -> Result<http::Response<Body>, Error> {
        let s = serde_json::to_string(body).map_err(|e| Error::Io(e.to_string()))?;
        self.headers.push(("Content-Type".into(), "application/json".into()));
        self.run(Some(s))
    }

    fn run(self, body: Option<String>) -> Result<http::Response<Body>, Error> {
        let js = |e: wasm_bindgen::JsValue| Error::Io(format!("xhr: {e:?}"));
        let x = XmlHttpRequest::new().map_err(js)?;
        x.open_with_async(self.method, &self.url, false).map_err(js)?;
        x.override_mime_type("text/plain; charset=x-user-defined").map_err(js)?;
        for (k, v) in &self.headers {
            x.set_request_header(k, v).map_err(js)?;
        }
        // The page's HTTP cache would answer API GETs with stale data (watch state, resume points); only image
        // URLs (they carry a tag) may come from it.
        if !self.url.contains("/Images/") {
            x.set_request_header("Cache-Control", "no-cache").map_err(js)?;
        }
        match &body {
            Some(b) => x.send_with_opt_str(Some(b)).map_err(js)?,
            None => x.send().map_err(js)?,
        }
        let status = x.status().map_err(js)?;
        if status == 0 {
            return Err(Error::Io(format!("network error: {}", self.url)));
        }
        if self.status_as_error && status >= 400 {
            return Err(Error::StatusCode(status));
        }
        // x-user-defined: byte b >= 0x80 arrives as U+F700 + b; the low byte is the original byte.
        let text = x.response_text().map_err(js)?.unwrap_or_default();
        let bytes = text.chars().map(|c| c as u32 as u8).collect();
        Ok(http::Response { status, body: Body(bytes) })
    }
}
