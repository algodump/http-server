use std::{collections::HashMap, fs, io::Write};

use anyhow::{Error, Result};
use log::{error, trace};

use crate::common::{HttpMessageContent, HttpStream, InternalHttpError, DEFAULT_HTTP_VERSION};
use crate::request::{HttpRequest, HttpRequestMethod};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResponseCode {
    OK = 200,
    Created = 201,

    // Client Errors
    BadRequest = 400,
    NotFound = 404,
    ContentTooLarge = 413,
    UnsupportedMediaType = 415,
    RequestHeaderFieldsTooLarge = 431,

    // Server Errors
    InternalServerError = 500,
    NotImplemented = 501,
    HTTPVersionNotSupported = 505,

    // TODO: remove later as this is not an actual HTTP response code,
    //      just used for internal purposes
    Undefined = 1000,
}

impl ToString for ResponseCode {
    fn to_string(&self) -> String {
        match self {
            ResponseCode::NotFound => return String::from("Not Found"),
            _ => return String::from(format!("{:?}", self)),
        }
    }
}

pub struct HttpResponse {
    status_code: ResponseCode,
    version: String,
    content: HttpMessageContent,
}

pub struct HttpResponseBuilder(HttpResponse);
impl HttpResponseBuilder {
    pub fn new(http_status_code: ResponseCode, version: &str) -> Self {
        Self(HttpResponse {
            status_code: http_status_code,
            version: String::from(version),
            content: HttpMessageContent::new(HashMap::new(), Vec::new()),
        })
    }

    pub fn default() -> Self {
        Self(HttpResponse {
            status_code: ResponseCode::Undefined,
            version: String::from(DEFAULT_HTTP_VERSION),
            content: HttpMessageContent::new(HashMap::new(), Vec::new()),
        })
    }

    pub fn status_code(mut self, status_code: ResponseCode) -> Self {
        self.0.status_code = status_code;
        self
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

    pub fn build(self) -> HttpResponse {
        debug_assert_ne!(self.0.status_code, ResponseCode::Undefined);
        self.0
    }
}

impl HttpResponse {
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

pub fn build_http_response_for_invalid_request(mb_http_error: Error) -> HttpResponse {
    trace!("Http error: {:?}", mb_http_error);
    match mb_http_error.downcast_ref::<InternalHttpError>() {
        Some(error) => match error {
            // Client Errors
            InternalHttpError::BodySizeLimit => HttpResponseBuilder::default()
                .status_code(ResponseCode::ContentTooLarge)
                .build(),
            InternalHttpError::HeaderSizeLimit => HttpResponseBuilder::default()
                .status_code(ResponseCode::RequestHeaderFieldsTooLarge)
                .build(),
            // Server errors
            InternalHttpError::UnsupportedHttpVersion(_) => HttpResponseBuilder::default()
                .status_code(ResponseCode::HTTPVersionNotSupported)
                .build(),
            _ => {
                return HttpResponseBuilder::default()
                    .status_code(ResponseCode::BadRequest)
                    .build()
            }
        },
        None => HttpResponseBuilder::default()
            .status_code(ResponseCode::InternalServerError)
            .build(),
    }
}

pub fn build_http_response(http_request: &HttpRequest) -> HttpResponse {
    let resource = http_request.get_resource();
    let version = http_request.get_version();

    trace!(
        "Method: {:?}, Resource: {}",
        http_request.get_method(),
        resource
    );

    let ok_response_builder = HttpResponseBuilder::new(ResponseCode::OK, &version);
    let not_found_response_builder = HttpResponseBuilder::new(ResponseCode::NotFound, &version);
    let internal_server_error_response_builder =
        HttpResponseBuilder::new(ResponseCode::InternalServerError, &version);

    match http_request.get_method() {
        HttpRequestMethod::GET => match resource.as_str() {
            "/" => {
                return ok_response_builder.build();
            }
            "/user-agent" => {
                let user_agent_response =
                    if let Some(user_agent) = http_request.content().get_header("user-agent") {
                        ok_response_builder
                            .header("content-type", "text/plain")
                            .body(user_agent.as_bytes())
                            .build()
                    } else {
                        not_found_response_builder.build()
                    };

                return user_agent_response;
            }
            _ => {
                if let Some(echo) = resource.strip_prefix("/echo/") {
                    let echo_response = ok_response_builder
                        .header("content-type", "text/plain")
                        .body(echo.as_bytes())
                        .build();

                    return echo_response;
                } else if let Some(file_path) = resource.strip_prefix("/files/") {
                    // FIXME: handel file versions properly, for now it will be just discarded
                    // EXAMPLE: fontawesome-webfont.woff?v=4.7.0
                    let path_end = file_path.find("?v=").unwrap_or(file_path.len());
                    let processed_file_path = &file_path[..path_end];

                    let mb_file_content = fs::read(processed_file_path);
                    let Ok(file_content) = mb_file_content else {
                        error!(
                            "Can't read `{:?}` error = {:?}",
                            processed_file_path,
                            mb_file_content.unwrap_err()
                        );
                        return not_found_response_builder.build();
                    };

                    let Ok(content_type) =
                        http_request.content().get_content_type(processed_file_path)
                    else {
                        error!("Unsupported media type: {}", resource);
                        return HttpResponseBuilder::new(
                            ResponseCode::UnsupportedMediaType,
                            &version,
                        )
                        .build();
                    };
                    trace!("Content type: {}", content_type);

                    return ok_response_builder
                        .header("content-type", content_type)
                        .body(&file_content)
                        .build();
                }
                error!("GET: Unhandled response message: resource - {:?}", resource);
                return internal_server_error_response_builder.build();
            }
        },
        HttpRequestMethod::POST => {
            if let Some(file_path) = resource.strip_prefix("/files/") {
                let mb_file = fs::File::create(file_path);
                let Ok(mut file) = mb_file else {
                    error!(
                        "POST: Failed to create a file: {:?}. {:?}",
                        file_path,
                        mb_file.unwrap_err()
                    );
                    return internal_server_error_response_builder.build();
                };

                let mb_success = file.write_all(&http_request.content().get_body());
                let Ok(_) = mb_success else {
                    error!(
                        "POST: Failed to write to file: {:?}. {:?}",
                        file_path,
                        mb_success.unwrap_err()
                    );
                    return internal_server_error_response_builder.build();
                };

                return HttpResponseBuilder::new(ResponseCode::Created, &version).build();
            }
            return internal_server_error_response_builder.build();
        }
        _ => return HttpResponseBuilder::new(ResponseCode::NotImplemented, &version).build(),
    }
}

#[cfg(test)]
mod tests {
    use super::{build_http_response, ResponseCode};

