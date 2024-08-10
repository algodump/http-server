use std::{
    collections::HashMap,
    io::{Read, Write},
};

use anyhow::{anyhow, Result};
use bytesize::ByteSize;
use mime_guess::from_path;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum InternalHttpError {
    #[error("Unsupported HTTP version {0}")]
    UnsupportedHttpVersion(String),
    #[error("Unsupported HTTP method {0}")]
    UnsupportedHttpMethod(String),
    #[error("Empty HTTP request")]
    EmptyHttpRequest,
    #[error("Malformed HTTP request line: `{0}`")]
    MalformedRequestLine(String),
    #[error("Invalid HTTP method type: {0}")]
    InvalidMethodType(String),
    #[error("Wrong Header Format")]
    WrongHeaderFormat,
    #[error("Can't find requested resources: {0}")]
    GetFailed(String),
    #[error("Can't post requested resources: {0}")]
    PostFailed(String),
    #[error("Exceeded maximum amount of headers {}", MAX_HEADERS_AMOUNT)]
    HeaderOverflow,
    #[error("Encountered invalid UTF8 while parsing HTTP request")]
    InvalidUTF8Char,
    #[error(
        "Request body size exceeded maximum allowed size of {}",
        MAX_REQUEST_BODY_SIZE
    )]
    BodySizeLimit,
    #[error(
        "Request header size exceeded maximum allowed size {}",
        MAX_HEADER_SIZE
    )]
    HeaderSizeLimit,
}

pub const MAX_HEADERS_AMOUNT: usize = 10_000;
pub const MAX_REQUEST_BODY_SIZE: u64 = ByteSize::gb(2).as_u64();
pub const MAX_HEADER_SIZE: u64 = ByteSize::kb(8).as_u64();

pub trait HttpStream: Read + Write {}
impl<T: Read + Write> HttpStream for T {}

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
            return Ok(content_type.clone());
        } else {
            let mime_type = determine_content_type(&path_to_resource)
                .ok_or_else(|| anyhow!("Failed to determine MIME type"))?
                .to_string();
            return Ok(mime_type);
        }
    }
}
