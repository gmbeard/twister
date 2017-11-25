pub mod parser;

trait FromBytes : Sized {
    fn from_bytes(bytes: &[u8]) -> Option<Self>;
}

#[derive(Debug, PartialEq)]
pub enum HttpMethod {
    Connect,
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Other(Vec<u8>),
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

impl<'a> From<&'a [u8]> for HttpMethod {
    fn from(bytes: &[u8]) -> HttpMethod {
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

        HttpMethod::Other(bytes.to_vec())
    }
}
