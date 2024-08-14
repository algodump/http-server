use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    str::FromStr,
};

use crate::common::{
    ErrorCode, HttpMessageContent, HttpStream, InternalHttpError, MAX_HEADERS_AMOUNT,
    MAX_HEADER_SIZE, MAX_REQUEST_BODY_SIZE, MAX_URI_LENGTH,
};

use anyhow::{anyhow, Context, Result};
use log::trace;

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

#[derive(Debug)]
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

#[derive(Debug)]
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
        .ok_or_else(|| {
            InternalHttpError::KnownError(ErrorCode::HTTPVersionNotSupported)})?;
    
    return Ok(version.to_string());
}

fn parse_header(header: &String) -> Result<(String, String)> {
    if header.len() as u64 > MAX_HEADER_SIZE {
        return Err(anyhow!(InternalHttpError::KnownError(ErrorCode::RequestHeaderFieldsTooLarge)));
    }

    let Some(header_parsed) = header.split_once(':') else {
        return Err(anyhow!(InternalHttpError::WrongHeaderFormat));
    };
    if header_parsed.0.is_empty() || header_parsed.1.is_empty() {
        return Err(anyhow!(InternalHttpError::WrongHeaderFormat));
    }

    trace!("Parsed header: {} {}", header_parsed.0, header_parsed.1);
    return Ok((
        header_parsed.0.trim().to_ascii_lowercase(),
        header_parsed.1.trim().to_string(),
    ));
}

pub fn parse_http_request(mut stream: &mut impl HttpStream) -> Result<HttpRequest> {
    let mut buf_reader = BufReader::new(&mut stream);

    // Parse request line
    let mut request_line = String::new();
    buf_reader
        .read_line(&mut request_line)
        .context(InternalHttpError::InvalidUTF8Char)?;

    let mut request_line_iter = request_line.split_ascii_whitespace();
    let (Some(method), Some(resource), Some(version)) = (
        request_line_iter.next(),
        request_line_iter.next(),
        request_line_iter.next(),
    ) else {
        return Err(anyhow!(InternalHttpError::MalformedRequestLine(
            request_line.to_string()
        )));
    };

    if resource.len() > MAX_URI_LENGTH {
        return Err(anyhow!(InternalHttpError::KnownError(ErrorCode::URITooLong)));
    }

    let method = HttpRequestMethod::from_str(method)
        .map_err(|_| anyhow!(InternalHttpError::KnownError(ErrorCode::NotImplemented)))?;
    let version = get_http_version(version)?;

    // Parse headers
    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut line = String::new();
        // TODO: Infinite loop possible if EOF is never provide
        buf_reader
            .read_line(&mut line)
            .context(InternalHttpError::InvalidUTF8Char)?;
        let trimmed = line.trim_end().to_string();

        if trimmed.is_empty() {
            break;
        }

        let header = parse_header(&line)?;
        headers.insert(header.0, header.1);

        if headers.len() > MAX_HEADERS_AMOUNT {
            return Err(anyhow!(InternalHttpError::HeaderOverflow));
        }
    }

    let content_length = if let Some(content_length) = headers.get("content-length") {
        content_length
            .parse::<u64>()
            .context("Invalid content-length value.")?
    } else {
        0
    };

    // Allow max body length up to 2 GB
    if content_length > MAX_REQUEST_BODY_SIZE {
        return Err(anyhow!(InternalHttpError::KnownError(ErrorCode::ContentTooLarge)));
    }

    let mut body = Vec::new();
    if method != HttpRequestMethod::HEAD || content_length != 0 {
        body.resize(content_length as usize, 0);
        buf_reader
            .read_exact(&mut body)
            .context("Failed to read body of Http request")?;
    }

    return Ok(HttpRequest {
        request_line: HttpRequestLine::new(method, resource.to_string(), version),
        content: HttpMessageContent::new(headers, body),
    });
}

#[cfg(test)]
mod tests {
    use rand::Rng;
    use std::io::{Cursor, Write};

    use super::*;

    #[test]
    fn request_parse_invalid() {
        let invalid_requests = [
            // invalid request line
            String::from(""),
            String::from("GET HTTP/1.1"),
            String::from("INVALID / HTTP/1.1"),
            // invalid header format
            String::from("GET / HTTP/1.1\r\nContent-Type :: text/plain\r\n\r\n"),
            // invalid content type
            String::from("GET / HTTP/1.1\r\nContent-Length: -32\r\n\r\n"),
            format!(
                "GET / HTTP/1.1\r\nContent-Length: {} \r\n\r\n",
                MAX_REQUEST_BODY_SIZE + 1
            ),
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
    fn request_parse_get() {
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

    #[test]
    fn request_parse_max_allowed_headers() {
        let mut request = String::from("GET / HTTP/1.1\r\n");
        for _ in 0..MAX_HEADERS_AMOUNT {
            let header_name = get_random_string(10);
            let header_value = get_random_string(10);
            let header = format!("{}:{}\r\n", header_name, header_value);
            request.push_str(&header);
        }

        let mut stream = Cursor::new(request.as_bytes().to_vec());
        let result = parse_http_request(&mut stream);
        assert!(result.is_ok());

        // Add one extra header that should lead to error
        request.push_str("break:http\r\n");
        stream.flush().expect("Failed to flush stream");
        stream
            .write_all(request.as_bytes())
            .expect("Failed to write to stream");

        let result = parse_http_request(&mut stream);
        assert!(result.is_err());
    }

    static CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                            abcdefghijklmnopqrstuvwxyz\
                            0123456789)(*&^%$#@!~";
    fn get_random_string(len: u8) -> String {
        let mut rng = rand::thread_rng();
        let string: String = (0..len)
            .map(|_| {
                let idx = rng.gen_range(0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect();
        return string;
    }
}
