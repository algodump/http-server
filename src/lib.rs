use anyhow::{bail, Context, Result};
use std::{fs, io::Write};

use common::Stream;
use request::{parse_http_request, HttpRequestMethod};
use response::{HTTPResponseBuilder, HTTPResponseMessage, HttpResponseStatusCode};

pub mod common;
pub mod request;
mod response;

fn send_response(stream: &mut impl Stream, response: HTTPResponseMessage) -> Result<()> {
    response
        .write_to(stream)
        .context("Failed to write to stream")?;
    Ok(())
}

pub fn handel_connection(stream: &mut impl Stream) -> Result<()> {
    let http_request = parse_http_request(stream)?;
    let resource = http_request.get_resource();
    let version = http_request.get_version();

    let http_ok_response_builder = HTTPResponseBuilder::new(HttpResponseStatusCode::OK, &version);
    let http_not_found_response_builder =
        HTTPResponseBuilder::new(HttpResponseStatusCode::NotFound, &version);

    match http_request.get_method() {
        HttpRequestMethod::GET => {
            if resource == "/" {
                send_response(stream, http_ok_response_builder.build())?;
            } else if let Some(echo) = resource.strip_prefix("/echo/") {
                let response_echo_ok = http_ok_response_builder
                    .header("content-type", "text/plain")
                    .body(echo.as_bytes())
                    .build();
                send_response(stream, response_echo_ok)?;
            } else if resource.starts_with("/user-agent") {
                let Some(user_agent) = http_request.headers.get("user-agent") else {
                    bail!("Can't find user-agent");
                };

                let user_agent_response = http_ok_response_builder
                    .header("content-type", "text/plain")
                    .body(user_agent.as_bytes())
                    .build();
                send_response(stream, user_agent_response)?;
            } else if let Some(file_path) = resource.strip_prefix("/files/") {
                let get_file_response = if let Ok(file_content) = fs::read_to_string(file_path) {
                    http_ok_response_builder
                        .header("content-type", "application/octet-stream")
                        .body(file_content.as_bytes())
                        .build()
                } else {
                    // TODO: log fs_read_to_string error
                    http_not_found_response_builder.build()
                };
                send_response(stream, get_file_response)?;
            }
        }
        HttpRequestMethod::POST => {
            if let Some(file_path) = resource.strip_prefix("/files/") {
                let mut file = fs::File::create(file_path).context("Failed to create file")?;
                file.write_all(&http_request.body)
                    .context("POST request failed")?;
                let created_response =
                    HTTPResponseBuilder::new(HttpResponseStatusCode::Created, &version).build();
                send_response(stream, created_response)?;
            }
        }
        _ => todo!("Not implemented"),
    }
    Ok(())
}
