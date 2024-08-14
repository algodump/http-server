use std::{collections::HashMap, fs, io::Write};

use crate::common::{
    ErrorCode, HttpMessageContent, HttpStream, InternalHttpError, ResponseCode, SuccessCode,
    DEFAULT_HTTP_VERSION,
};
use crate::request::{HttpRequest, HttpRequestMethod};

use anyhow::{Error, Result};
use log::{error, trace};

impl ToString for ResponseCode {
    fn to_string(&self) -> String {
        fn split_camel_case(s: String) -> String {
            let mut result = String::new();
            for (i, c) in s.chars().enumerate() {
                if c.is_uppercase() && i != 0 {
                    result.push(' ');
                }
                result.push(c);
            }
            result
        }
        match self {
            ResponseCode::Success(code) => return split_camel_case(format!("{:?}", code)),
            ResponseCode::Error(code) => return split_camel_case(format!("{:?}", code)),
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
    pub fn new(status_code: ResponseCode, version: &str) -> Self {
        Self(HttpResponse {
            status_code,
            version: String::from(version),
            content: HttpMessageContent::new(HashMap::new(), Vec::new()),
        })
    }

    pub fn default(status_code: ResponseCode) -> Self {
        Self(HttpResponse {
            status_code,
            version: String::from(DEFAULT_HTTP_VERSION),
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

    pub fn build(self) -> HttpResponse {
        self.0
    }
}

impl HttpResponse {
    pub fn write_to(&self, stream: &mut impl HttpStream) -> Result<()> {
        let mut response = format!(
            "HTTP/{} {} {}\r\n",
            self.version,
            self.status_code.get_code_value(),
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
    if let Some(http_error) = mb_http_error.downcast_ref::<InternalHttpError>() {
        match http_error {
            InternalHttpError::KnownError(http_error_code) => {
                HttpResponseBuilder::default(ResponseCode::Error(*http_error_code)).build()
            }
            _ => {
                return HttpResponseBuilder::default(ResponseCode::Error(ErrorCode::BadRequest))
                    .build()
            }
        }
    } else {
        error!("System error: {:?}", mb_http_error);
        return HttpResponseBuilder::default(ResponseCode::Error(ErrorCode::InternalServerError))
            .build();
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

    let ok_response_builder =
        HttpResponseBuilder::new(ResponseCode::Success(SuccessCode::OK), &version);
    let not_found_response_builder =
        HttpResponseBuilder::new(ResponseCode::Error(ErrorCode::NotFound), &version);
    let internal_server_error_response_builder = HttpResponseBuilder::new(
        ResponseCode::Error(ErrorCode::InternalServerError),
        &version,
    );

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
                            ResponseCode::Error(ErrorCode::UnsupportedMediaType),
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

                return HttpResponseBuilder::new(
                    ResponseCode::Success(SuccessCode::Created),
                    &version,
                )
                .build();
            }
            return internal_server_error_response_builder.build();
        }
        _ => {
            return HttpResponseBuilder::new(
                ResponseCode::Error(ErrorCode::NotImplemented),
                &version,
            )
            .build()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_http_response, build_http_response_for_invalid_request, ErrorCode, HttpResponse,
        ResponseCode, SuccessCode,
    };

    use std::{
        env::{current_dir, temp_dir},
        fs,
        io::{Cursor, Read},
    };

    use crate::common::{MAX_HEADER_SIZE, MAX_REQUEST_BODY_SIZE, MAX_URI_LENGTH};
    use crate::request::{
        parse_http_request, HttpRequestBuilder, HttpRequestLine, HttpRequestMethod,
    };

    // UTILS
    fn generate_error_response_for(invalid_request: &str) -> HttpResponse {
        let mut stream = Cursor::new(invalid_request.as_bytes().to_vec());
        let http_error = parse_http_request(&mut stream).unwrap_err();
        println!("{:?}", http_error);
        return build_http_response_for_invalid_request(http_error);
    }

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
    fn response_get_empty() {
        let request = request_get_builder("/").build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::OK));
    }

    #[test]
    fn response_get_user_agent() {
        let user_agent = "my-http-server";
        let request = request_get_builder("/user-agent")
            .header("user-agent", user_agent)
            .build();
        let response = build_http_response(&request);

        let response = response;
        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::OK));
        assert!(response
            .content
            .get_body()
            .starts_with(user_agent.as_bytes()));
    }

    #[test]
    fn response_get_echo() {
        let request = request_get_builder("/echo/test").build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::OK));
        assert!(response.content.get_body().starts_with(b"test"));
    }

