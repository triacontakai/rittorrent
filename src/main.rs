mod args;
mod connections;
mod file;
mod http;
mod peers;
mod threads;
mod torrent;
mod tracker;

use anyhow::Result;
use peers::connect_to_peer;
use threads::Response;

use std::{collections::HashMap, net::TcpListener, sync::mpsc};

use crate::args::{ARGS, METAINFO};
use crate::peers::{spawn_peer_thread, PeerRequest};

fn main() -> Result<()> {
    // we do a little arg parsing
    lazy_static::initialize(&ARGS);

    // map of addresses to channel senders
    let mut peers = HashMap::new();

    let (tx, rx) = mpsc::channel();

    let server = TcpListener::bind("0.0.0.0:5000")?;

    connections::spawn_accept_thread(server, tx.clone());
    let tracker_sender = tracker::spawn_tracker_thread(tx.clone());

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
                println!("received response {:#?}", data);
            }
            Response::Tracker(data) => {
                println!("main thread received response {:#?}", data);
            }
        }
    }

    Ok(())
}

// tracker test
//tracker_sender.send(tracker::ThreadRequest {
//    url: String::from("http://128.8.126.63:21212/announce"),
//    request: tracker::request::Request {
//        info_hash: hex!("d4437aed681cb06c5ecbcf2c7f590ae8a3f73aeb"),
//        peer_id: String::from("deadbeefdeafbeefbeef"),
//        my_port: 5000,
//        uploaded: 0,
//        downloaded: 0,
//        left: 0,
//        event: Some(tracker::request::Event::Started),
//    },
//})?;
