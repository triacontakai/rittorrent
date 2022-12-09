mod connections;
mod peers;
mod threads;

// temp delete me
mod args;
mod http;
mod torrent;
mod tracker;

mod file;

use anyhow::Result;
use threads::Response;

use std::{collections::HashMap, net::TcpListener, sync::mpsc};

use crate::args::ARGS;
use crate::peers::{spawn_peer_thread, PeerRequest};

fn main() -> Result<()> {
    // we do a little arg parsing
    lazy_static::initialize(&ARGS);

    let mut peers = HashMap::new();

    let (tx, rx) = mpsc::channel();

    let server = TcpListener::bind("0.0.0.0:5000")?;

    connections::spawn_accept_thread(server, tx.clone());

    for resp in rx.iter() {
        match resp {
            Response::Connection(data) => {
                println!("{:?}", data.peer);

                let addr = data.peer.peer_addr()?;
                let peer_sender = spawn_peer_thread(data.peer, tx.clone());
                peers.insert(addr, peer_sender.clone());

                peer_sender.send(PeerRequest::GetInfo)?;
            }
            Response::Peer(data) => {
                println!("received response {:?}", data);
            },
        }
    }

    Ok(())
}
