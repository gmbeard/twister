#![cfg_attr(not(test), no_std)]

#[cfg(test)]
extern crate core;

pub mod parser;

trait FromBytes : Sized {
    fn from_bytes(bytes: &[u8]) -> Option<Self>;
}

#[derive(Debug, PartialEq)]
pub enum HttpMethod<'a> {
    Connect,
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Other(&'a [u8]),
}

fn to_lower(v: u8) -> u8 {
    match v {
        b'A'...b'Z' => v + (b'a' - b'A'),
        o => o
    }
}

fn which_of(to_find: &[u8], in_set: &[&[u8]]) -> Option<usize> {
    for (i, el) in in_set.iter().enumerate() {
        let eq = el.iter().map(|byte| to_lower(*byte))
            .eq(to_find.iter().map(|byte| to_lower(*byte)));

        if eq {
            return Some(i);
        }
    }

    None
}

impl<'a> From<&'a [u8]> for HttpMethod<'a> {
    fn from(bytes: &'a [u8]) -> HttpMethod<'a> {
        let valid: &[&[u8]] = &[
            b"connect",
            b"Get",
            b"Post",
            b"Put",
            b"Delete",
            b"Patch",
            b"Head",
        ];

        if let Some(n) = which_of(bytes, valid) {
            return match n {
                0 => HttpMethod::Connect,
                1 => HttpMethod::Get,
                2 => HttpMethod::Post,
                3 => HttpMethod::Put,
                4 => HttpMethod::Delete,
                5 => HttpMethod::Patch,
                6 => HttpMethod::Head,
                _ => unreachable!(),
            }
        }

        HttpMethod::Other(bytes)
    }
}

/// A type representing a HTTP header name/value pair. E.g.
///
/// ```no_compile
/// Host: docs.rs:443
/// ```
#[derive(Default, Debug, PartialEq, Clone, Copy)]
pub struct Header<'a>(pub &'a [u8], pub &'a [u8]);

/// A type to represent a HTTP request object
pub struct Request<'a> {
    /// The object's method - E.g. `GET`, `POST`. See [`HttpMethod`]
    /// [`HttpMethod`]: ../enum.HttpMethod.html
    pub method: HttpMethod<'a>,
    /// The path value
    pub path: &'a [u8],
    /// The version string - E.g. `HTTP/1.1`
    pub version: &'a [u8],
    /// The headers contained in the object
    pub headers: &'a [Header<'a>],
    /// The body of the request
    pub body: &'a [u8],
}

impl<'a> From<(&'a [u8], &'a [u8], &'a [u8], &'a [Header<'a>], &'a [u8])> for Request<'a> {
    fn from(parts: (&'a [u8], &'a [u8], &'a [u8], &'a [Header<'a>], &'a [u8])) -> Request<'a> {
        let (method, path, version, headers, body) = parts;
        Request {
            method: method.into(),
            path: path,
            version: version,
            headers: headers,
            body: body,
        }
    }
}

/// A type respresenting a HTTP response object
pub struct Response<'a> {
    /// The version string - E.g. `HTTP/1.1`
    pub version: &'a [u8],
    /// The status code - E.g. `200`, `404`, etc.
    pub status_code: &'a [u8],
    /// The status text - E.g. `OK`, `Not Found`, etc.
    pub status_text: &'a [u8],
    /// The headers contained in the object
    pub headers: &'a [Header<'a>],
    /// The body of the request
    pub body: &'a [u8],
}

impl<'a> From<(&'a [u8], &'a [u8], &'a [u8], &'a [Header<'a>], &'a [u8])> for Response<'a> {
    fn from(parts: (&'a [u8], &'a [u8], &'a [u8], &'a [Header<'a>], &'a [u8])) -> Response<'a> {
        let (version, status, text, headers, body) = parts;
        Response {
            version: version,
            status_code: status,
            status_text: text,
            headers: headers,
            body: body,
        }
    }
}

