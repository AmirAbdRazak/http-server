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

enum ContentEncoding {
    Gzip,
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

    fn update(&mut self, http_version: HttpVersion, status_code: StatusCode, body: String) {
        self.http_version = http_version;
        self.status_code = status_code;
        self.body = body;
    }

    fn new_404() -> Self {
        Self::new(HttpVersion::Http1_1, StatusCode::NotFound, String::new())
    }

    fn add_header(&mut self, header_name: &str, header_value: &str) {
        let header = format!("{header_name}: {header_value}");
        self.headers.push(header);
    }

    fn integrate_request(&mut self, request: &Request) {
        if let Some(content_encoding) = request.headers.get("Accept-Encoding") {
            self.headers
                .push(format!("Content-Encoding: {content_encoding}"))
        }
    }

    fn success(&mut self, body: &str) {
        self.body = body.to_string();
        self.status_code = StatusCode::Ok;

        self.add_header("Content-Type", &ContentType::TextPlain.to_string());
        self.add_header("Content-Length", &self.body.as_bytes().len().to_string());
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
        if let Entry::Occupied(entry) = self.headers.entry("Accept-Encoding".to_string()) {
            if ContentEncoding::parse_content_encoding(entry.get()).is_err() {
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
            Self::InvalidContentEncoding(raw_content_encoding) => {
                write!(f, "Invalid Content Encoding: {}", raw_content_encoding)
            }
        }
    }
}

impl fmt::Display for Response {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let crlf = "\r\n";
        let concatenated_header = if self.headers.len() > 0 {
            self.headers.join(crlf) + crlf
        } else {
            String::new()
        };

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
    InvalidContentEncoding(String),
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
    fn parse_content_encoding(
        raw_content_encoding: &str,
    ) -> Result<ContentEncoding, HttpException> {
        match raw_content_encoding {
            "gzip" => Ok(Self::Gzip),
            _ => Err(HttpException::InvalidContentEncoding(
                raw_content_encoding.to_owned(),
            )),
        }
    }
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
                response.success("");
            } else if request_path_vec.len() == 1 && request_path_vec[0] == "user-agent" {
                response.success(request.headers.get("User-Agent").unwrap_or(&String::new()));
            } else if request_path_vec.len() == 2 && request_path_vec[0] == "echo" {
                response.success(request_path_vec[1]);
            } else if request_path_vec.len() == 2 && request_path_vec[0] == "files" {
                let contents = read_to_string(format!(
                    "{}{}",
                    config.directory.unwrap_or(String::new()),
                    request_path_vec[1]
                ));

                if let Ok(contents) = contents {
                    response.status_code = StatusCode::Ok;
                    response.body = contents;

                    response.add_header(
                        "Content-Type",
                        &ContentType::ApplicationOctetStream.to_string(),
                    );
                    response.add_header(
                        "Content-Length",
                        &response.body.as_bytes().len().to_string(),
                    );
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
                        response.update(HttpVersion::Http1_1, StatusCode::Created, String::new())
                    }
                    Err(_err) => response.update(
                        HttpVersion::Http1_1,
                        StatusCode::ServerError,
                        String::new(),
                    ),
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
    let response = match request {
        Ok(request) => format!("{}", handle_request(request, config)),
        Err(http_exception) => format!("{}", http_exception),
    };

    let _ = stream.write(response.as_bytes());
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
        } else {
            panic!()
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
