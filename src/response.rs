use std::collections::HashMap;

use crate::common::Stream;
use anyhow::Result;

#[derive(Debug, Clone, Copy)]
pub enum HttpResponseStatusCode {
    OK = 200,
    Created = 201,
    NotFound = 404,
}

impl ToString for HttpResponseStatusCode {
    fn to_string(&self) -> String {
        match self {
            HttpResponseStatusCode::NotFound => return String::from("Not Found"),
            _ => return String::from(format!("{:?}", self)),
        }
    }
}

#[derive(Debug)]
pub struct HTTPResponseMessage {
    status_code: HttpResponseStatusCode,
    version: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

pub struct HTTPResponseBuilder(HTTPResponseMessage);
impl HTTPResponseBuilder {
    pub fn new(http_status_code: HttpResponseStatusCode, version: &str) -> Self {
        Self(HTTPResponseMessage {
            status_code: http_status_code,
            version: String::from(version),
            headers: HashMap::new(),
            body: vec![],
        })
    }

    pub fn header(
        mut self,
        header_name: impl Into<String>,
        header_content: impl Into<String>,
    ) -> Self {
        self.0
            .headers
            .insert(header_name.into(), header_content.into());
        self
    }

    pub fn body(mut self, body: &[u8]) -> Self {
        self.0.body = Vec::from(body);
        let body_length = self.0.body.len();
        self.header("content-length", body_length.to_string())
    }

    pub fn build(self) -> HTTPResponseMessage {
        self.0
    }
}

impl HTTPResponseMessage {
    pub fn write_to(&self, stream: &mut impl Stream) -> Result<()> {
        let mut response = format!(
            "HTTP/{} {} {}\r\n",
            self.version,
            self.status_code as u16,
            self.status_code.to_string()
        );

        for (header_name, header_content) in &self.headers {
            response.push_str(&format!("{} : {}\r\n", header_name, header_content));
        }
        response.push_str("\r\n");

        stream.write_all(response.as_bytes())?;
        stream.write_all(&self.body)?;
        Ok(())
    }
}
