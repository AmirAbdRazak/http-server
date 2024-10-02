use core::fmt;
use std::{
    collections::{hash_map::Entry, HashMap},
    env::args,
    fs::{create_dir_all, read_to_string, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    thread::{self, JoinHandle},
};

use flate2::{write::GzEncoder, Compression};

enum StatusCode {
    Ok,
    Created,
    NotFound,
    ServerError,
}

enum HttpVersion {
    Http1_1,
}

enum ContentType {
    TextPlain,
    ApplicationOctetStream,
}

#[derive(PartialEq)]
enum ContentEncoding {
    Gzip,
}

struct Response {
    http_version: HttpVersion,
    status_code: StatusCode,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

impl Response {
    fn new(http_version: HttpVersion, status_code: StatusCode, body: Vec<u8>) -> Self {
        Self {
            http_version,
            status_code,
            body,
            headers: HashMap::new(),
        }
    }

    fn update(&mut self, http_version: HttpVersion, status_code: StatusCode, body: Vec<u8>) {
        self.http_version = http_version;
        self.status_code = status_code;
        self.body = body;
    }

    fn new_404() -> Self {
        Self::new(HttpVersion::Http1_1, StatusCode::NotFound, vec![])
    }

    fn add_header(&mut self, header_name: &str, header_value: &str) {
        self.headers
            .entry(header_name.to_string())
            .and_modify(|e| *e = header_value.to_string())
            .or_insert(header_value.to_string());
    }

    fn integrate_request(&mut self, request: &Request) {
        if let Some(content_encoding) = request.headers.get("Accept-Encoding") {
            self.compress_body(ContentEncoding::parse_content_encoding(content_encoding).unwrap());
            self.add_header("Content-Encoding", content_encoding);
        }
    }

    fn compress_body(&mut self, content_encoding: Vec<ContentEncoding>) {
        if content_encoding.contains(&ContentEncoding::Gzip) {
            let mut encoder = GzEncoder::new(vec![], Compression::default());
            let _ = encoder.write_all(&self.body);
            self.body = encoder.finish().unwrap();
            self.add_header("Content-Length", &self.body.len().to_string());
        }
    }

    fn success(&mut self, body: Vec<u8>) {
        self.body = body;
        self.status_code = StatusCode::Ok;

        self.add_header("Content-Type", &ContentType::TextPlain.to_string());
        self.add_header("Content-Length", &self.body.len().to_string());
    }

    fn write_to_stream(&self, stream: &mut TcpStream) {
        let crlf = "\r\n";

        write!(stream, "{} {}{}", self.http_version, self.status_code, crlf).unwrap();
        write!(stream, "{}", stringify_headers(&self.headers)).unwrap();
        write!(stream, "{}", crlf).unwrap();
        let _ = stream.write_all(&self.body);
    }
}

struct Request {
    http_method: HttpMethod,
    request_target: String,
    http_version: HttpVersion,
    headers: HashMap<String, String>,
    body: String,
}

impl Request {
    fn new(
        http_method: HttpMethod,
        request_target: String,
        http_version: HttpVersion,
        headers: HashMap<String, String>,
        body: String,
    ) -> Self {
        Self {
            http_method,
            request_target,
            http_version,
            headers,
            body,
        }
    }

    fn validate_headers(&mut self) {
        if let Entry::Occupied(mut entry) = self.headers.entry("Accept-Encoding".to_string()) {
            if let Some(valid_encoding) = ContentEncoding::parse_content_encoding(entry.get()) {
                entry.insert(
                    valid_encoding
                        .iter()
                        .map(|encoding| encoding.to_string())
                        .collect::<Vec<String>>()
                        .join(", "),
                );
            } else {
                entry.remove();
            };
        }
    }
}

impl fmt::Display for StatusCode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Ok => write!(f, "200 OK"),
            Self::Created => write!(f, "201 Created"),
            Self::NotFound => write!(f, "404 Not Found"),
            Self::ServerError => write!(f, "500 Server Error"),
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
            Self::TextPlain => write!(f, "text/plain"),
            Self::ApplicationOctetStream => write!(f, "application/octet-stream"),
        }
    }
}

impl fmt::Display for ContentEncoding {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Self::Gzip => write!(f, "gzip"),
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

