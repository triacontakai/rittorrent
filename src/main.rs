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
use timer::{spawn_timer_thread, TimerRequest};
use tracker::{request, TrackerRequest};

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::ops::Range;
use std::time::Duration;
use std::{collections::HashMap, net::TcpListener, sync::mpsc};

use anyhow::{bail, Result};
use bitvec::prelude::*;
use crossbeam::channel::{self, Receiver, Sender};

use crate::args::{ARGS, METAINFO};
use crate::peers::{spawn_peer_thread, PeerRequest, PeerResponse};

const DIGEST_SIZE: usize = 20;

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(1);

pub struct PeerInfo {
    // channel to send to this peer
    pub sender: Sender<PeerRequest>,

    // basic state
    pub choked: bool,
    pub interested: bool,
    pub peer_choked: bool,
    pub peer_interested: bool,

    // which pieces does this peer have?
    pub has: BitVec<u8, Msb0>,
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

pub struct MainState {
    pub peers: HashMap<SocketAddr, PeerInfo>,
    pub file: DownloadFile,
    pub timer_sender: Sender<TimerRequest>,
    pub requested: HashMap<timer::Token, (file::BlockInfo, SocketAddr)>,
}

fn handle_peer_response(state: &mut MainState, resp: PeerResponse) -> Result<()> {
    Ok(())
}

fn main() -> Result<()> {
    // we do a little arg parsing
    lazy_static::initialize(&ARGS);

    // map of addresses to peer structs
    let mut peers: HashMap<SocketAddr, PeerInfo> = HashMap::new();

    // this is how each thread will communicate back with main thread
    let (tx, rx) = channel::unbounded();

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

    // get list of peers from tracker
    // since tracker is currently the only thread, we can just recv here
    let Ok(Response::Tracker(Ok(tracker_resp))) = rx.recv() else {
        bail!("failed to receive tracker response");
    };

    println!("Tracker response: {:#?}", tracker_resp);

    let mut peer_iter = tracker_resp.peers.iter();
    while let Some(p) = peer_iter.next() {
        if peers.len() >= ARGS.max_connections {
            break;
        }

        let addr = (&p.ip[..], p.port)
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();
        eprintln!("Connecting to peer {:?}", addr);
        let Ok(stream) = TcpStream::connect_timeout(&addr, CONNECTION_TIMEOUT) else {
            continue;
        };
        eprintln!("Connected to peer");
        peers.insert(
            stream.peer_addr().unwrap(),
            PeerInfo::new(stream, tx.clone()),
        );
    }

    // timer thread to handle block timeouts and periodic game theory
    let timer_sender = spawn_timer_thread(tx.clone());

    let server = TcpListener::bind("0.0.0.0:5001")?;
    connections::spawn_accept_thread(server, tx.clone());

    // Main loop
    for resp in rx.iter() {
        match resp {
            Response::Connection(data) => {
                println!("{:?}", data.peer);

                let addr = data.peer.peer_addr()?;
                let peer_info = PeerInfo::new(data.peer, tx.clone());
                let peer_info = peers.entry(addr).or_insert(peer_info);

                peer_info
                    .sender
                    .send(PeerRequest::SendMessage(peers::Message::Choke))?; // TODO; question mark?
            }
            Response::Peer(data) => {
                println!("received response {:#?}", data);

                use PeerResponse::*;
                match data {
                    MessageReceived(_) => todo!(),
                    _ => {}
                }
            }
            Response::Tracker(data) => {
                println!("main thread received response {:#?}", data);
            }
            Response::Timer(_) => unimplemented!(),
        }
    }

    Ok(())
}
