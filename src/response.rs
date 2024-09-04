use std::{
    cmp,
    collections::HashMap,
    fs::{self, File},
    io::Write,
    os::windows::fs::FileExt,
};

use crate::{
    auth::Authenticator,
    common::*,
    compressor::{Compressor, ContentEncoding},
    request::{HttpRequest, HttpRequestMethod},
};

use anyhow::{Error, Result};
use chrono::Utc;
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
            ResponseCode::Success(code) => match code {
                SuccessCode::Ok => return "OK".to_string(),
                _ => return split_camel_case(format!("{:?}", code)),
            },

            ResponseCode::Error(code) => return split_camel_case(format!("{:?}", code)),
        }
    }
}

#[derive(Debug)]
pub struct HttpResponse {
    status_code: ResponseCode,
    version: String,
    content: HttpMessageContent,
    encoding: Option<ContentEncoding>,
}

pub struct HttpResponseBuilder(HttpResponse);
impl HttpResponseBuilder {
    pub fn new(
        status_code: ResponseCode,
        version: &str,
        encoding: Option<ContentEncoding>,
    ) -> Self {
        let builder = Self(HttpResponse {
            status_code,
            version: String::from(version),
            content: HttpMessageContent::new(HashMap::new(), Vec::new()),
            encoding,
        })
        // General purpose headers
        .header("accept-ranges", "bytes")
        .header(
            "date",
            Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        )
        .header("server", "simple http");

        if let Some(encoding) = encoding {
            builder.header("content-encoding", encoding.to_string())
        } else {
            builder
        }
    }

    pub fn default(status_code: ResponseCode) -> Self {
        HttpResponseBuilder::new(status_code, DEFAULT_HTTP_VERSION, None)
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

    // TODO: don't read body for the head request, just query the length of resource
    pub fn optional_body(self, body: &[u8], include_body: bool) -> Self {
        if include_body {
            self.body(body)
        } else {
            self.header("content-length", body.len().to_string())
        }
    }

    pub fn body(mut self, body: &[u8]) -> Self {
        if let Some(content_encoding) = self.0.encoding {
            self.0
                .content
                .set_body(Compressor::compress(body, content_encoding));
        } else {
            self.0.content.set_body(Vec::from(body));
        };

        let body_length = self.0.content.get_body().len();
        self.header("content-length", body_length.to_string())
    }

    pub fn build(self) -> HttpResponse {
        self.0
    }
}

impl HttpResponse {
    pub fn write_to(&self, stream: &mut impl HttpStream) -> Result<()> {
        stream.write_all(&self.as_bytes())?;
        Ok(())
    }

    pub fn as_bytes(&self) -> Vec<u8> {
        let mut response = Vec::new();
        response.extend_from_slice(
            format!(
                "HTTP/{} {} {}\r\n",
                self.version,
                self.status_code.get_code_value(),
                self.status_code.to_string()
            )
            .as_bytes(),
        );

        for (header_name, header_content) in self.content.get_headers() {
            response
                .extend_from_slice(format!("{}: {}\r\n", header_name, header_content).as_bytes());
        }

        response.extend_from_slice(b"\r\n");
        response.extend_from_slice(&self.content.get_body());
        response
    }

    pub fn content(&self) -> &HttpMessageContent {
        &self.content
    }

