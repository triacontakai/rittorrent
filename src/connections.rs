use crate::threads::Response;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use crossbeam::channel::{self, Sender};
use log::{info, warn};

const CONNECTION_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug)]
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

pub fn spawn_connect_thread(sender: Sender<Response>) -> Sender<SocketAddr> {
    let (tx, rx) = channel::unbounded::<SocketAddr>();
    thread::spawn(move || {
        for req in rx {
            info!("Connecting to peer at {:?}", req);
            let Ok(stream) = TcpStream::connect_timeout(&req, CONNECTION_TIMEOUT) else {
                warn!(" --> Connection to peer at {:?} timed out", req);
                continue;
            };
            info!(" --> Connection successful");

            sender
                .send(Response::Connection(ConnectionData { peer: stream }))
                .expect("Receiver hung up!");
        }
    });

    tx
}
