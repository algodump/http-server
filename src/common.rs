use std::{collections::HashMap, io::{Read, Write}};
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum HttpError {
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
    PostFailed(String)
}

pub trait HttpStream: Read + Write {}
impl<T: Read + Write> HttpStream for T {}

pub struct HttpMessageContent {
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl HttpMessageContent {
    pub fn new(headers: HashMap<String, String>, body: Vec<u8>) -> Self {
        Self {
            headers,
            body
        }
    }

    pub fn get_header(&self, header_name: impl Into<String>) -> Option<&String >{
        self.headers.get(&header_name.into())
    }

    pub fn add_header(&mut self, header_name: impl Into<String>, header_content: impl Into<String>) -> Option<String> {
        self.headers.insert(header_name.into(), header_content.into())
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
}