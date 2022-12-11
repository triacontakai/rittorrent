mod args;
mod connections;
mod file;
mod http;
mod peers;
mod threads;
mod timer;
mod torrent;
mod tracker;

use anyhow::Result;
use args::PEER_ID;
use file::DownloadFile;
use peers::connect_to_peer;
use threads::Response;
use tracker::{request, TrackerRequest};

use std::{collections::HashMap, net::TcpListener, sync::mpsc};

use crate::args::{ARGS, METAINFO};
use crate::peers::{spawn_peer_thread, PeerRequest};

const DIGEST_SIZE: usize = 20;

fn main() -> Result<()> {
    // we do a little arg parsing
    lazy_static::initialize(&ARGS);

    // map of addresses to channel senders
    let mut peers = HashMap::new();

    // this is how each thread will communicate back with main thread
    let (tx, rx) = mpsc::channel();

    let server = TcpListener::bind("0.0.0.0:5000")?;
    connections::spawn_accept_thread(server, tx.clone());
    let tracker_sender = tracker::spawn_tracker_thread(tx.clone());

    // open file
    let hashes: Vec<[u8; 20]> = METAINFO
        .info
        .pieces
        .chunks_exact(DIGEST_SIZE)
        .map(|x| x.try_into().expect("malformed torrent piece hashes"))
        .collect();
    let file = DownloadFile::new(
        METAINFO.info.name.clone(),
        &hashes,
        METAINFO.info.piece_length,
        METAINFO.info.length,
    )?;

    // send initial starting request
    let tracker_req = TrackerRequest {
        url: METAINFO.announce.clone(),
        request: request::Request {
            info_hash: METAINFO.info_hash(),
            peer_id: *PEER_ID,
            my_port: ARGS.port.unwrap_or(5000),
            uploaded: 0,
            downloaded: 0,
            left: todo!(),
            event: Some(request::Event::Started),
        },
    };
    tracker_sender
        .send(tracker_req)
        .expect("Failed to send request to tracker thread");

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
            Response::Timer(_) => unimplemented!(),
        }
    }

    Ok(())
}
