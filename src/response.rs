use std::{collections::HashMap, fs, io::Write};

use anyhow::{Result, bail, anyhow, Context};

use crate::common::{Stream, HttpError};
use crate::request::{HttpRequest, HttpRequestMethod};

#[derive(Debug, Clone, Copy)]
pub enum HttpResponseStatusCode {
    OK = 200,
    Created = 201,
    NotFound = 404,
}

impl ToString for HttpResponseStatusCode {
    fn to_string(&self) -> String {
        match self {
            HttpResponseStatusCode::NotFound => return String::from("Not Found"),
            _ => return String::from(format!("{:?}", self)),
        }
    }
}

#[derive(Debug)]
pub struct HttpResponseMessage {
    status_code: HttpResponseStatusCode,
    version: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

pub struct HttpResponseBuilder(HttpResponseMessage);
impl HttpResponseBuilder {
    pub fn new(http_status_code: HttpResponseStatusCode, version: &str) -> Self {
        Self(HttpResponseMessage {
            status_code: http_status_code,
            version: String::from(version),
            headers: HashMap::new(),
            body: vec![],
        })
    }

    pub fn header(
        mut self,
        header_name: impl Into<String>,
        header_content: impl Into<String>,
    ) -> Self {
        self.0
            .headers
            .insert(header_name.into(), header_content.into());
        self
    }

    pub fn body(mut self, body: &[u8]) -> Self {
        self.0.body = Vec::from(body);
        let body_length = self.0.body.len();
        self.header("content-length", body_length.to_string())
    }

    pub fn build(self) -> HttpResponseMessage {
        self.0
    }
}

impl HttpResponseMessage {
    pub fn write_to(&self, stream: &mut impl Stream) -> Result<()> {
        let mut response = format!(
            "HTTP/{} {} {}\r\n",
            self.version,
            self.status_code as u16,
            self.status_code.to_string()
        );

        for (header_name, header_content) in &self.headers {
            response.push_str(&format!("{} : {}\r\n", header_name, header_content));
        }
        response.push_str("\r\n");

        stream.write_all(response.as_bytes())?;
        stream.write_all(&self.body)?;
        Ok(())
    }
}

pub fn parse_http_response(http_request: &HttpRequest) -> Result<HttpResponseMessage> {
    let resource = http_request.get_resource();
    let version = http_request.get_version();

    let http_ok_response_builder = HttpResponseBuilder::new(HttpResponseStatusCode::OK, &version);
    let http_not_found_response_builder =
        HttpResponseBuilder::new(HttpResponseStatusCode::NotFound, &version);

    match http_request.get_method() {
        HttpRequestMethod::GET => {
            match resource.as_str() {
                "/" => {
                    return Ok(http_ok_response_builder.build());
                }
                "/user-agent" => {
                    let Some(user_agent) = http_request.headers.get("user-agent") else {
                        bail!("Can't find user-agent");
                    };
                    let user_agent_response = http_ok_response_builder
                        .header("content-type", "text/plain")
                        .body(user_agent.as_bytes())
                        .build();

                    return Ok(user_agent_response);
                }
                _ => {
                    if let Some(echo) = resource.strip_prefix("/echo/") {
                        let response_echo_ok = http_ok_response_builder
                            .header("content-type", "text/plain")
                            .body(echo.as_bytes())
                            .build();

                        return Ok(response_echo_ok);
                    } else if let Some(file_path) = resource.strip_prefix("/files/") {
                        let get_file_response =
                            if let Ok(file_content) = fs::read_to_string(file_path) {
                                http_ok_response_builder
                                    .header("content-type", "application/octet-stream")
                                    .body(file_content.as_bytes())
                                    .build()
                            } else {
                                // TODO: log fs_read_to_string error
                                http_not_found_response_builder.build()
                            };

                        return Ok(get_file_response);
                    }
                    return Err(anyhow!(HttpError::GetFailed(resource)));
                }
            }
        }
        HttpRequestMethod::POST => {
            if let Some(file_path) = resource.strip_prefix("/files/") {
                let mut file = fs::File::create(file_path).context("Failed to create file")?;
                file.write_all(&http_request.body)
                    .context("POST request failed")?;
                let created_response =
                    HttpResponseBuilder::new(HttpResponseStatusCode::Created, &version).build();
                return Ok(created_response);
            }
            return Err(anyhow!(HttpError::PostFailed(resource)));
        }
        _ => todo!("Not implemented"),
    }
}
