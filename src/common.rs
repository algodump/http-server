use std::{io::{Read, Write}, net::TcpStream};
use thiserror::Error;

#[derive(Error, Debug)]
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

pub trait Stream: Read + Write {}
impl Stream for TcpStream {}