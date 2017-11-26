use std::io::{self, Read, Write};
use std::mem;
use std::str;

use twister_http::{HttpMethod, Header, Request};
use twister_http::parser::HttpObjectParser;

fn read_into<S: Read>(buffer: &mut Vec<u8>, from: &mut S) -> Result<u64, io::Error> {
    let mut tmp = [0_u8; 512];
    let n = from.read(&mut tmp)?;
    buffer.extend(&tmp[..n]);
    Ok(n as _)
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
                debug!("Reading initial request");
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
                    Ok(ResponseHandlerResult::Done(stream)) => return Ok(Some(stream)),
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
                    Ok(0) => return Ok(Some(inside)),
                    Ok(_) => ConnectionState::TunnellingRead(inside, outside),
                    Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => ConnectionState::TunnellingWrite(outside, inside),
                    _ => return Ok(Some(inside)),
                }
            },

            ConnectionState::TunnellingWrite(mut outside, mut inside) => {
                match io::copy(&mut outside, &mut inside) {
                    Ok(0) => return Ok(Some(inside)),
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

#[cfg_attr(test, derive(Debug, PartialEq))]
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
        let n = match read_into(&mut self.1, self.0.as_mut().unwrap()) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => 0,
            Err(e) => return Err(e),
        };

        debug!("Read {} bytes of request", n);

        let mut headers = [Header::default(); 32];
        let object = HttpObjectParser::new(&mut headers)
            .parse::<Request>(&*self.1);

        if object.is_none() {
            debug!("Request not done: {}", ::std::str::from_utf8(&*self.1).unwrap());
            return Ok(RequestHandlerResult::MoreDataRequired);
        }

        let object = object.unwrap();

        debug!("Recieved request for {}", ::std::str::from_utf8(object.path).unwrap());

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
    use std::cmp;

    struct Trickle<T>(T);

    impl<T: Read + Write> Trickle<T> {
        fn new(stream: T) -> Trickle<T> {
            Trickle(stream)
        }

        fn into_inner(self) -> T {
            self.0
        }
    }

    impl<T: Read> Read for Trickle<T> {
        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, io::Error> {
            let to_read = cmp::min(1, buffer.len());
            return self.0.read(&mut buffer[..to_read]);
        }
    }

    impl<T: Write> Write for Trickle<T> {
        fn write(&mut self, buffer: &[u8]) -> Result<usize, io::Error> {
            let to_write = cmp::min(1, buffer.len());
            self.0.write(&buffer[..to_write])
        }

        fn flush(&mut self) -> Result<(), io::Error> {
            self.0.flush()
        }
    }

    enum StagedRead {
        Connect(usize, Cursor<Vec<u8>>, Cursor<Vec<u8>>),
        Request(Cursor<Vec<u8>>, Cursor<Vec<u8>>),
        Done,
    }

    impl StagedRead {
        fn new() -> StagedRead {
            StagedRead::Connect(
                b"CONNECT source HTTP/1.0\r\n\r\n".len(),
                Cursor::new(
                    b"CONNECT source HTTP/1.0\r\n\
                      \r\n\
                      GET /index.html HTTP/1.0\r\n\
                      \r\n".to_vec()),
                Cursor::new(vec![])
            )
        }

        fn input_buffer_mut(&mut self) -> &mut Cursor<Vec<u8>> {
            match *self {
                StagedRead::Connect(.., ref mut input) => input,
                StagedRead::Request(_ , ref mut input) => input,
                StagedRead::Done => panic!("Stream invalid"),
            }
        }

        fn into_inner(mut self) -> (Cursor<Vec<u8>>, Cursor<Vec<u8>>) {
            match mem::replace(&mut self, StagedRead::Done) {
                StagedRead::Connect(_, input, output) => (input, output),
                StagedRead::Request(input, output) => (input, output),
                StagedRead::Done => panic!("Stream invalid"),
            }
        }
    }

    impl Read for StagedRead {
        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, io::Error> {
            let (result, next) = match mem::replace(self, StagedRead::Done) {
                StagedRead::Connect(read, mut output, input) => {
                    if read == 0 {
                        (Err(io::ErrorKind::WouldBlock.into()), StagedRead::Request(output, input))
                    }
                    else {
                        let to_read = cmp::min(read, buffer.len());
                        let n = output.read(&mut buffer[..to_read])?;
                        (Ok(n), StagedRead::Connect(read - n, output, input))
                    }
                },
                StagedRead::Request(mut output, input) => (output.read(buffer), StagedRead::Request(output, input)),
                StagedRead::Done => panic!("Stread invalid!"),
            };

            *self= next;
            result
        }
    }

    impl Write for StagedRead {
        fn write(&mut self, buffer: &[u8]) -> Result<usize, io::Error> {
            self.input_buffer_mut().write(buffer)
        }

        fn flush(&mut self) -> Result<(), io::Error> {
            self.input_buffer_mut().flush()
        }
    }

    #[test]
    fn handle_connect_request() {
        let mut stream = Trickle(StagedRead::new());
//        let mut stream = Trickle(Cursor::new(b"CONNECT source HTTP/1.0\r\n\r\n".to_vec()));
        let mut handler = RequestHandler::new(stream);

        let dest = loop {
            match handler.poll().unwrap() {
                RequestHandlerResult::MoreDataRequired => continue,
                RequestHandlerResult::WantsProxy(dest, _) => break dest,
                RequestHandlerResult::WantsResource(dest, _) => panic!("Got WantsResource {}", dest),
                RequestHandlerResult::Invalid => panic!("Got Invalid"),
            }
        };

        assert_eq!("source", &*dest);
    }

    #[test]
    fn proxy_request() {
        let upstream = b"Hello, World!".to_vec();
        let mut requested_upstream = false;

        let s = {
            let mut conn = Connection::new(Trickle::new(StagedRead::new()), |dest| {
                requested_upstream = dest == "source";
                Trickle::new(Cursor::new(upstream.clone()))
            });

            let s = loop {
                if let Some(stream) = conn.poll().unwrap() {
                    break stream;
                }
            };

            s
        };

        assert!(requested_upstream);
        let (stream, sink) = s.into_inner().into_inner();
        let input = sink.into_inner();
        let output = stream.into_inner();

//        assert_eq!("GET / HTTP/1.0\r\n\r\n", str::from_utf8(&*output).unwrap());
        assert_eq!("HTTP/1.1 200 OK\r\n\r\nHello, World!", str::from_utf8(&*input).unwrap());
    }
}

