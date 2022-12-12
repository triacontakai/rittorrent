mod args;
mod connections;
mod file;
mod http;
mod peers;
mod threads;
mod timer;
mod torrent;
mod tracker;

use args::PEER_ID;
use file::DownloadFile;
use threads::Response;
use tracker::{request, TrackerRequest};

use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::Sender;
use std::{collections::HashMap, net::TcpListener, sync::mpsc};

use anyhow::Result;
use bitvec::prelude::*;

use crate::args::{ARGS, METAINFO};
use crate::peers::{spawn_peer_thread, PeerRequest};

const DIGEST_SIZE: usize = 20;

struct PeerInfo {
    // channel to send to this peer
    sender: Sender<PeerRequest>,

    // basic state
    choked: bool,
    interested: bool,
    peer_choked: bool,
    peer_interested: bool,

    // which pieces does this peer have?
    has: BitVec<u8, Msb0>,
}

impl PeerInfo {
    // Consumes a TcpStream, creates a new peer thread
    fn new(peer: TcpStream, sender: Sender<Response>) -> Self {
        Self {
            sender: spawn_peer_thread(peer, sender),
            choked: true,
            interested: false,
            peer_choked: true,
            peer_interested: false,
            has: bitvec![u8, Msb0; 0; METAINFO.info.pieces.len()],
        }
    }
}

fn main() -> Result<()> {
    // we do a little arg parsing
    lazy_static::initialize(&ARGS);

    // map of addresses to peer structs
    let mut peers: HashMap<SocketAddr, PeerInfo> = HashMap::new();

    // this is how each thread will communicate back with main thread
    let (tx, rx) = mpsc::channel();

    let server = TcpListener::bind("0.0.0.0:5000")?;
    connections::spawn_accept_thread(server, tx.clone());
    let tracker_sender = tracker::spawn_tracker_thread(tx.clone());

    // open file
    let hashes: Vec<[u8; DIGEST_SIZE]> = METAINFO
        .info
        .pieces
        .chunks_exact(DIGEST_SIZE)
        .map(|x| x.try_into().unwrap())
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
            left: 5000, // TODO
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
                let peer_info = PeerInfo::new(data.peer, tx.clone());
                peers.insert(addr, peer_info);

                //peer_sender.send(PeerRequest::GetInfo)?;
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
