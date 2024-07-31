use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    str::FromStr,
};

use crate::common::{HttpParsingError, Stream};
use anyhow::{anyhow, Context, Result};

#[derive(Debug, enum_utils::FromStr, Clone, Copy)]
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

pub struct HTTPRequest {
    request_line: HttpRequestLine,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HTTPRequest {
    pub fn get_method(&self) -> HttpRequestMethod {
        return self.request_line.method;
    }

    pub fn get_resource(&self) -> String {
        return self.request_line.resource.clone();
    }

    pub fn get_version(&self) -> String {
        return self.request_line.version.clone();
    }
}

fn get_http_version(version_line: &str) -> Result<String> {
    let supported_versions = ["1.1"];
    if let Some(http_version) = version_line.strip_prefix("HTTP/") {
        for version in supported_versions {
            if http_version.contains(version) {
                return Ok(version.to_string());
            }
        }
    }
    return Err(anyhow!(HttpParsingError::UnsupportedHttpVersion(
        version_line.to_string()
    )));
}

fn parse_header(header: &String) -> Result<(String, String)> {
    let Some(header_parsed) = header.split_once(':') else {
        return Err(anyhow!(HttpParsingError::WrongHeaderFormat));
    };

    return Ok((
        header_parsed.0.trim().to_ascii_lowercase(),
        header_parsed.1.trim().to_string(),
    ));
}

pub fn parse_http_request(mut stream: &mut impl Stream) -> Result<HTTPRequest> {
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
        return Err(anyhow!(HttpParsingError::EmptyHttpRequest));
    }

    let mut start_line = http_request[0].split_ascii_whitespace();
    let (Some(method), Some(resource), Some(version)) =
        (start_line.next(), start_line.next(), start_line.next())
    else {
        return Err(anyhow!(HttpParsingError::MalformedRequestLine(
            http_request[0].to_string()
        )));
    };

    // TODO: verify resource
    let method = HttpRequestMethod::from_str(method)
        .map_err(|_| anyhow!(HttpParsingError::InvalidMethodType(method.to_string())))?;
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

    return Ok(HTTPRequest {
        request_line: HttpRequestLine {
            method,
            resource: resource.to_string(),
            version,
        },
        headers,
        body,
    });
}
