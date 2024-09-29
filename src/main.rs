use core::fmt;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    thread::{self, JoinHandle},
};

enum StatusCode {
    Ok,
    NotFound,
}

enum HttpVersion {
    Http1_1,
}

enum ContentType {
    PlainText,
}

struct Response {
    http_version: HttpVersion,
    status_code: StatusCode,
    headers: Vec<String>,
    body: String,
}

impl Response {
    fn new(http_version: HttpVersion, status_code: StatusCode, body: String) -> Self {
        Self {
            http_version,
            status_code,
            body,
            headers: vec![],
        }
    }

    fn add_header(&mut self, header_name: &str, header_value: &str) {
        let header = format!("{header_name}: {header_value}");
        self.headers.push(header);
    }

    fn send_200(body: &str) -> Self {
        let mut response = Self::new(HttpVersion::Http1_1, StatusCode::Ok, body.to_string());

        response.add_header("Content-Type", &ContentType::PlainText.to_string());
        response.add_header(
            "Content-Length",
            &response.body.as_bytes().len().to_string(),
        );

        response
    }

    fn send_404() -> Self {
        Self::new(HttpVersion::Http1_1, StatusCode::NotFound, String::new())
    }
}

struct Request {
    http_method: HttpMethod,
    request_target: String,
    http_version: HttpVersion,
    headers: HashMap<String, String>,
}

impl Request {
    fn new(
        http_method: HttpMethod,
        request_target: String,
        http_version: HttpVersion,
        headers: HashMap<String, String>,
    ) -> Self {
        Self {
            http_method,
            request_target,
            http_version,
            headers,
        }
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Ok => write!(f, "200 OK"),
            Self::NotFound => write!(f, "404 Not Found"),
        }
    }
}

impl fmt::Display for HttpVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Http1_1 => write!(f, "HTTP/1.1"),
        }
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
        }
    }
}

impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::PlainText => write!(f, "text/plain"),
        }
    }
}

impl fmt::Display for HttpException {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidMethod(raw_method) => {
                write!(f, "Invalid Method: {}", raw_method)
            }
            Self::InvalidVersion(raw_version) => {
                write!(f, "Invalid Version: {}", raw_version)
            }
            Self::InvalidStatusLine(raw_status_line) => {
                write!(f, "Invalid Status Line: {}", raw_status_line)
            }
        }
    }
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let crlf = "\r\n";
        let concatenated_header = self.headers.join(crlf) + crlf;
        write!(
            f,
            "{} {}{}{}{}{}",
            self.http_version, self.status_code, crlf, concatenated_header, crlf, self.body
        )
    }
}
impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let crlf = "\r\n";
        let concatenated_header = self.headers.iter().fold(String::new(), |acc, (key, val)| {
            format!("{acc}{key}: {val}{crlf}")
        });
        write!(
            f,
            "{} {} {}{}{}",
            self.http_method, self.http_version, crlf, concatenated_header, crlf
        )
    }
}

enum HttpMethod {
    Get,
    Post,
}

enum HttpException {
    InvalidMethod(String),
    InvalidVersion(String),
    InvalidStatusLine(String),
}

impl HttpMethod {
    fn parse_method(raw_method: &str) -> Result<HttpMethod, HttpException> {
        match raw_method {
            "GET" => Ok(HttpMethod::Get),
            "POST" => Ok(HttpMethod::Post),
            _ => Err(HttpException::InvalidMethod(raw_method.to_string())),
        }
    }
}
impl HttpVersion {
    fn parse_version(raw_version: &str) -> Result<HttpVersion, HttpException> {
        match raw_version {
            "HTTP/1.1" => Ok(HttpVersion::Http1_1),
            _ => Err(HttpException::InvalidVersion(raw_version.to_string())),
        }
    }
}

fn handle_request(request: Request) -> Response {
    let request_path_vec: Vec<_> = request
        .request_target
        .split("/")
        .filter(|path_section| path_section.len() > 0)
        .collect();

    if request_path_vec.len() == 0 {
        Response::send_200("")
    } else if request_path_vec.len() == 1 && request_path_vec[0] == "user-agent" {
        Response::send_200(request.headers.get("User-Agent").unwrap_or(&String::new()))
    } else if request_path_vec.len() == 2 && request_path_vec[0] == "echo" {
        Response::send_200(request_path_vec[1])
    } else {
        Response::send_404()
    }
}

fn parse_request(buf_reader: BufReader<&mut TcpStream>) -> Result<Request, HttpException> {
    let raw_request: Vec<String> = buf_reader
        .lines()
        .map(|result| result.unwrap())
        .take_while(|line| !line.is_empty())
        .collect();

    let raw_status_line = &raw_request[0];
    let [raw_method, request_target, raw_version] =
        raw_status_line.split_whitespace().collect::<Vec<&str>>()[..3]
    else {
        return Err(HttpException::InvalidStatusLine(
            raw_status_line.to_string(),
        ));
    };

    let headers: HashMap<String, String> = raw_request[1..]
        .iter()
        .filter_map(|header_line| {
            header_line
                .split_once(":")
                .map(|(key, val)| (key.trim().to_string(), val.trim().to_string()))
        })
        .collect();

    Ok(Request::new(
        HttpMethod::parse_method(raw_method)?,
        request_target.to_string(),
        HttpVersion::parse_version(raw_version)?,
        headers,
    ))
}

struct ThreadPool {
    max_connections: usize,
    current_connections: Vec<JoinHandle<()>>,
}

impl ThreadPool {
    fn new(max_connections: usize) -> Self {
        Self {
            max_connections,
            current_connections: Vec::new(),
        }
    }

    fn execute(&mut self, stream: TcpStream) {
        self.current_connections.retain(|jh| !jh.is_finished());

        if self.current_connections.len() < self.max_connections {
            println!(
                "=== Connection Established @ Thread {} ===",
                self.current_connections.len()
            );
            self.current_connections
                .push(thread::spawn(|| handle_connection(stream)));
        } else {
            println!("=== Connection Refused ===");
        }
    }
}

fn handle_connection(mut stream: TcpStream) {
    let buf_reader = BufReader::new(&mut stream);

    let request = parse_request(buf_reader);
    let response = match request {
        Ok(request) => format!("{}", handle_request(request)),
        Err(http_exception) => format!("{}", http_exception),
    };

    let _ = stream.write(response.as_bytes());
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();

    let mut pool = ThreadPool::new(5);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => pool.execute(stream),
            Err(e) => {
                println!("error: {}", e);
            }
        }
    }
}
