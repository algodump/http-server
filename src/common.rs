use std::{
    collections::HashMap,
    io::{Cursor, Read, Write},
    net::TcpStream,
    str::FromStr,
    time::Duration,
};

use anyhow::{anyhow, Result};
use log::trace;
use mime_guess::from_path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SuccessCode {
    Ok = 200,
    Created = 201,
    PartialContent = 206,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorCode {
    // Client Errors
    BadRequest = 400,
    Unauthorized = 401,
    NotFound = 404,
    NotAcceptable = 406,
    RequestTimeout = 408,
    ContentTooLarge = 413,
    URITooLong = 414,
    UnsupportedMediaType = 415,
    RequestHeaderFieldsTooLarge = 431,

    // Server Errors
    InternalServerError = 500,
    NotImplemented = 501,
    HTTPVersionNotSupported = 505,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResponseCode {
    Success(SuccessCode),
    Error(ErrorCode),
}
impl ResponseCode {
    pub fn get_code_value(&self) -> u16 {
        match self {
            ResponseCode::Success(code) => return *code as u16,
            ResponseCode::Error(code) => return *code as u16,
        }
    }
}

#[derive(Error, Debug, PartialEq)]
pub enum InternalHttpError {
    #[error("{}", 0.to_string())]
    KnownError(ErrorCode),
    #[error("Malformed HTTP request line: `{0}`")]
    MalformedRequestLine(String),
    #[error("Wrong Header Format")]
    WrongHeaderFormat,
    #[error("Exceeded maximum amount of headers {}", MAX_HEADERS_AMOUNT)]
    HeaderOverflow,
    #[error("Encountered invalid UTF8 while parsing HTTP request")]
    InvalidUTF8Char,
}

pub const MAX_HEADERS_AMOUNT: usize = 10_000;
pub const MAX_REQUEST_BODY_SIZE: u64 = u64::MAX / 2; // 2 GB
pub const MAX_HEADER_SIZE: u64 = (u16::MAX / 2) as u64; // 8 KB
pub const DEFAULT_HTTP_VERSION: &str = "1.1";
pub const MAX_URI_LENGTH: usize = u16::MAX as usize;
pub const REQUEST_TIMEOUT: Duration = Duration::new(60, 0);

pub trait HttpStream: Read + Write + Send + 'static {
    fn clone_stream(&self) -> Self;
}

impl HttpStream for TcpStream {
    fn clone_stream(&self) -> Self {
        self.try_clone().expect("Failed to clone stream")
    }
}

impl HttpStream for Cursor<Vec<u8>> {
    fn clone_stream(&self) -> Self {
        self.clone()
    }
}

#[derive(Debug)]
pub struct HttpMessageContent {
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

// TODO: write your own MIME detector
fn determine_content_type(resource: &str) -> Option<mime_guess::Mime> {
    let mime_type = from_path(resource);
    return mime_type.first();
}

impl HttpMessageContent {
    pub fn new(headers: HashMap<String, String>, body: Vec<u8>) -> Self {
        Self { headers, body }
    }

    pub fn get_header(&self, header_name: impl Into<String>) -> Option<&String> {
        self.headers.get(&header_name.into().to_ascii_lowercase())
    }

    pub fn add_header(
        &mut self,
        header_name: impl Into<String>,
        header_content: impl Into<String>,
    ) -> Option<String> {
        self.headers
            .insert(header_name.into(), header_content.into())
    }

    pub fn get_body(&self) -> &Vec<u8> {
        &self.body
    }

    pub fn set_body(&mut self, body: Vec<u8>) {
        self.body = body
    }

    pub fn get_headers(&self) -> &HashMap<String, String> {
        &self.headers
    }

    pub fn get_content_type(&self, path_to_resource: &str) -> Result<String> {
        if let Some(content_type) = self.headers.get("content-type") {
            // TODO: verify this, at it might not be supported by the server
            return Ok(content_type.clone());
        } else {
            trace!("Content type wasn't provided by the client, determine content type based on the resource name");
            let mime_type = determine_content_type(&path_to_resource)
                .ok_or_else(|| anyhow!("Failed to determine MIME type"))?
                .to_string();
            return Ok(mime_type);
        }
    }
}

#[derive(Debug, Clone)]
pub struct Range {
    pub from: u64,
    pub to: u64,
}

impl Range {
    pub fn new(from: u64, to: u64) -> Self {
        Range { from, to }
    }
}

impl FromStr for Range {
    type Err = anyhow::Error;
    // Example:  100-150
    fn from_str(range: &str) -> Result<Self> {
        fn parse_range(range: &str) -> Option<Range> {
            let (from, to) = range.split_once('-')?;
            let from = from.parse().ok()?;
            let to = to.parse().ok()?;

            if from < to {
                Some(Range::new(from, to))
            } else {
                None
            }
        }
        return parse_range(&range)
            .ok_or_else(|| anyhow!(format!("Failed to parse range: {}", range)));
    }
}

impl ToString for Range {
    fn to_string(&self) -> String {
        format!("{}-{}", self.from, self.to)
    }
}

#[derive(Debug, Clone)]
pub struct Ranges {
    ranges: Vec<Range>,
}

impl Ranges {
    pub fn new(ranges: Vec<Range>) -> Self {
        Self { ranges }
    }

    pub fn is_multipart(&self) -> bool {
        self.ranges.len() > 1
    }

    pub fn first(&self) -> Option<&Range> {
        self.ranges.first()
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn elements(&self) -> &Vec<Range> {
        &self.ranges
    }
}

impl FromStr for Ranges {
    type Err = anyhow::Error;
    // Example: bytes=0-50, 100-150"
    fn from_str(ranges: &str) -> Result<Self> {
        fn parse_ranges(data: &str) -> Option<Ranges> {
            let ranges = data.strip_prefix("bytes=")?;
            let res = ranges
                .split(',')
                .map(|range| range.trim().parse().ok())
                .collect::<Option<Vec<Range>>>()?;
            Some(Ranges::new(res))
        }
        return parse_ranges(&ranges)
            .ok_or_else(|| anyhow!(format!("Failed to parse multipart range: {}", ranges)));
    }
}

impl ToString for Ranges {
    fn to_string(&self) -> String {
        self.ranges
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",")
    }
}
