use anyhow::{Result, Context};

use common::Stream;
use request::parse_http_request;
use response::parse_http_response;

pub mod common;
pub mod request;
mod response;

pub fn handel_connection(stream: &mut impl Stream) -> Result<()> {
    let http_request = parse_http_request(stream)?;
    let http_response = parse_http_response(&http_request)?;

    http_response
        .write_to(stream)
        .context("Failed to write to stream")?;
    Ok(())
}
