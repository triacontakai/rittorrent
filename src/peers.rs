use std::{
    net::{SocketAddr, TcpStream},
    sync::mpsc::{self, Sender},
    thread,
};

use crate::threads::Response;

#[derive(Debug)]
pub enum PeerRequest {
    GetInfo,
}

#[derive(Debug)]
pub struct PeerInfo {
    // basic info
    addr: SocketAddr,
}

#[derive(Debug)]
pub enum PeerResponse {
    PeerInfo(PeerInfo),
}

pub fn spawn_peer_thread(peer: TcpStream, sender: Sender<Response>) -> Sender<PeerRequest> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        for req in rx {
            println!("Received request: {:?}", req);
            sender
                .send(Response::Peer(PeerResponse::PeerInfo(PeerInfo {
                    addr: peer.peer_addr().unwrap(),
                })))
                .expect("hi");
        }
    });

    tx
}
