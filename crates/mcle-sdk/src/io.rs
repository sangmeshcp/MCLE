use bytes::Bytes;
use serde_json::Value;

#[derive(Clone, Debug, Default)]
pub struct RequestHead {
    pub method: String,
    pub path: String,
    pub query: String,
    pub host: String,
    pub headers: Vec<(String, String)>,
}

impl RequestHead {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, Default)]
pub struct OpInput {
    pub method: String,
    pub path: String,
    pub query: Vec<(String, String)>,
    pub path_params: Vec<(String, String)>,
    pub headers: Vec<(String, String)>,
    pub body: Value,
    pub raw: Bytes,
}

impl OpInput {
    pub fn param(&self, name: &str) -> Option<&str> {
        self.path_params
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    pub fn query(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
    pub fn body_str(&self, field: &str) -> Option<&str> {
        self.body.get(field).and_then(Value::as_str)
    }
    pub fn lookup(&self, name: &str) -> Option<String> {
        self.param(name)
            .or_else(|| self.query(name))
            .map(str::to_string)
            .or_else(|| self.body_str(name).map(str::to_string))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Empty,
    Json(Value),
    Text { content_type: String, data: String },
    Bytes { content_type: String, data: Bytes },
}

#[derive(Clone, Debug, PartialEq)]
pub struct OpOutput {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
}

impl OpOutput {
    pub fn json(value: Value) -> Self {
        Self::json_status(200, value)
    }
    pub fn json_status(status: u16, value: Value) -> Self {
        OpOutput {
            status,
            headers: Vec::new(),
            body: Body::Json(value),
        }
    }
    pub fn empty(status: u16) -> Self {
        OpOutput {
            status,
            headers: Vec::new(),
            body: Body::Empty,
        }
    }
    pub fn with_header(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.headers.push((k.into(), v.into()));
        self
    }
}
