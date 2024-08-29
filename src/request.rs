use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    str::FromStr,
    sync::mpsc,
    thread,
};

use crate::{cache::CacheControl, common::*, compressor::ContentEncoding, url::Url};

use anyhow::{anyhow, Context, Result};
use log::{info, trace};

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
    url: Url,
    version: String,
}

impl HttpRequestLine {
    pub fn new(method: HttpRequestMethod, url: Url, version: String) -> Self {
        Self {
            method,
            url,
            version,
        }
    }
}

#[derive(Debug)]
pub struct HttpRequest {
    request_line: HttpRequestLine,
    content: HttpMessageContent,
    requested_encoding: Option<ContentEncoding>,
    ranges: Option<Ranges>,
    cache_control: Option<CacheControl>,
}

impl HttpRequest {
    pub fn get_method(&self) -> HttpRequestMethod {
        return self.request_line.method;
    }

    pub fn get_url(&self) -> Url {
        return self.request_line.url.clone();
    }

    pub fn get_version(&self) -> String {
        return self.request_line.version.clone();
    }

    pub fn get_encoding(&self) -> Option<ContentEncoding> {
        return self.requested_encoding;
    }

    pub fn content(&self) -> &HttpMessageContent {
        &self.content
    }

    pub fn ranges(&self) -> Option<Ranges> {
        self.ranges.clone()
    }

    pub fn cache_control(&self) -> &Option<CacheControl> {
        &self.cache_control
    }
}

pub struct HttpRequestBuilder(HttpRequest);
impl HttpRequestBuilder {
    pub fn new(request_line: HttpRequestLine) -> Self {
        Self(HttpRequest {
            request_line,
            content: HttpMessageContent::new(HashMap::new(), Vec::new()),
            requested_encoding: None,
            ranges: None,
            cache_control: None,
        })
    }

    pub fn set_range(mut self, ranges: Ranges) -> Self {
        let range_content = if ranges.is_multipart() {
            ranges.to_string()
        } else {
            let range = ranges.first().expect("Expected non-empty range");
            format!("{}-{}", range.from, range.to)
        };
        self.0.ranges = Some(ranges);
        self.header("Range", format!("bytes={}", range_content))
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
        .ok_or_else(|| InternalHttpError::KnownError(ErrorCode::HTTPVersionNotSupported))?;

    return Ok(version.to_string());
}

fn parse_header(header: &String) -> Result<(String, String)> {
    if header.len() as u64 > MAX_HEADER_SIZE {
        return Err(anyhow!(InternalHttpError::KnownError(
            ErrorCode::RequestHeaderFieldsTooLarge
        )));
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

// Parse string: "br;q=1.0, gzip;q=0.8, *;q=0.1"
fn parse_encodings(accepted_encodings: &str) -> Result<Vec<ContentEncoding>> {
    let mut encodings_by_priority: Vec<(ContentEncoding, f32)> = Vec::new();
    for encoding in accepted_encodings.split(',') {
        let (name, priority) = if let Some((name, priority)) = encoding.split_once(";q=") {
            (name, priority)
        } else {
            (encoding, "1.0")
        };
        let priority = priority
            .parse::<f32>()
            .context(format!("Failed to parse {:?}", priority))?;
        let content_encoding = ContentEncoding::from_str(name.trim())
            .context(format!("Unknown content encoding {:?}", name))?;

        encodings_by_priority.push((content_encoding, priority));
    }

    encodings_by_priority.sort_by(|lhs, rhs| lhs.1.partial_cmp(&rhs.1).unwrap());
    let res = encodings_by_priority
        .into_iter()
        .map(|(content_encoding, _)| content_encoding)
        .collect();
    return Ok(res);
}

fn choose_content_encoding(content_encodings: &Vec<ContentEncoding>) -> Result<ContentEncoding> {
    let Some(supported_encoding) = content_encodings
        .into_iter()
        .find(|encoding| encoding.is_supported())
    else {
        return Err(anyhow!(InternalHttpError::KnownError(
            ErrorCode::NotAcceptable
        )));
    };
    return Ok(supported_encoding.clone());
}

pub fn parse_http_request_internal(stream: &mut impl HttpStream) -> Result<HttpRequest> {
    let mut buf_reader = BufReader::new(stream);

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
        return Err(anyhow!(InternalHttpError::KnownError(
            ErrorCode::URITooLong
        )));
    }

    let method = HttpRequestMethod::from_str(method)
        .map_err(|_| anyhow!(InternalHttpError::KnownError(ErrorCode::NotImplemented)))?;
    let version = get_http_version(version)?;
    let url = Url::new(resource);

    // Parse headers
    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut line = String::new();
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
        return Err(anyhow!(InternalHttpError::KnownError(
            ErrorCode::ContentTooLarge
        )));
    }

    let mut body = Vec::new();
    if content_length != 0 {
        body.resize(content_length as usize, 0);
        buf_reader
            .read_exact(&mut body)
            .context("Failed to read body of Http request")?;
    }

    let requested_encoding = if let Some(encodings) = headers.get("accept-encoding") {
        let proposed_encodings = parse_encodings(&encodings)?;
        let encoding = choose_content_encoding(&proposed_encodings)?;
        Some(encoding)
    } else {
        info!("accept-encoding, wasn't provided by the client, sending data as is");
        None
    };

    let ranges = headers.get("range").and_then(|ranges| ranges.parse().ok());
    let cache_control = headers
        .get("cache-control")
        .and_then(|cache_control| cache_control.parse().ok());

    return Ok(HttpRequest {
        request_line: HttpRequestLine::new(method, url, version),
        content: HttpMessageContent::new(headers, body),
        requested_encoding,
        ranges,
        cache_control
    });
}

pub fn parse_http_request(stream: &mut impl HttpStream) -> Result<HttpRequest> {
    let (tx, rx) = mpsc::channel();
    let mut stream_for_parser = stream.clone_stream();
    // TODO: this is not a correct implementation as the spawn thread will continue to run even after
    //       the timeout
    thread::spawn(move || {
        _ = tx.send(parse_http_request_internal(&mut stream_for_parser));
    });

    let Ok(parsed_http_request) = rx.recv_timeout(REQUEST_TIMEOUT) else {
        return Err(anyhow!(InternalHttpError::KnownError(
            ErrorCode::RequestTimeout
        )));
    };
    return parsed_http_request;
}

#[cfg(test)]
mod test {
    use rand::Rng;
    use std::io::Cursor;