        write!(
            f,
            "{} {}{}{}{}{}",
            self.http_version,
            self.status_code,
            crlf,
            stringify_headers(&self.headers),
            crlf,
            String::from_utf8(self.body.clone()).unwrap()
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
            "{} {} {}{}{}{}",
            self.http_method, self.http_version, crlf, concatenated_header, crlf, self.body
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

impl ContentEncoding {
    fn parse_content_encoding(raw_content_encoding: &str) -> Option<Vec<ContentEncoding>> {
        let content_encoding_list: Vec<ContentEncoding> = raw_content_encoding
            .trim()
            .split(",")
            .filter_map(|encoding| match encoding.trim() {
                "gzip" => Some(Self::Gzip),
                _ => None,
            })
            .collect();

        if content_encoding_list.is_empty() {
            None
        } else {
            Some(content_encoding_list)
        }
    }
}

fn stringify_headers(headers: &HashMap<String, String>) -> String {
    let crlf = "\r\n";
    headers.iter().fold(String::new(), |acc, (key, val)| {
        format!("{acc}{key}: {val}{crlf}")
    })
}

fn handle_request(request: Request, config: Config) -> Response {
    let request_path_vec: Vec<_> = request
        .request_target
        .split("/")
        .filter(|path_section| path_section.len() > 0)
        .collect();

    let mut response = Response::new_404();
    match request.http_method {
        HttpMethod::Get => {
            if request_path_vec.len() == 0 {
                response.success(vec![]);
            } else if request_path_vec.len() == 1 && request_path_vec[0] == "user-agent" {
                response.success(
                    request
                        .headers
                        .get("User-Agent")
                        .unwrap_or(&String::new())
                        .as_bytes()
                        .to_owned(),
                );
            } else if request_path_vec.len() == 2 && request_path_vec[0] == "echo" {
                response.success(request_path_vec[1].into());
            } else if request_path_vec.len() == 2 && request_path_vec[0] == "files" {
                let contents = read_to_string(format!(
                    "{}{}",
                    config.directory.unwrap_or(String::new()),
                    request_path_vec[1]
                ));

                if let Ok(contents) = contents {
                    response.status_code = StatusCode::Ok;
                    response.body = contents.into();

                    response.add_header(
                        "Content-Type",
                        &ContentType::ApplicationOctetStream.to_string(),
                    );
                    response.add_header("Content-Length", &response.body.len().to_string());
                };
            };
        }
        HttpMethod::Post => {
            if request_path_vec.len() == 2 && request_path_vec[0] == "files" {
                let file_path = format!(
                    "{}{}",
                    config.clone().directory.unwrap_or(String::new()),
                    request_path_vec[1]
                );

                if let Some(parent) = Path::new(&file_path).parent() {
                    let _ = create_dir_all(parent);
                }

                let file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(file_path);

                match file {
                    Ok(mut file) => {
                        let _ = file.write_all(request.body.as_bytes());
                        response.update(HttpVersion::Http1_1, StatusCode::Created, vec![])
                    }
                    Err(_err) => {
                        response.update(HttpVersion::Http1_1, StatusCode::ServerError, vec![])
                    }
                };
            };
        }
    }

    response.integrate_request(&request);
    response
}

fn parse_request(buf_reader: &mut BufReader<&mut TcpStream>) -> Result<Request, HttpException> {
    let raw_request: Vec<String> = buf_reader
        .lines()
        .map(|result| result.unwrap())
        .take_while(|line| !line.is_empty())
        .collect();

    let (status_line, raw_headers) = (&raw_request[0], &raw_request[1..]);

    let [raw_method, request_target, raw_version] =
        status_line.split_whitespace().collect::<Vec<&str>>()[..3]
    else {
        return Err(HttpException::InvalidStatusLine(status_line.to_string()));
    };

    let headers: HashMap<String, String> = raw_headers
        .iter()
        .filter_map(|header_line| {
            header_line
                .split_once(":")
                .map(|(key, val)| (key.trim().to_owned(), val.trim().to_owned()))
        })
        .collect();

    let content_length = headers
        .get("Content-Length")
        .and_then(|content_length| content_length.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; content_length];
    let _ = buf_reader.read_exact(&mut body);

    let mut request = Request::new(
        HttpMethod::parse_method(raw_method)?,
        request_target.to_string(),
        HttpVersion::parse_version(raw_version)?,
        headers,
        String::from_utf8(body).unwrap(),
    );

    request.validate_headers();
    Ok(request)
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

    fn execute(&mut self, stream: TcpStream, config: Config) {
        self.current_connections.retain(|jh| !jh.is_finished());

        if self.current_connections.len() < self.max_connections {
            println!(
                "=== Connection Established @ Thread {} ===",
                self.current_connections.len()
            );
            self.current_connections
                .push(thread::spawn(|| handle_connection(stream, config)));
        } else {
            println!("=== Connection Refused ===");
        }
    }
}

fn handle_connection(mut stream: TcpStream, config: Config) {
    let mut buf_reader = BufReader::new(&mut stream);

    let request = parse_request(&mut buf_reader);
    if let Ok(request) = request {
        let response = handle_request(request, config);
        response.write_to_stream(&mut stream);
    }
}

#[derive(Clone)]
struct Config {
    directory: Option<String>,
}

fn main() {
    let listener = TcpListener::bind("127.0.0.1:4221").unwrap();

    let mut directory: Option<String> = None;
    if args().len() > 1 {
        if std::env::args().nth(1).expect("no pattern given") == "--directory" {
            directory = Some(args().nth(2).expect("no pattern given"));
        }
    }
    let config = Config { directory };

    let mut pool = ThreadPool::new(5);
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => pool.execute(stream, config.clone()),
            Err(e) => {
                println!("error: {}", e);
            }
        }
    }
}
