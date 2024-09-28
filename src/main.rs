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

struct Response {
    http_version: HttpVersion,
    status_code: StatusCode,
    //body
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

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let crlf = "\r\n\r\n";
        write!(f, "{} {}{}", self.http_version, self.status_code, crlf)
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

struct Request {
    http_method: HttpMethod,
    request_target: String,
    http_version: HttpVersion,
    headers: HashMap<String, String>,
}

fn handle_client(request: Request) -> Response {
    match request.request_target.as_str() {
        "/" => Response {
            http_version: HttpVersion::Http1_1,
            status_code: StatusCode::Ok,
        },
        _ => Response {
            http_version: HttpVersion::Http1_1,
            status_code: StatusCode::NotFound,
        },
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
                .map(|header_vec| (header_vec.0.to_string(), header_vec.1.to_string()))
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
fn main() {
    // You can use print statements as follows for debugging, they'll be visible when running tests.
    println!("Logs from your program will appear here!");

    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                println!("accepted the connection");
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
