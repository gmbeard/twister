use core::mem;
use Header;

fn skip_newline(data: &[u8]) -> &[u8] {
    data.iter()
        .position(|b| *b != b'\r' && *b != b'\n')
        .map(|p| {
            let (_, tail) = data.split_at(p);
            tail
        })
        .unwrap_or_else(|| &[])
}

fn skip_whitespace(data: &[u8]) -> &[u8] {
    data.iter()
        .position(|byte| *byte != b' ' && *byte != b'\t')
        .map(|p| {
            let (_, tail) = data.split_at(p);
            tail
        })
        .unwrap_or_else(|| &[])
}

fn skip_header_separator(data: &[u8]) -> &[u8] {
    data.iter()
        .position(|byte| *byte != b'\t' && *byte != b' ' && *byte != b':')
        .map(|p| {
            let (_, tail) = data.split_at(p);
            tail
        })
        .unwrap_or_else(|| &[])
}

fn split_as_first_newline(data: &[u8]) -> Option<(&[u8], &[u8])> {
    data.iter()
        .position(|byte| *byte == b'\r' || *byte == b'\n')
        .map(|p| data.split_at(p))
}

fn split_at_first_whitespace(data: &[u8]) -> Option<(&[u8], &[u8])> {
    data.iter()
        .position(|byte| *byte == b' ' || *byte == b'\t')
        .map(|p| data.split_at(p))
}

fn split_at_first_header_separator(data: &[u8]) -> Option<(&[u8], &[u8])> {
    data.iter()
        .position(|byte| *byte == b':')
        .map(|p| data.split_at(p))
}