    pub fn partial_content_boundary<'life>() -> &'life str {
        "3d6b6a416f9b5"
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

fn read_file_content(file: &File, content_range: Option<Ranges>) -> Result<Vec<u8>> {
    let range = match content_range {
        Some(ranges) if !ranges.is_multipart() => {
            let first = ranges.first().unwrap();
            Range::new(first.from, first.to)
        }
        _ => Range::new(0, file.metadata()?.len()),
    };
    let body_size = (range.to - range.from) as usize;
    let mut file_content = vec![0; body_size];
    let bytes_read = file.seek_read(&mut file_content, range.from)?;
    debug_assert!(bytes_read == body_size);

    Ok(file_content)
}

pub fn build_body_for_multipart_request(
    ranges: &Ranges,
    content_type: &str,
    boundary: &str,
    file_content: &Vec<u8>,
) -> Vec<u8> {
    let mut res: Vec<u8> = Vec::new();

    for range in ranges.elements() {
        res.extend_from_slice(format!("--{}\r\n", boundary.to_string()).as_bytes());
        res.extend_from_slice(format!("content-type: {}\r\n", content_type).as_bytes());
        res.extend_from_slice(
            format!("content-range: bytes {}-{}\r\n\r\n", range.from, range.to).as_bytes(),
        );

        let from = range.from as usize;
        let to = cmp::min((range.to + 1) as usize, file_content.len());

        res.extend_from_slice(&file_content[from..to]);
        res.extend_from_slice(b"\r\n");
    }
    res
}

pub fn build_response_for_multipart_request(
    http_request: &HttpRequest,
    file_content: &Vec<u8>,
    ranges: &Ranges,
    content_type: &str,
) -> HttpResponse {
    let partial_content_builder = HttpResponseBuilder::new(
        ResponseCode::Success(SuccessCode::PartialContent),
        &http_request.get_version(),
        http_request.get_encoding(),
    );
    let is_not_head_request = http_request.get_method() != HttpRequestMethod::HEAD;

    if ranges.is_multipart() {
        let boundary = HttpResponse::partial_content_boundary();
        let multipart_content_type = format!("multipart/byteranges; boundary={}", boundary);
        return partial_content_builder
            .header("content-type", multipart_content_type)
            .optional_body(
                &build_body_for_multipart_request(&ranges, &content_type, &boundary, &file_content),
                is_not_head_request,
            )
            .build();
    } else {
        let range = ranges.first().unwrap();
        return partial_content_builder
            .header("content-type", content_type)
            .header(
                "content-range",
                format!("bytes {}-{}", range.from, range.to),
            )
            .optional_body(&file_content, is_not_head_request)
            .build();
    }
}

pub fn build_http_response(http_request: &HttpRequest) -> HttpResponse {
    let resource = http_request.get_url().resource();
    let version = http_request.get_version();
    let encoding = http_request.get_encoding();
    let method = http_request.get_method();
    let is_not_head_request = method != HttpRequestMethod::HEAD;

    trace!(
        "Method: {:?}, Resource: {}, Headers: {:?}",
        method,
        resource,
        http_request.content().get_headers()
    );

    let ok_response_builder =
        HttpResponseBuilder::new(ResponseCode::Success(SuccessCode::Ok), &version, encoding);
    let not_found_response_builder =
        HttpResponseBuilder::new(ResponseCode::Error(ErrorCode::NotFound), &version, encoding);
    let internal_server_error_response_builder = HttpResponseBuilder::new(
        ResponseCode::Error(ErrorCode::InternalServerError),
        &version,
        encoding,
    );

    match method {
        HttpRequestMethod::GET | HttpRequestMethod::HEAD => match resource.as_str() {
            "/" => ok_response_builder.build(),
            "/user-agent" => {
                if let Some(user_agent) = http_request.content().get_header("user-agent") {
                    ok_response_builder
                        .header("content-type", "text/plain")
                        .optional_body(user_agent.as_bytes(), is_not_head_request)
                        .build()
                } else {
                    not_found_response_builder.build()
                }
            }
            _ => {
                if let Some(file_path) = resource.strip_prefix("/files/") {
                    if let Some((auth_method, auth_data)) = http_request.auth_info() {
                        let authenticated =
                            Authenticator::authenticate(auth_data.as_bytes(), &auth_method);
                        if !authenticated {
                            return HttpResponseBuilder::new(
                                ResponseCode::Error(ErrorCode::Unauthorized),
                                &version,
                                encoding,
                            )
                            .header("WWW-Authenticate", auth_method.to_string())
                            .build();
                        }
                    }

                    let mb_file = fs::File::open(file_path);
                    let Ok(file) = mb_file else {
                        error!(
                            "Can't open `{:?}` error = {:?}",
                            file_path,
                            mb_file.unwrap_err()
                        );
                        return not_found_response_builder.build();
                    };

                    let Ok(content_type) = http_request.content().get_content_type(file_path)
                    else {
                        error!("Unsupported media type: {}", resource);
                        return HttpResponseBuilder::new(
                            ResponseCode::Error(ErrorCode::UnsupportedMediaType),
                            &version,
                            encoding,
                        )
                        .build();
                    };
                    trace!("Content type: {}", content_type);

                    // TODO: don't unwrap error, and don't use this pattern with mb_something then Ok()
                    let mb_file_content = read_file_content(&file, http_request.ranges());
                    let Ok(file_content) = mb_file_content else {
                        return build_http_response_for_invalid_request(
                            mb_file_content.unwrap_err(),
                        );
                    };

                    if let Some(ranges) = http_request.ranges() {
                        return build_response_for_multipart_request(
                            &http_request,
                            &file_content,
                            &ranges,
                            &content_type,
                        );
                    }

                    return ok_response_builder
                        .header("content-type", content_type)
                        .optional_body(&file_content, is_not_head_request)
                        .build();
                } else if let Some(echo) = resource.strip_prefix("/echo/") {
                    let echo_response = ok_response_builder
                        .header("content-type", "text/plain")
                        .optional_body(echo.as_bytes(), is_not_head_request)
                        .build();

                    return echo_response;
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
                    encoding,
                )
                .build();
            }
            return internal_server_error_response_builder.build();
        }
        HttpRequestMethod::OPTIONS => {
            let Ok(content_type) = http_request.content().get_content_type(&resource) else {
                error!("Unsupported media type: {}", resource);
                return HttpResponseBuilder::new(
                    ResponseCode::Error(ErrorCode::UnsupportedMediaType),
                    &version,
                    encoding,
                )
                .build();
            };

            return ok_response_builder
                .header("allow", HttpRequestMethod::supported_methods().join(", "))
                .header("content-type", content_type)
                .header("content-length", "0")
                .build();
        }
        _ => {
            return HttpResponseBuilder::new(
                ResponseCode::Error(ErrorCode::NotImplemented),
                &version,
                encoding,
            )
            .build()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::{
        env::{current_dir, temp_dir},
        fs,
        io::{Cursor, Read},
        path::PathBuf,
    };

    use crate::{
        auth::{AuthMethod, Authenticator},
        common::{Range, Ranges, MAX_HEADER_SIZE, MAX_REQUEST_BODY_SIZE, MAX_URI_LENGTH},
        request::{parse_http_request, HttpRequestBuilder, HttpRequestLine, HttpRequestMethod},
        url::Url,
    };

    // UTILS
    fn generate_error_response_for(invalid_request: &str) -> HttpResponse {
        let mut stream = Cursor::new(invalid_request.as_bytes().to_vec());
        let http_error = parse_http_request(&mut stream).unwrap_err();
        return build_http_response_for_invalid_request(http_error);
    }

    fn get_full_path(file_path: &str) -> PathBuf {
        current_dir()
            .expect("Failed to get current directory")
            .as_path()
            .join(file_path)
    }

    fn read_file(file_path: &PathBuf) -> Vec<u8> {
        let mut file = fs::File::open(&file_path).expect("Can't open test file");
        let mut file_content = Vec::new();
        file.read_to_end(&mut file_content)
            .expect("Failed to read test file");
        file_content
    }

    // BUILDERS
    fn request_get_builder(resource: &str) -> HttpRequestBuilder {
        HttpRequestBuilder::new(HttpRequestLine::new(
            HttpRequestMethod::GET,
            Url::new(resource),
            String::from("HTTP/1.1"),
        ))
    }

    fn request_post_builder(resource: &str) -> HttpRequestBuilder {
        HttpRequestBuilder::new(HttpRequestLine::new(
            HttpRequestMethod::POST,
            Url::new(resource),
            String::from("HTTP/1.1"),
        ))
    }

    fn request_head_builder(resource: &str) -> HttpRequestBuilder {
        HttpRequestBuilder::new(HttpRequestLine::new(
            HttpRequestMethod::HEAD,
            Url::new(resource),
            String::from("HTTP/1.1"),
        ))
    }

    fn request_options_builder(resource: &str) -> HttpRequestBuilder {
        HttpRequestBuilder::new(HttpRequestLine::new(
            HttpRequestMethod::OPTIONS,
            Url::new(resource),
            String::from("HTTP/1.1"),
        ))
    }

    // GET REQUEST TESTS
    #[test]
    fn response_get_empty() {
        let request = request_get_builder("/").build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::Ok));
    }

    #[test]
    fn response_get_user_agent() {
        let user_agent = "my-http-server";
        let request = request_get_builder("/user-agent")
            .header("user-agent", user_agent)
            .build();
        let response = build_http_response(&request);

        let response = response;
        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::Ok));
        assert!(response
            .content
            .get_body()
            .starts_with(user_agent.as_bytes()));
    }