    use super::*;

    // UTILS
    fn get_error(res: Result<HttpRequest>) -> InternalHttpError {
        let error = res.unwrap_err();
        match error.downcast::<InternalHttpError>() {
            Ok(http_error) => return http_error,
            _ => panic!("Not an InternalHttpError"),
        }
    }

    fn parse_request(request: &str) -> Result<HttpRequest> {
        let mut stream = Cursor::new(request.as_bytes().to_vec());
        return parse_http_request(&mut stream);
    }

    // SUCCESS
    #[test]
    fn request_parse_get() {
        let request =
            "GET /index.html HTTP/1.1\r\nHost: example.com\r\nContent-Length: 5\r\n\r\nHello";

        let result = parse_request(request);
        assert!(result.is_ok());

        let parsed_request = result.unwrap();
        assert_eq!(parsed_request.request_line.method, HttpRequestMethod::GET);
        assert_eq!(parsed_request.request_line.url.resource(), "/index.html");
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
    fn request_parse_accept_encoding() {
        let request = "GET / HTTP/1.1\r\nAccept-Encoding : br;q=0.8, gzip, *\r\n\r\n";
        let result = parse_request(request);
        assert!(result.is_ok());

        let parsed_request = result.unwrap();
        assert!(parsed_request.get_encoding().is_some());
        assert_eq!(
            parsed_request.get_encoding().unwrap(),
            ContentEncoding::Gzip
        );
    }

    // ERRORS
    #[test]
    fn request_malformed_request_line() {
        let invalid_requests = vec![
            // invalid request line
            String::from("\r\n"),
            String::from("GET HTTP/1.1\r\n"),
            String::from("/ HTTP/1.1\r\n"),
            String::from("GET / \r\n"),
        ];

        for request in invalid_requests {
            let result = parse_request(request.as_str());
            assert!(result.is_err());
            assert_eq!(
                get_error(result),
                InternalHttpError::MalformedRequestLine(request)
            );
        }
    }

    #[test]
    fn request_wrong_header_format() {
        let invalid_requests = vec![
            // invalid request line
            String::from("GET / HTTP/1.1\r\nHeader:"),
            String::from("GET / HTTP/1.1\r\n:"),
        ];

        for request in invalid_requests {
            let result = parse_request(request.as_str());
            assert!(result.is_err());
            assert_eq!(get_error(result), InternalHttpError::WrongHeaderFormat);
        }
    }

    #[test]
    fn request_invalid_utf_char() {
        let broken_heart: Vec<u8> = vec![240, 159, 146, 69];
        let invalid_utf_string = unsafe { String::from_utf8_unchecked(broken_heart) };

        let request = format!("GET / HTTP/1.1\r\nLove:{}", invalid_utf_string);
        let res = parse_request(request.as_str());
        assert!(res.is_err());
        assert_eq!(get_error(res), InternalHttpError::InvalidUTF8Char);
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
        request.push_str("break:http\r\n\r\n");

        let mut stream = Cursor::new(request.as_bytes().to_vec());
        let result = parse_http_request(&mut stream);

        assert!(result.is_err());
        assert_eq!(get_error(result), InternalHttpError::HeaderOverflow);
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