/// A type to parse the *protocol line* of a HTTP request.
/// E.g.
///
/// ```no_compile
/// CONNECT docs.rs:443 HTTP/1.1
/// ```
///
/// `ProtocolParser` is non-allocating and works purely
/// on borrowed data, hence the lifetime parameter.
pub enum ProtocolParser<'a> {
    #[doc(hidden)]
    Method(&'a [u8]),
    #[doc(hidden)]
    Path(&'a [u8], &'a [u8]),
    #[doc(hidden)]
    Version(&'a [u8], &'a [u8], &'a [u8]),
    #[doc(hidden)]
    Done,
}

/// A type to parse a *header* of a HTTP request.
/// E.g.
///
/// ```no_compile
/// Content-Type: text/json; charset=utf-8
/// ```
///
/// `HeaderParser` is non-allocating and works purely
/// on borrowed data, hence the lifetime parameter.
pub enum HeaderParser<'a> {
    #[doc(hidden)]
    Name(&'a [u8]),
    #[doc(hidden)]
    Value(&'a [u8], &'a [u8]),
    #[doc(hidden)]
    Done,
}

impl<'a> ProtocolParser<'a> {
    /// Creates a new instance. `bytes` must be at the start
    /// of the *protocol line* for any parsing to be successful.
    pub fn new(bytes: &'a [u8]) -> ProtocolParser<'a> {
        ProtocolParser::Method(bytes)
    }

    /// Parses the protocol line contained at the start of 
    /// the data provided to [`ProtocolParser::new`]
    ///
    /// Parse requires `&mut self` because it is internally
    /// represented as a state machine and so must modify
    /// itself in the process of parsing.
    ///
    /// # Return Value
    /// If parsing is successful, a tuple is returned consisting
    /// of `(method: HttpMethod, path: &[u8], version: &[u8], 
    /// remaining: &[u8])`. `remaining` is any remaining data found 
    /// after the protocol line. The parser consumes the trailing `\r\n` 
    /// bytes of the protocol line so, assuming a well-formed request, 
    /// `remaining` is at the very start of the first header line.
    ///
    /// If parsing can't be completed because either the data is
    /// incomplete, or it is invalid, then this function returns
    /// `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use twister_http::parser::ProtocolParser;
    /// use twister_http::HttpMethod;
    ///
    /// const HTTP: &'static [u8] = b"GET /index.html HTTP/1.1\r\n";
    ///
    /// let mut parser = ProtocolParser::new(HTTP);
    /// let (method, path, version, tail) = parser.parse().unwrap();
    ///
    /// assert_eq!(HttpMethod::Get, method.into());
    /// assert_eq!(b"/index.html", path);
    /// assert_eq!(b"HTTP/1.1", version);
    /// assert_eq!(0, tail.len());
    /// ```
    ///
    /// [`ProtocolParser::new`]: enum.ProtocolParser.html#method.new
    pub fn parse(&mut self) -> Option<(&'a [u8], &'a [u8], &'a [u8], &'a [u8])> {
        use self::ProtocolParser::*;
        loop {
            let next = match mem::replace(self, Done) {
                Method(data) => {
                    split_at_first_whitespace(data)
                        .map(|(val, tail)| {
                            Path(val, skip_whitespace(tail))
                        })
                },
                Path(method, data) => {
                    split_at_first_whitespace(data)
                        .map(|(val, tail)| {
                            Version(method, val, skip_whitespace(tail))
                        })
                },
                Version(method, url, data) => {
                    return split_as_first_newline(data)
                        .map(|(val, tail)| {
                            (method, url, val, skip_newline(tail))
                        });
                },
                Done => panic!("parse called after done"),
            };

            if let Some(next) = next {
                *self = next;
            }
            else {
                return None
            }
        }
    }
}

impl<'a> HeaderParser<'a> {
    /// Creates a new instance. `bytes` must be at the start
    /// of the *header line* for any parsing to be successful.
    pub fn new(bytes: &'a [u8]) -> HeaderParser<'a> {
        HeaderParser::Name(bytes)
    }

    /// Parses a single HTTP header contained at the start of 
    /// the data provided to [`HeaderParser::new`]
    ///
    /// Parsing requires `&mut self` because it is internally
    /// represented as a state machine and so must modify
    /// itself in the process of parsing.
    ///
    /// # Return Value
    /// If parsing is successful, a tuple is returned consisting
    /// of `(header: Header, remaining: &[u8])`. `remaining` is 
    /// any remaining data found after the protocol line. The parser 
    /// consumes the trailing `\r\n` bytes of the protocol line so, 
    /// assuming a well-formed request, `remaining` is at the very start 
    /// of the next header line.
    ///
    /// If parsing can't be completed because either the data is
    /// incomplete, or it is invalid, then this function returns
    /// `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use twister_http::Header;
    /// use twister_http::parser::HeaderParser;
    ///
    /// const HTTP: &'static [u8] = b"Content-Type: text/xml; charset=utf8\r\n";
    ///
    /// let mut parser = HeaderParser::new(HTTP);
    /// let (Header (name, value), remaining) = parser.parse().unwrap();
    ///
    /// assert_eq!(b"Content-Type", name);
    /// assert_eq!(b"text/xml; charset=utf8", value);
    /// assert_eq!(0, remaining.len());
    /// ```
    ///
    /// [`HeaderParser::new`]: enum.HeaderParser.html#method.new
    pub fn parse(&mut self) -> Option<(Header<'a>, &'a [u8])> {
        use self::HeaderParser::*;

        loop {
            let next = match mem::replace(self, Done) {
                Name(data) => {
                    if let Some(state) = split_at_first_header_separator(data)
                        .map(|(val, tail)| {
                            Value(val, skip_header_separator(tail))
                        })
                    {
                        Some(state)
                    }
                    else {
                        return Some((Header(&[], &[]), skip_newline(data)));
                    }
                },
                Value(name, data) => {
                    return split_as_first_newline(data)
                        .map(|(val, tail)| {
                            (Header(name, val), skip_newline(tail))
                        });
                },
                Done => panic!("parse called on finished result"),
            };

            if let Some(next) = next {
                *self = next;
            }
            else {
                return None;
            }
        }
    }
}

/// A non-allocating HTTP object parser
pub enum HttpObjectParser<'a> {
    #[doc(hidden)]
    NotStarted(&'a mut [Header<'a>]),
    #[doc(hidden)]
    Protocol(&'a mut [Header<'a>], ProtocolParser<'a>),
    #[doc(hidden)]
    Headers(&'a [u8], &'a [u8], &'a [u8], &'a mut [Header<'a>], HeaderParser<'a>),
    #[doc(hidden)]
    Done
}

impl<'a> HttpObjectParser<'a> 
{
    /// Creates a new instance. `headers` will be used to store all
    /// the headers found in the HTTP object when [`parse`] is called. It
    /// is important to provide enough space in `headers`, otherwise [`parse`]
    /// will `panic`.
    ///
    /// # Examples
    /// ```
    /// use twister_http::Header;
    /// use twister_http::parser::HttpObjectParser;
    ///
    /// let mut headers = vec![Header::default(); 16];
    /// let mut parser = HttpObjectParser::new(&mut headers);
    /// ```
    /// [`parse`]: enum.ResponseParser.html#method.parse
    pub fn new(headers: &'a mut [Header<'a>]) -> HttpObjectParser<'a> {
        HttpObjectParser::NotStarted(headers)
    }

    /// Parses a HTTP object.
    ///
    /// # Return Value
    /// If parsing succeeds, a `T` is returned. If parsing fails
    /// due to an incomplete, or invalid object then `None` is returned.
    ///
    /// # Panics
    /// This function will `panic` if there is not enough storage for all
    /// the headers found in the HTTP object.
    ///
    /// # Examples
    ///
    /// Parsing a Response
    ///
    /// ```
    /// use std::str;
    /// use twister_http::{Header, Response};
    /// use twister_http::parser::HttpObjectParser;
    ///
    /// const HTTP: &'static [u8] = 
    ///     b"HTTP/1.1 200 OK\r\n\
    ///       Content-Type: text/plain\r\n\
    ///       Content-Length: 13\r\n\
    ///       \r\n\
    ///       Hello, World!";
    ///
    /// let mut headers = [Header::default(); 16];
    /// let mut parser = HttpObjectParser::new(&mut headers);
    /// let http_object = parser.parse::<Response>(HTTP).unwrap();
    ///
    /// assert_eq!("HTTP/1.1", str::from_utf8(http_object.version).unwrap());
    /// assert_eq!("200", str::from_utf8(http_object.status_code).unwrap());
    /// assert_eq!("OK", str::from_utf8(http_object.status_text).unwrap());
    /// assert_eq!(2, http_object.headers.len());
    ///
    /// let mut iter = http_object.headers.iter();
    /// assert_eq!(Header(b"Content-Type", b"text/plain"), *iter.next().unwrap());
    /// assert_eq!(Header(b"Content-Length", b"13"), *iter.next().unwrap());
    ///
    /// assert_eq!("Hello, World!", str::from_utf8(http_object.body).unwrap());
    /// ```
    ///
    /// Parsing a Request
    ///
    /// ```
    /// use std::str;
    /// use twister_http::{Header, HttpMethod, Request};
    /// use twister_http::parser::HttpObjectParser;
    ///
    /// const HTTP: &'static [u8] = 
    ///     b"POST /api/resource HTTP/1.1\r\n\
    ///       Host: docs.rs\r\n\
    ///       Content-Type: text/plain\r\n\
    ///       Content-Length: 13\r\n\
    ///       \r\n\
    ///       Hello, World!";
    ///
    /// let mut headers = [Header::default(); 16];
    /// let mut parser = HttpObjectParser::new(&mut headers);
    /// let http_object = parser.parse::<Request>(HTTP).unwrap();
    ///
    /// assert_eq!(HttpMethod::Post, http_object.method);
    /// assert_eq!("/api/resource", str::from_utf8(http_object.path).unwrap());
    /// assert_eq!(3, http_object.headers.len());
    ///
    /// let mut iter = http_object.headers.iter();
    /// assert_eq!(Header(b"Host", b"docs.rs"), *iter.next().unwrap());
    /// assert_eq!(Header(b"Content-Type", b"text/plain"), *iter.next().unwrap());
    /// assert_eq!(Header(b"Content-Length", b"13"), *iter.next().unwrap());
    ///
    /// assert_eq!("Hello, World!", str::from_utf8(http_object.body).unwrap());
    /// ```
    pub fn parse<T>(&mut self, data: &'a [u8]) -> Option<T>
        where T: From<(&'a [u8], &'a [u8], &'a [u8], &'a [Header<'a>], &'a [u8])>
    {
        use self::HttpObjectParser::*;

        loop {
            let next = match mem::replace(self, Done) {
                NotStarted(headers) => Some(Protocol(headers, ProtocolParser::new(data))),
                Protocol(headers, mut parser) => {
                    parser.parse()
                        .map(move |(part1, part2, part3, tail)| {
                            Headers(part1, part2, part3, headers, HeaderParser::new(tail))
                        })
                },
                Headers(part1, part2, part3, headers, mut parser) => {
                    let mut header_pos = 0;
                    while let Some((Header(name, val), tail)) = parser.parse() {

                        if name.len() == 0 {
                            let parts = (part1, part2, part3, &headers[..header_pos], tail);
                            return Some(parts.into());
                        }

                        if header_pos >= headers.len() {
                            panic!("Not enough room for headers");
                        }

                        headers[header_pos] = Header(name, val);
                        parser = HeaderParser::new(tail);
                        header_pos += 1;
                    }
                    
                    Some(Done)
                },
                Done => panic!("parse called on finished result"),
            };

            if let Some(next) = next {
                *self = next;
            }
            else {
                return None;
            }
        }
    }
}

#[cfg(test)]
mod protocol_parser_should {
    use super::*;
    use std::str;
    use HttpMethod;

    #[test]
    fn parse_protocol_header() {
        let proxy_connect = include_bytes!("../tests/proxy_connect.txt");
        let mut p = ProtocolParser::new(proxy_connect);
        let (method, url, version, _) = p.parse().unwrap();

        assert_eq!(HttpMethod::Connect, method.into());
        assert_eq!("docs.rs:443", str::from_utf8(url).unwrap());
        assert_eq!("HTTP/1.1", str::from_utf8(version).unwrap());
    }
}

#[cfg(test)]
mod header_parser_should {
    use super::*;
    use std::str;
   
    #[test]
    fn parse_multiple_headers() {
        let proxy_connect = include_bytes!("../tests/proxy_connect.txt");
        let (_, headers) = split_as_first_newline(proxy_connect).unwrap();
        let headers = skip_newline(headers);

        let mut p = HeaderParser::new(headers);
        let (Header(name, val), tail) = p.parse().unwrap();

        assert_eq!("User-Agent", str::from_utf8(name).unwrap());
        assert_eq!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:59.0) \
            Gecko/20100101 Firefox/59.0", str::from_utf8(val).unwrap());

        let mut p = HeaderParser::new(tail);
        let (Header(name, val), tail) = p.parse().unwrap();

        assert_eq!("Proxy-Connection", str::from_utf8(name).unwrap());
        assert_eq!(
            "keep-alive", str::from_utf8(val).unwrap());

        let mut p = HeaderParser::new(tail);
        let (Header(name, val), tail) = p.parse().unwrap();

        assert_eq!("Connection", str::from_utf8(name).unwrap());
        assert_eq!(
            "keep-alive", str::from_utf8(val).unwrap());

        let mut p = HeaderParser::new(tail);
        let (Header(name, val), tail) = p.parse().unwrap();

        assert_eq!("Host", str::from_utf8(name).unwrap());
        assert_eq!(
            "docs.rs:443", str::from_utf8(val).unwrap());

        let (Header(_, _), tail) = HeaderParser::new(tail).parse().unwrap();
        assert_eq!("Hello, World!\r\n", str::from_utf8(tail).unwrap());

    }

    #[test]
    fn parse_a_header() {
        let proxy_connect = include_bytes!("../tests/proxy_connect.txt");
        let (_, headers) = split_as_first_newline(proxy_connect).unwrap();
        let headers = skip_newline(headers);

        let mut p = HeaderParser::new(headers);
        let (Header(name, val), _) = p.parse().unwrap();

        assert_eq!("User-Agent", str::from_utf8(name).unwrap());
        assert_eq!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:59.0) \
            Gecko/20100101 Firefox/59.0", str::from_utf8(val).unwrap());
    }
}

#[cfg(test)]
mod request_parser_should {
    use super::*;
    use std::str;
    use {Request, HttpMethod};

    #[test]
    fn parse_a_request() {
        use std::mem;

        let proxy_connect = include_bytes!("../tests/proxy_connect.txt");
        let mut header_size = 16;
        loop {
            let mut headers = vec![Header::default(); header_size];
            if let Some(r) = HttpObjectParser::new(&mut headers).parse::<Request>(proxy_connect)
            {

                assert_eq!(HttpMethod::Connect, r.method);
                assert_eq!("docs.rs:443", str::from_utf8(r.path).unwrap());
                assert_eq!(4, r.headers.len());
                assert_eq!("Hello, World!\r\n", str::from_utf8(r.body).unwrap());
                break;
            }

            header_size *= 2;
        }

    }
}