    #[test]
    fn response_get_echo() {
        let request = request_get_builder("/echo/test").build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::Ok));
        assert!(response.content.get_body().starts_with(b"test"));
    }

    #[test]
    fn response_get_file() {
        let file_full_path = get_full_path("src/main.rs");
        let file_content = read_file(&file_full_path);

        let request =
            request_get_builder(format!("/files/{}", file_full_path.display()).as_str()).build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::Ok));
        assert_eq!(
            response.content.get_header("content-type").unwrap(),
            "text/x-rust"
        );
        assert!(response.content.get_body().starts_with(&file_content));
    }

    #[test]
    fn response_get_partial_content_single_range() {
        let file_full_path = get_full_path("src/main.rs");
        let file_content = read_file(&file_full_path);
        let range = Range::new(0, 64);
        let ranges = Ranges::new(vec![range.clone()]);
        let request = request_get_builder(format!("/files/{}", file_full_path.display()).as_str())
            .set_range(ranges.clone())
            .build();
        let response = build_http_response(&request);

        assert_eq!(
            response.status_code,
            ResponseCode::Success(SuccessCode::PartialContent)
        );
        assert_eq!(
            response.content.get_header("content-range").unwrap(),
            format!("bytes {}-{}", range.from, range.to).as_str()
        );
        assert_eq!(
            response.content.get_body().len(),
            (range.to - range.from) as usize
        );
        let partial_file_content =
            &file_content[(range.from as usize)..(range.to as usize)].to_vec();
        assert_eq!(response.content.get_body(), partial_file_content);
    }

    #[test]
    fn response_get_partial_content_multiple_ranges() {
        let file_full_path = get_full_path("src/main.rs");
        let range = Range::new(0, 64);
        let ranges = Ranges::new(vec![range.clone(), range.clone()]);
        let request = request_get_builder(format!("/files/{}", file_full_path.display()).as_str())
            .set_range(ranges.clone())
            .build();
        let response = build_http_response(&request);

        assert_eq!(
            response.status_code,
            ResponseCode::Success(SuccessCode::PartialContent)
        );

        fn count(s: &str, response_body: &String) -> usize {
            response_body.match_indices(s).collect::<Vec<_>>().len()
        }

        let response_body = String::from_utf8(response.content().get_body().clone())
            .expect("Failed to convert body to string");
        let number_of_ranges = ranges.len();

        assert_eq!(
            count(HttpResponse::partial_content_boundary(), &response_body),
            number_of_ranges
        );
        assert_eq!(count("content-type", &response_body), number_of_ranges);
        assert_eq!(count("content-range", &response_body), number_of_ranges);
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
    fn response_unauthorized_request() {
        let request = request_get_builder("/files/test")
            .set_auth_info((AuthMethod::Basic, String::from("djkfdskjf")))
            .build();
        let response = build_http_response(&request);

        assert_eq!(
            response.status_code,
            ResponseCode::Error(ErrorCode::Unauthorized)
        );
    }

    #[test]
    fn response_authorized_request() {
        let request = request_get_builder("/files/test")
            .header(
                "authorization",
                format!("Basic {}", Authenticator::default_credentials()),
            )
            .build();
        let response = build_http_response(&request);

        assert_ne!(
            response.status_code,
            ResponseCode::Error(ErrorCode::Unauthorized)
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
    fn response_with_invalid_request_not_accepted() {
        let not_supported_encoding = ContentEncoding::Pack200gzip.to_string();
        let invalid_request = format!(
            "GET /echo/test HTTP/1.1\r\nAccept-Encoding : {}",
            not_supported_encoding
        );
        let error_response = generate_error_response_for(&invalid_request);

        assert_eq!(
            error_response.status_code,
            ResponseCode::Error(ErrorCode::NotAcceptable)
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

    // HEAD requests
    #[test]
    fn response_head_file() {
        let file_full_path = get_full_path("src/main.rs");

        let request =
            request_head_builder(format!("/files/{}", file_full_path.display()).as_str()).build();
        let response = build_http_response(&request);

        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::Ok));
        assert_eq!(
            response.content.get_header("content-type").unwrap(),
            "text/x-rust"
        );

        assert!(response.content.get_body().is_empty());
    }

    // OPTIONS requests
    #[test]
    fn response_options() {
        let file_full_path = get_full_path("src/main.rs");
        let options_request =
            request_options_builder(format!("/files/{}", file_full_path.display()).as_str())
                .build();
        let response = build_http_response(&options_request);

        assert_eq!(response.status_code, ResponseCode::Success(SuccessCode::Ok));
        assert!(response.content().get_header("allow").is_some());
    }
}
