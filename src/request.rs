use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    str::FromStr,
};

use crate::common::{HttpError, HttpMessageContent, HttpStream};
use anyhow::{anyhow, Context, Result};

#[derive(Debug, enum_utils::FromStr, Clone, Copy, PartialEq)]
pub enum HttpRequestMethod {
    OPTIONS,
    GET,
    HEAD,
    POST,
    PUT,
    DELETE,
    TRACE,
    CONNECT,
}

pub struct HttpRequestLine {
    method: HttpRequestMethod,
    resource: String,
    version: String,
}

impl HttpRequestLine {
    pub fn new(method: HttpRequestMethod, resource: String, version: String) -> Self {
        Self {
            method,
            resource,
            version,
        }
    }
}

pub struct HttpRequest {
    request_line: HttpRequestLine,
    content: HttpMessageContent,
}

impl HttpRequest {
    pub fn get_method(&self) -> HttpRequestMethod {
        return self.request_line.method;
    }

    pub fn get_resource(&self) -> String {
        return self.request_line.resource.clone();
    }

    pub fn get_version(&self) -> String {
        return self.request_line.version.clone();
    }

    pub fn content(&self) -> &HttpMessageContent {
        &self.content
    }
}

pub struct HttpRequestBuilder(HttpRequest);
impl HttpRequestBuilder {
    pub fn new(request_line: HttpRequestLine) -> Self {
        Self(HttpRequest {
            request_line,
            content: HttpMessageContent::new(HashMap::new(), Vec::new()),
        })
    }

    pub fn header(
        mut self,
        header_name: impl Into<String>,
        header_content: impl Into<String>,
    ) -> Self {
        self.0
            .content
            .add_header(header_name.into(), header_content.into());
        self
    }

    pub fn body(mut self, body: &[u8]) -> Self {
        self.0.content.set_body(Vec::from(body));
        let body_length = self.0.content.get_body().len();
        self.header("content-length", body_length.to_string())
    }

    pub fn build(self) -> HttpRequest {
        self.0
    }
}

fn get_http_version(version_line: &str) -> Result<String> {
    let version = ["1.1"]
        .iter()
        .find(|&&version| version_line.ends_with(version))
        .ok_or_else(|| HttpError::UnsupportedHttpVersion(version_line.to_string()))?;
    return Ok(version.to_string());
}

fn parse_header(header: &String) -> Result<(String, String)> {
    let Some(header_parsed) = header.split_once(':') else {
        return Err(anyhow!(HttpError::WrongHeaderFormat));
    };

    return Ok((
        header_parsed.0.trim().to_ascii_lowercase(),
        header_parsed.1.trim().to_string(),
    ));
}

pub fn parse_http_request(mut stream: &mut impl HttpStream) -> Result<HttpRequest> {
    let mut buf_reader = BufReader::new(&mut stream);
    let mut http_request = Vec::new();

    loop {
        let mut line = String::new();
        // TODO: Infinite loop possible if EOF is never provide
        buf_reader
            .read_line(&mut line)
            .context("Encountered invalid UTF8 while parsing HTTP request.")?;

        let trimmed = line.trim_end().to_string();
        if trimmed.is_empty() {
            break;
        }
        http_request.push(trimmed);
    }

    if http_request.is_empty() {
        return Err(anyhow!(HttpError::EmptyHttpRequest));
    }

    let mut start_line = http_request[0].split_ascii_whitespace();
    let (Some(method), Some(resource), Some(version)) =
        (start_line.next(), start_line.next(), start_line.next())
    else {
        return Err(anyhow!(HttpError::MalformedRequestLine(
            http_request[0].to_string()
        )));
    };

    // TODO: verify resource
    let method = HttpRequestMethod::from_str(method)
        .map_err(|_| anyhow!(HttpError::InvalidMethodType(method.to_string())))?;
    let version = get_http_version(version)?;

    // TODO: Eliminate security loophole, attacker can send unlimited amount of http headers
    let headers = http_request
        .iter()
        .skip(1)
        .map(|header| parse_header(header))
        .collect::<Result<HashMap<String, String>, _>>()?;

    let content_length: usize = if let Some(content_length) = headers.get("content-length") {
        content_length
            .parse::<usize>()
            .context("Invalid content-length value.")?
    } else {
        0
    };

    // TODO: this might lead to OS stack overflow
    let mut body = vec![0; content_length];
    buf_reader
        .read_exact(&mut body)
        .context("Failed to read parse body of Http request")?;

    return Ok(HttpRequest {
        request_line: HttpRequestLine::new(method, resource.to_string(), version),
        content: HttpMessageContent::new(headers, body),
    });
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use super::*;

    #[test]
    fn parse_invalid_request() {
        let invalid_requests = [
            "",
            "GET HTTP/1.1",
            "INVALID / HTTP/1.1",
            "GET / HTTP/1.1\r\nContent-Type :: text/plain\r\n\r\n",
        ];

        // TODO: match on error type if possible
        let mut stream = Cursor::new(Vec::new());
        for request in invalid_requests {
            let stream_res = stream.write(request.as_bytes());
            assert!(stream_res.is_ok());
            let result = parse_http_request(&mut stream);
            assert!(result.is_err());
        }
    }

    #[test]
    fn parse_get_request() {
        let request =
            b"GET /index.html HTTP/1.1\r\nHost: example.com\r\nContent-Length: 5\r\n\r\nHello";
        let mut stream = Cursor::new(request.to_vec());
        let result = parse_http_request(&mut stream);
        assert!(result.is_ok());

        let parsed_request = result.unwrap();
        assert_eq!(parsed_request.request_line.method, HttpRequestMethod::GET);
        assert_eq!(parsed_request.request_line.resource, "/index.html");
        assert_eq!(parsed_request.request_line.version, "1.1");
        assert_eq!(
            parsed_request.content.get_header("host").unwrap(),
            "example.com"
        );
        assert_eq!(
            parsed_request.content.get_header("content-length").unwrap(),
            "5"
        );
        assert_eq!(parsed_request.content.get_body(), b"Hello");
    }
}
