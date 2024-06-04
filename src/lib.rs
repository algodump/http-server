use std::{
    collections::HashMap,
    collections::HashSet,
    error, fs,
    io::{prelude::*, BufReader, Write},
    net::TcpStream
};

use flate2::read::GzEncoder;
use flate2::Compression;

// TODO: use better error handling mechanism, it's annoying to use .into() every time I want to report an error
type Result<T> = std::result::Result<T, Box<dyn error::Error>>;

#[derive(Debug)]
enum HTTPRequestMethod {
    GET(String),
    POST(String),
}

struct HTTPRequest {
    method: HTTPRequestMethod,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
enum HTTPResponseStatusCode {
    OK = 200,
    Created = 201,
    NotFound = 404,
}

impl HTTPResponseStatusCode {
    fn to_string(&self) -> String {
        match self {
            HTTPResponseStatusCode::NotFound => return String::from("Not Found"),
            _ => return String::from(format!("{:?}", self)),
        }
    }
}

#[derive(Debug)]
struct HTTPResponseMessage {
    status_code: HTTPResponseStatusCode,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

struct HTTPResponseBuilder(HTTPResponseMessage);
impl HTTPResponseBuilder {
    pub fn new(http_status_code: HTTPResponseStatusCode) -> Self {
        Self(HTTPResponseMessage {
            status_code: http_status_code,
            headers: vec![],
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
            .push((header_name.into(), header_content.into()));
        self
    }

    pub fn body(mut self, body: &[u8]) -> Self {
        self.0.body = Vec::from(body);
        let body_length = self.0.body.len();
        self.header("content-length", body_length.to_string())
    }

    pub fn build(self) -> HTTPResponseMessage {
        self.0
    }
}

impl HTTPResponseMessage {
    pub fn write_to(&self, stream: &mut TcpStream) -> Result<()> {
        let mut response = format!(
            "HTTP/1.1 {} {}\r\n",
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

// TODO: move to separate module
fn extract_encoding(encodings: &Vec<String>) -> Option<&String> {
    let SUPPORTED_ENCODINGS: HashSet<&str> = HashSet::from(["gzip"]);
    encodings
        .iter()
        .find(|encoding| SUPPORTED_ENCODINGS.contains(encoding.as_str()))
}

fn encode_gzip(text: &str) -> Result<Vec<u8>> {
    let mut gz_encoder = GzEncoder::new(text.as_bytes(), Compression::best());
    let mut res_vec = Vec::new();
    gz_encoder.read_to_end(&mut res_vec)?;
    Ok(res_vec)
}

fn parse_http_request(mut stream: &mut TcpStream) -> Result<HTTPRequest> {
    let mut buf_reader = BufReader::new(&mut stream);
    let mut http_request = Vec::new();
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line)?;
        let trimmed = line.trim_end().to_string();
        if trimmed.is_empty() {
            break;
        }
        http_request.push(trimmed);
    }

    if http_request.is_empty() {
        return Err(format!("Malformed HTTP request {:?}", http_request).into());
    }

    let start_line = &http_request[0].split_ascii_whitespace().collect::<Vec<_>>();

    let method_type = match start_line[0] {
        "GET" => {
            if let Some(recourse) = start_line.get(1) {
                HTTPRequestMethod::GET(recourse.to_string())
            } else {
                return Err("GET Method was called without arguments".into());
            }
        }
        "POST" => {
            if let Some(recourse) = start_line.get(1) {
                HTTPRequestMethod::POST(recourse.to_string())
            } else {
                return Err("POST Method was called without arguments".into());
            }
        }
        _ => return Err("Invalid request type".into()),
    };

    let headers: HashMap<String, String> = http_request
        .iter()
        .skip(1)
        .map(|header| {
            let (header_name, header_content) = header.split_once(':').unwrap_or_default();
            (
                header_name.trim().to_string().to_ascii_lowercase(),
                header_content.trim().to_string(),
            )
        })
        .collect();

    let content_length: usize = if let Some(content_length) = headers.get("content-length") {
        content_length.parse::<usize>()?
    } else {
        0
    };

    // TODO: this might lead to OS stack overflow
    let mut body = vec![0; content_length];
    buf_reader.read_exact(&mut body)?;

    return Ok(HTTPRequest {
        method: method_type,
        headers,
        body,
    });
}

fn send_response(stream: &mut TcpStream, response: HTTPResponseMessage) -> Result<()> {
    response.write_to(stream)?;
    Ok(())
}

pub fn handel_connection(mut stream: TcpStream) -> Result<()> {
    let http_request = parse_http_request(&mut stream)?;

    match http_request.method {
        HTTPRequestMethod::GET(recourse) => {
            if recourse == "/" {
                let response_ok = HTTPResponseBuilder::new(HTTPResponseStatusCode::OK).build();
                send_response(&mut stream, response_ok)?;
            } else if let Some(echo) = recourse.strip_prefix("/echo/") {
                let encodings: Vec<String> = http_request
                    .headers
                    .get("accept-encoding")
                    .cloned()
                    .unwrap_or_default()
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .collect();
                let encoding = extract_encoding(&encodings).cloned();

                if encoding.is_some() {
                    let encoded_echo = encode_gzip(echo)?;
                    let response_echo_ok = HTTPResponseBuilder::new(HTTPResponseStatusCode::OK)
                        .header("content-type", "text/plain")
                        .header("content-encoding", encoding.unwrap())
                        .body(&encoded_echo)
                        .build();
                    send_response(&mut stream, response_echo_ok)?;
                } else {
                    let response_echo_ok = HTTPResponseBuilder::new(HTTPResponseStatusCode::OK)
                        .header("content-type", "text/plain")
                        .body(echo.as_bytes())
                        .build();
                    send_response(&mut stream, response_echo_ok)?;
                }
            } else if recourse.starts_with("/user-agent") {
                let user_agent = if let Some(user_agent) = http_request.headers.get("user-agent") {
                    user_agent.clone()
                } else {
                    return Err("Malformed HTTP request, couldn't find user agent".into());
                };
                let user_agent_response = HTTPResponseBuilder::new(HTTPResponseStatusCode::OK)
                    .header("content-type", "text/plain")
                    .body(user_agent.as_bytes())
                    .build();
                send_response(&mut stream, user_agent_response)?;
            } else if let Some(file_path) = recourse.strip_prefix("/files/") {
                let get_file_response = if let Ok(file_content) = fs::read_to_string(file_path) {
                    HTTPResponseBuilder::new(HTTPResponseStatusCode::OK)
                        .header("content-type", "application/octet-stream")
                        .body(file_content.as_bytes())
                        .build()
                } else {
                    HTTPResponseBuilder::new(HTTPResponseStatusCode::NotFound).build()
                };
                send_response(&mut stream, get_file_response)?;
            } else {
                let response_not_found =
                    HTTPResponseBuilder::new(HTTPResponseStatusCode::NotFound).build();
                send_response(&mut stream, response_not_found)?;
            }
        }
        HTTPRequestMethod::POST(recourse) => {
            if let Some(file_path) = recourse.strip_prefix("/files/") {
                let mut file = fs::File::create(file_path)?;
                file.write_all(&http_request.body)?;
                let created_response =
                    HTTPResponseBuilder::new(HTTPResponseStatusCode::Created).build();
                send_response(&mut stream, created_response)?;
            }
        }
    }
    Ok(())
}