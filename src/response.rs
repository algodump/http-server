use std::{collections::HashMap, fs, io::Write};

use anyhow::{anyhow, bail, Context, Result};
use log::{error, trace};

use crate::common::{HttpError, HttpMessageContent, HttpStream};
use crate::request::{HttpRequest, HttpRequestMethod};

#[derive(Debug, Clone, Copy, PartialEq)]
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

pub struct HttpResponseMessage {
    status_code: HttpResponseStatusCode,
    version: String,
    content: HttpMessageContent,
}

pub struct HttpResponseBuilder(HttpResponseMessage);
impl HttpResponseBuilder {
    pub fn new(http_status_code: HttpResponseStatusCode, version: &str) -> Self {
        Self(HttpResponseMessage {
            status_code: http_status_code,
            version: String::from(version),
            content: HttpMessageContent::new(HashMap::new(), Vec::new()),
        })
    }

    pub fn header(
        mut self,
        header_name: impl Into<String>,
        header_content: impl Into<String>,
    ) -> Self {
        self.0
            .content
            .add_header(header_name.into(), header_content.into());
        self
    }

    pub fn body(mut self, body: &[u8]) -> Self {
        self.0.content.set_body(Vec::from(body));
        let body_length = self.0.content.get_body().len();
        self.header("content-length", body_length.to_string())
    }

    pub fn build(self) -> HttpResponseMessage {
        self.0
    }
}

impl HttpResponseMessage {
    pub fn write_to(&self, stream: &mut impl HttpStream) -> Result<()> {
        let mut response = format!(
            "HTTP/{} {} {}\r\n",
            self.version,
            self.status_code as u16,
            self.status_code.to_string()
        );

        for (header_name, header_content) in self.content.get_headers() {
            response.push_str(&format!("{} : {}\r\n", header_name, header_content));
        }
        response.push_str("\r\n");

        stream.write_all(response.as_bytes())?;
        stream.write_all(&self.content.get_body())?;
        Ok(())
    }

    pub fn content(&self) -> &HttpMessageContent {
        &self.content
    }
}

