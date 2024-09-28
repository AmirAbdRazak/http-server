use core::fmt;
#[allow(unused_imports)]
use std::net::TcpListener;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::TcpStream,
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
    fn add_header(&mut self, header_name: &str, header_value: &str) {
        let header = format!("{header_name}: {header_value}");
        self.headers.push(header);
    }
}

struct Request {
    http_method: HttpMethod,
    request_target: String,
    http_version: HttpVersion,
    headers: HashMap<String, String>,
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            StatusCode::Ok => write!(f, "200 OK"),
            StatusCode::NotFound => write!(f, "404 Not Found"),
        }
    }
}

impl fmt::Display for HttpVersion {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            HttpVersion::Http1_1 => write!(f, "HTTP/1.1"),
        }
    }
}
impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
        }
    }
}
impl fmt::Display for ContentType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ContentType::PlainText => write!(f, "text/plain"),
        }
    }
}
impl fmt::Display for HttpException {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            HttpException::InvalidMethod(raw_method) => {
                write!(f, "Invalid Method: {}", raw_method)
            }
            HttpException::InvalidVersion(raw_version) => {
                write!(f, "Invalid Version: {}", raw_version)
            }
            HttpException::InvalidStatusLine(raw_status_line) => {
                write!(f, "Invalid Status Line: {}", raw_status_line)
            }
        }
    }
}

// HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 3\r\n\r\nabc
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

fn handle_client(request: Request) -> Response {
    let request_path_vec: Vec<_> = request
        .request_target
        .split("/")
        .filter(|path_section| path_section.len() > 0)
        .collect();

    if request_path_vec.len() == 0 {
        Response {
            http_version: HttpVersion::Http1_1,
            status_code: StatusCode::Ok,
            body: String::from(""),
            headers: vec![],
        }
    } else if request_path_vec.len() == 1 && request_path_vec[0] == "user-agent" {
        let mut response = Response {
            http_version: HttpVersion::Http1_1,
            status_code: StatusCode::Ok,
            body: String::from(request.headers.get("User-Agent").unwrap_or(&String::new())),
            headers: vec![],
        };

        response.add_header("Content-Type", &ContentType::PlainText.to_string());
        response.add_header(
            "Content-Length",
            &response.body.as_bytes().len().to_string(),
        );

        response
    } else if request_path_vec.len() == 2 && request_path_vec[0] == "echo" {
        let mut response = Response {
            http_version: HttpVersion::Http1_1,
            status_code: StatusCode::Ok,
            body: String::from(request_path_vec[1]),
            headers: vec![],
        };

        response.add_header("Content-Type", &ContentType::PlainText.to_string());
        response.add_header(
            "Content-Length",
            &response.body.as_bytes().len().to_string(),
        );

        response
    } else {
        Response {
            http_version: HttpVersion::Http1_1,
            status_code: StatusCode::NotFound,
            body: String::from(""),
            headers: vec![],
        }
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

    Ok(Request {
        http_method: HttpMethod::parse_method(raw_method)?,
        request_target: request_target.to_string(),
        http_version: HttpVersion::parse_version(raw_version)?,
        headers,
    })
}

// GET /index.html HTTP/1.1\r\nHost: localhost:4221\r\nUser-Agent: curl/7.64.1\r\nAccept: */*\r\n\r\n
// HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 3\r\n\r\nabc

fn main() {
    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                println!("=== Connection Established! ===");
                let buf_reader = BufReader::new(&mut stream);

                let request = parse_request(buf_reader);
                let response = match request {
                    Ok(request) => format!("{}", handle_client(request)),
                    Err(http_exception) => format!("{}", http_exception),
                };

                let _ = stream.write(response.as_bytes());
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }
    }
}