    use std::{
        env::{current_dir, temp_dir},
        fs,
        io::Read,
    };

    use crate::{
        request::{HttpRequestBuilder, HttpRequestLine, HttpRequestMethod},
    };

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
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::OK);
    }

    #[test]
    fn test_user_agent_response() {
        let user_agent = "my-http-server";
        let request = request_get_builder("/user-agent")
            .header("user-agent", user_agent)
            .build();
        let response = build_http_response(&request);

        let response = response;
        assert_eq!(response.status_code, ResponseCode::OK);
        assert!(response
            .content
            .get_body()
            .starts_with(user_agent.as_bytes()));
    }

    #[test]
    fn test_user_echo_response() {
        let request = request_get_builder("/echo/test").build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::OK);
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
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::OK);
        assert_eq!(
            response.content.get_header("content-type").unwrap(),
            "text/x-rust"
        );
        assert!(response.content.get_body().starts_with(&file_content));
    }

    #[test]
    fn test_not_found_response() {
        let request = request_get_builder("/files/nonexistent_file").build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::NotFound);
    }

    #[test]
    fn test_invalid_get_response() {
        let request = request_get_builder("/nonexistent/test").build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::InternalServerError);
    }

    // POST REQUEST TESTS
    #[test]
    fn test_post_response() {
        let tmp_file_path = temp_dir().join("test.txt");
        let file_data = b"data for testing POST request".to_vec();

        let request = request_post_builder(format!("/files/{}", tmp_file_path.display()).as_str())
            .body(&file_data)
            .build();
        let response = build_http_response(&request);

        let mut file = fs::File::open(&tmp_file_path).expect("POST request failed to create file");
        let mut file_content_create_by_post_request = Vec::new();
        file.read_to_end(&mut file_content_create_by_post_request)
            .expect("Failed to read test file");

        assert_eq!(response.status_code, ResponseCode::Created);
        assert_eq!(file_content_create_by_post_request, file_data);
    }

    #[test]
    fn test_invalid_post_response() {
        let request = request_post_builder("/nonexistent/test").build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::InternalServerError);
    }
}
