use std::io::{self, Read, Write};
use std::mem;
use std::str;

use twister_http::{HttpMethod, Header, Request};
use twister_http::parser::HttpObjectParser;

fn read_into<S: Read>(buffer: &mut Vec<u8>, from: &mut S) -> Result<u64, io::Error> {
    let mut tmp = [0_u8; 512];
    match io::copy(from, &mut &mut tmp[..]) {
        Ok(n) => {
            buffer.extend(&tmp[..n as usize]);
            return Ok(n);
        }
        result => result,
    }
}

pub struct Connection<S, F, U>
    where S: Read + Write,
          U: Read + Write,
{
    state: ConnectionState<S, U>,
    upstream_fn: F,
}

enum ConnectionState<S: Read + Write, U: Read + Write> {
    Request(RequestHandler<S>),
    Response(ResponseHandler<S>),
    AcceptingProxyRequest(ResponseHandler<S>, U),
    TunnellingWrite(U, S),
    TunnellingRead(S, U),
    Done,
}

impl<S, F, U> Connection<S, F, U> 
    where S: Read + Write,
          F: FnMut(&str) -> U,
          U: Read + Write
{
    pub fn new(stream: S, f: F) -> Connection<S, F, U> {
        Connection {
            state: ConnectionState::new(stream),
            upstream_fn: f,
        }
    }

    pub fn poll(&mut self) -> Result<Option<S>, io::Error> {

        let next = match mem::replace(&mut self.state, ConnectionState::Done) {
            ConnectionState::Request(mut handler) => {
                match handler.poll() {
                    Ok(RequestHandlerResult::MoreDataRequired) => 
                        ConnectionState::Request(handler),

                    Ok(RequestHandlerResult::WantsProxy(dest, stream)) => 
                        ConnectionState::AcceptingProxyRequest(
                            ResponseHandler::new(b"HTTP/1.1 200 OK\r\n\r\n".to_vec(), stream), 
                            (self.upstream_fn)(&dest)),

                    Ok(RequestHandlerResult::WantsResource(_, stream)) => 
                        ConnectionState::Response(
                            ResponseHandler::new(b"HTTP/1.1 404 Not Found\r\n\r\n".to_vec(), stream)),

                    _ => return Ok(Some(handler.into_inner())),
                }
            },
            ConnectionState::Response(mut handler) => {
                match handler.poll() {
                    Ok(ResponseHandlerResult::Done(_)) => return Ok(Some(handler.into_inner())),
                    Ok(ResponseHandlerResult::NotDone) => ConnectionState::Response(handler),
                    _ => return Ok(Some(handler.into_inner())),
                }
            },

            ConnectionState::AcceptingProxyRequest(mut handler, upstream) => {
                match handler.poll() {
                    Ok(ResponseHandlerResult::Done(stream)) => ConnectionState::TunnellingRead(stream, upstream),
                    Ok(ResponseHandlerResult::NotDone) => ConnectionState::AcceptingProxyRequest(handler, upstream),
                    _ => return Ok(Some(handler.into_inner())),
                }
            },

            ConnectionState::TunnellingRead(mut inside, mut outside) => {
                match io::copy(&mut inside, &mut outside) {
                    Ok(n) if n == 0 => return Ok(Some(inside)),
                    Ok(_) => ConnectionState::TunnellingRead(inside, outside),
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => ConnectionState::TunnellingWrite(outside, inside),
                    _ => return Ok(Some(inside)),
                }
            },

            ConnectionState::TunnellingWrite(mut outside, mut inside) => {
                match io::copy(&mut outside, &mut inside) {
                    Ok(n) if n == 0 => return Ok(Some(inside)),
                    Ok(_) => ConnectionState::TunnellingWrite(outside, inside),
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => ConnectionState::TunnellingRead(inside, outside),
                    _ => return Ok(Some(inside)),
                }
            },
            ConnectionState::Done => panic!("poll called on done!"),
        };

        self.state = next;
        return Ok(None);
    }
}

impl<S, U> ConnectionState<S, U>
    where S: Read + Write,
          U: Read + Write,
{
    pub fn new(stream: S) -> ConnectionState<S, U> {
        ConnectionState::Request(RequestHandler::new(stream))
    }
}

enum RequestHandlerResult<S> {
    MoreDataRequired,
    WantsProxy(String, S),
    WantsResource(String, S),
    Invalid,
}

enum ResponseHandlerResult<S> {
    Done(S),
    NotDone,
}

struct ResponseHandler<S: Write>(Option<S>, io::Cursor<Vec<u8>>);

struct RequestHandler<S: Read>(Option<S>, Vec<u8>);

impl<S: Read> RequestHandler<S> {
    fn new(stream: S) -> RequestHandler<S> {
        RequestHandler(Some(stream), vec![])
    }

    fn poll(&mut self) -> Result<RequestHandlerResult<S>, io::Error> {
        let n = read_into(&mut self.1, self.0.as_mut().unwrap())?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }

        let mut headers = [Header::default(); 32];
        let object = HttpObjectParser::new(&mut headers)
            .parse::<Request>(&*self.1);

        if object.is_none() {
            return Ok(RequestHandlerResult::MoreDataRequired);
        }

        let object = object.unwrap();

        match object.method {
            HttpMethod::Connect => 
                Ok(RequestHandlerResult::WantsProxy(
                    str::from_utf8(object.path).unwrap().to_string(), self.0.take().unwrap())),
            HttpMethod::Get => 
                Ok(RequestHandlerResult::WantsResource(
                    str::from_utf8(object.path).unwrap().to_string(), self.0.take().unwrap())),
            _ => Ok(RequestHandlerResult::Invalid)
        }
    }

    fn into_inner(mut self) -> S {
        self.0.take().unwrap()
    }
}

impl<S: Write> ResponseHandler<S> {
    fn new(response: Vec<u8>, stream: S) -> ResponseHandler<S> {
        ResponseHandler(Some(stream), io::Cursor::new(response))
    }

    fn poll(&mut self) -> Result<ResponseHandlerResult<S>, io::Error> {
        let n = io::copy(&mut self.1, self.0.as_mut().unwrap())?;
        if n == 0 {
            Ok(ResponseHandlerResult::Done(self.0.take().unwrap()))
        }
        else {
            Ok(ResponseHandlerResult::NotDone)
        }
    }

    fn into_inner(mut self) -> S {
        self.0.take().unwrap()
    }
}

#[cfg(test)]
mod connection_should {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn proxy_request() {
        let request = b"CONNECT source HTTP/1.1\r\n\
                        \r\n".to_vec();
        let upstream = b"Hello, World!".to_vec();
        let mut requested_upstream = false;

        let s = {
            let mut conn = Connection::new(Cursor::new(request), |dest| {
                requested_upstream = dest == "source";
                Cursor::new(upstream.clone())
            });

            let s = loop {
                if let Some(stream) = conn.poll().unwrap() {
                    break stream;
                }
            };

            s
        };

        assert!(requested_upstream);
        let v = s.into_inner();
        assert_eq!(
            "CONNECT source HTTP/1.1\r\n\
             \r\n\
             HTTP/1.1 200 OK\r\n\
             \r\n\
             Hello, World!",
            str::from_utf8(&*v).unwrap());
    }
}

