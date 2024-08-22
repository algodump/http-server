mod auth;
mod common;
mod compressor;
mod request;
mod response;
mod url;

use anyhow::{Context, Result};
use common::HttpStream;
use request::parse_http_request;
use response::{build_http_response, build_http_response_for_invalid_request};

pub fn handel_connection(stream: &mut impl HttpStream) -> Result<()> {
    let http_request = parse_http_request(stream);
    let http_response = if http_request.is_err() {
        let http_request_error = http_request.unwrap_err();
        build_http_response_for_invalid_request(http_request_error)
    } else {
        build_http_response(&http_request.unwrap())
    };

    http_response
        .write_to(stream)
        .context("Failed to write to stream")?;
    Ok(())
}