pub fn parse_http_response(http_request: &HttpRequest) -> Result<HttpResponseMessage> {
    let resource = http_request.get_resource();
    let version = http_request.get_version();

    trace!("Method: {:?}, Resource: {}", http_request.get_method(), resource);

    let http_ok_response_builder = HttpResponseBuilder::new(HttpResponseStatusCode::OK, &version);
    let http_not_found_response_builder =
        HttpResponseBuilder::new(HttpResponseStatusCode::NotFound, &version);

    match http_request.get_method() {
        HttpRequestMethod::GET => match resource.as_str() {
            "/" => {
                return Ok(http_ok_response_builder.build());
            }
            "/user-agent" => {
                let Some(user_agent) = http_request.content().get_header("user-agent") else {
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
                    // FIXME: handel file versions properly, for now it will be just discarded 
                    // EXAMPLE: fontawesome-webfont.woff?v=4.7.0
                    let path_end = file_path.find("?v=").unwrap_or(file_path.len());
                    let processed_file_path = &file_path[..path_end];
                    let file_content_res = fs::read(processed_file_path);

                    let get_file_response = if let Ok(file_content) = file_content_res
                    {
                        let content_type = http_request.content().get_content_type(processed_file_path)?;
                        trace!("Content type: {}", content_type);
                        
                        http_ok_response_builder
                            .header("content-type", content_type)
                            .body(&file_content)
                            .build()
                    } else {
                        error!("Can't read `{:?}` error = {:?}", processed_file_path, file_content_res.unwrap_err());
                        http_not_found_response_builder.build()
                    };

                    return Ok(get_file_response);
                }
                return Err(anyhow!(HttpError::GetFailed(resource)));
            }
        },
        HttpRequestMethod::POST => {
            if let Some(file_path) = resource.strip_prefix("/files/") {
                let mut file = fs::File::create(file_path).context("Failed to create file")?;
                file.write_all(&http_request.content().get_body())
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

#[cfg(test)]
mod tests {
    use super::{parse_http_response, HttpResponseStatusCode};

    use std::{
        env::{current_dir, temp_dir},
        fs,
        io::Read,
    };

    use crate::request::{HttpRequestBuilder, HttpRequestLine, HttpRequestMethod};

    // BUILDERS
    fn request_get_builder(resource: &str) -> HttpRequestBuilder {
        HttpRequestBuilder::new(HttpRequestLine::new(
            HttpRequestMethod::GET,
            String::from(resource),
            String::from("HTTP/1.1"),
        ))
    }

    fn request_post_builder(resource: &str) -> HttpRequestBuilder {
        HttpRequestBuilder::new(HttpRequestLine::new(
            HttpRequestMethod::POST,
            String::from(resource),
            String::from("HTTP/1.1"),
        ))
    }

    // GET REQUEST TESTS
    #[test]
    fn test_empty_get_response() {
        let request = request_get_builder("/").build();
        let response_result = parse_http_response(&request);

        assert!(response_result.is_ok());

        let response = response_result.unwrap();
        assert_eq!(response.status_code, HttpResponseStatusCode::OK);
    }

    #[test]
    fn test_user_agent_response() {
        let user_agent = "my-http-server";
        let request = request_get_builder("/user-agent")
            .header("user-agent", user_agent)
            .build();
        let response_result = parse_http_response(&request);
        assert!(response_result.is_ok());

        let response = response_result.unwrap();
        assert_eq!(response.status_code, HttpResponseStatusCode::OK);
        assert!(response
            .content
            .get_body()
            .starts_with(user_agent.as_bytes()));
    }

    #[test]
    fn test_user_echo_response() {
        let request = request_get_builder("/echo/test").build();
        let response_result = parse_http_response(&request);
        assert!(response_result.is_ok());

        let response = response_result.unwrap();
        assert_eq!(response.status_code, HttpResponseStatusCode::OK);
        assert!(response.content.get_body().starts_with(b"test"));
    }

    #[test]
    fn test_get_file_response() {
        let file_full_path = current_dir()
            .expect("Failed to get current directory")
            .as_path()
            .join("src")
            .join("main.rs");
        let mut file = fs::File::open(&file_full_path).expect("Can't open test file");
        let mut file_content = Vec::new();
        file.read_to_end(&mut file_content)
            .expect("Failed to read test file");

        let request =
            request_get_builder(format!("/files/{}", file_full_path.display()).as_str()).build();
        let response_result = parse_http_response(&request);
        assert!(response_result.is_ok());

        let response = response_result.unwrap();
        assert_eq!(response.status_code, HttpResponseStatusCode::OK);
        assert_eq!(response.content.get_header("content-type").unwrap(), "text/x-rust");
        assert!(response.content.get_body().starts_with(&file_content));
    }

    #[test]
    fn test_not_found_response() {
        let request = request_get_builder("/files/nonexistent_file").build();
        let response_result = parse_http_response(&request);
        assert!(response_result.is_ok());

        let response = response_result.unwrap();
        assert_eq!(response.status_code, HttpResponseStatusCode::NotFound);
    }

    #[test]
    fn test_invalid_get_response() {
        let request = request_get_builder("/nonexistent/test").build();
        let response_result = parse_http_response(&request);
        assert!(response_result.is_err());
    }

    // POST REQUEST TESTS
    #[test]
    fn test_post_response() {
        let tmp_file_path = temp_dir().join("test.txt");
        let file_data = b"data for testing POST request".to_vec();

        let request = request_post_builder(format!("/files/{}", tmp_file_path.display()).as_str())
            .body(&file_data)
            .build();
        let response_result = parse_http_response(&request);
        assert!(response_result.is_ok());

        let response = response_result.unwrap();
        let mut file = fs::File::open(&tmp_file_path).expect("POST request failed to create file");
        let mut file_content_create_by_post_request = Vec::new();
        file.read_to_end(&mut file_content_create_by_post_request)
            .expect("Failed to read test file");

        assert_eq!(response.status_code, HttpResponseStatusCode::Created);
        assert_eq!(file_content_create_by_post_request, file_data);
    }

    #[test]
    fn test_invalid_post_response() {
        let request = request_post_builder("/nonexistent/test").build();
        let response_result = parse_http_response(&request);
        assert!(response_result.is_err());
    }
}
