mod auth;
mod cache;
mod common;
mod compressor;
mod request;
mod response;
mod url;

use anyhow::{Context, Result};
use cache::Cache;
use common::HttpStream;
use request::parse_http_request;
use response::{build_http_response, build_http_response_for_invalid_request};

pub fn handel_connection(stream: &mut impl HttpStream) -> Result<()> {
    let http_request = parse_http_request(stream);

    match http_request {
        Ok(request) => {
            let resource = request.get_url().resource();
            // TODO: parse cache control string to check if the cache usage is allowed
            if let Ok(raw_response) = Cache::retrieve(&resource) {
                stream
                    .write_all(&raw_response)
                    .context("Failed to write raw response to stream")?;
                return Ok(());
            }
            let response = build_http_response(&request);
            Cache::add(&resource, &response)?;

            response
                .write_to(stream)
                .context("Failed to write to stream")?;
        }
        Err(error) => {
            let response = build_http_response_for_invalid_request(error);
            response
                .write_to(stream)
                .context("Failed to write to stream")?;
        }
    }
    Ok(())
}
