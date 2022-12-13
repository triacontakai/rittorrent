use crate::threads::Response;
use std::net::{TcpListener, TcpStream};
use std::thread;

use crossbeam::channel::Sender;

pub struct ConnectionData {
    pub peer: TcpStream,
}

pub fn spawn_accept_thread(listener: TcpListener, sender: Sender<Response>) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                sender
                    .send(Response::Connection(ConnectionData { peer: stream }))
                    .expect("Receiver hung up!")
            }
        }
    });
}
