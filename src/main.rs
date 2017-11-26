extern crate twister_core;
#[macro_use] extern crate log;
extern crate env_logger;

use std::time::Duration;
use std::thread;
use std::net::{TcpListener, TcpStream};
use twister_core::connection::Connection;

fn main() {
    env_logger::init().ok();

    let listener = TcpListener::bind("127.0.0.1:8083").unwrap();

    for stream in listener.incoming() {
        let mut s = stream.unwrap();
        s.set_nonblocking(true).unwrap();
        debug!("Accepted connection");
        let mut conn = Connection::new(s, |dest| {
            println!("Connecting to {}", dest);
            let mut s = TcpStream::connect(dest).unwrap();
            s.set_nonblocking(true).unwrap();
            s
        });

        loop {
            if let Some(_) = conn.poll().unwrap() {
                break;
            }

            thread::sleep(Duration::from_millis(5));
        }
    }
}
