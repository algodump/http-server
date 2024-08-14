use std::{
    collections::HashMap,
    io::{Read, Write},
};

use anyhow::{anyhow, Result};
use log::trace;
use mime_guess::from_path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SuccessCode {
    OK = 200,
    Created = 201,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ErrorCode {
    // Client Errors
    BadRequest = 400,
    NotFound = 404,
    ContentTooLarge = 413,
    URITooLong = 414,
    UnsupportedMediaType = 415,
    RequestHeaderFieldsTooLarge = 431,

    // Server Errors
    InternalServerError = 500,
    NotImplemented = 501,
    HTTPVersionNotSupported = 505,

    // TODO: remove later as this is not an actual HTTP response code,
    //      just used for internal purposes
    Undefined = 1000,
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

#[derive(Error, Debug)]
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

pub trait HttpStream: Read + Write {}
impl<T: Read + Write> HttpStream for T {}

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
        self.headers.get(&header_name.into())
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