    #[test]
    fn response_get_file() {
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

        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::OK));
        assert_eq!(
            response.content.get_header("content-type").unwrap(),
            "text/x-rust"
        );
        assert!(response.content.get_body().starts_with(&file_content));
    }

    #[test]
    fn response_get_file_not_found() {
        let request = request_get_builder("/files/nonexistent_file").build();
        let response = build_http_response(&request);

        assert_eq!(
            response.status_code,
            ResponseCode::Error(ErrorCode::NotFound)
        );
    }

    #[test]
    fn response_invalid_get_prefix() {
        let request = request_get_builder("/nonexistent/test").build();
        let response = build_http_response(&request);

        assert_eq!(
            response.status_code,
            ResponseCode::Error(ErrorCode::InternalServerError)
        );
    }

    #[test]
    fn response_with_invalid_request_bad_request() {
        let invalid_requests = [
            String::from(""),
            String::from("GET /"),
            String::from("GET / HTTP/1.1\r\nWrongHeader=value"),
        ];

        for invalid_request in invalid_requests {
            let error_response = generate_error_response_for(&invalid_request);
            assert_eq!(
                error_response.status_code,
                ResponseCode::Error(ErrorCode::BadRequest)
            );
        }
    }

    #[test]
    fn response_with_invalid_request_internal_server_error() {
        let invalid_requests = [String::from(
            "GET / HTTP/1.1\r\nContent-Length : -32\r\n\r\n",
        )];

        for invalid_request in invalid_requests {
            let error_response = generate_error_response_for(&invalid_request);
            assert_eq!(
                error_response.status_code,
                ResponseCode::Error(ErrorCode::InternalServerError)
            );
        }
    }

    #[test]
    fn response_with_invalid_request_content_too_large() {
        let invalid_request = format!(
            "GET / HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_REQUEST_BODY_SIZE + 1
        );
        let error_response = generate_error_response_for(&invalid_request);

        assert_eq!(
            error_response.status_code,
            ResponseCode::Error(ErrorCode::ContentTooLarge)
        );
    }

    #[test]
    fn response_with_invalid_request_uri_too_long() {
        let invalid_request = format!(
            "GET {} HTTP/1.1\r\n",
            ["X"; (MAX_URI_LENGTH as usize) + 2].concat()
        );
        let error_response = generate_error_response_for(&invalid_request);

        assert_eq!(
            error_response.status_code,
            ResponseCode::Error(ErrorCode::URITooLong)
        );
    }

    #[test]
    fn response_with_invalid_request_header_too_large() {
        let invalid_request = format!(
            "GET / HTTP/1.1\r\ntest:{}",
            ["X"; MAX_HEADER_SIZE as usize].concat()
        );
        let error_response = generate_error_response_for(&invalid_request);

        assert_eq!(
            error_response.status_code,
            ResponseCode::Error(ErrorCode::RequestHeaderFieldsTooLarge)
        );
    }

    #[test]
    fn response_with_invalid_request_http_version_not_supported() {
        let invalid_request = "GET / HTTP/3.0\r\n";
        let error_response = generate_error_response_for(&invalid_request);

        assert_eq!(
            error_response.status_code,
            ResponseCode::Error(ErrorCode::HTTPVersionNotSupported)
        );
    }

    // POST REQUEST TESTS
    #[test]
    fn response_post() {
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

        assert_eq!(
            response.status_code,
            ResponseCode::Success(SuccessCode::Created)
        );
        assert_eq!(file_content_create_by_post_request, file_data);
    }

    #[test]
    fn response_post_invalid() {
        let request = request_post_builder("/nonexistent/test").build();
        let response = build_http_response(&request);

        assert_eq!(
            response.status_code,
            ResponseCode::Error(ErrorCode::InternalServerError)
        );
    }
}
