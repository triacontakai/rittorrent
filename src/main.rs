mod args;
mod connections;
mod file;
mod http;
mod peers;
mod strategy;
mod threads;
mod timer;
mod torrent;
mod tracker;
mod utils;

use args::PEER_ID;
use file::DownloadFile;
use rand::Rng;
use threads::Response;
use timer::{spawn_timer_thread, TimerRequest};
use tracker::{request, TrackerRequest};

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::ops::Range;
use std::time::Duration;
use std::{collections::HashMap, net::TcpListener};

use anyhow::{bail, Result};
use bitvec::prelude::*;
use crossbeam::channel::{self, Sender};

use crate::args::{ARGS, METAINFO};
use crate::file::{Block, BlockInfo};
use crate::peers::{spawn_peer_thread, Message, PeerRequest, PeerResponse};
use crate::utils::RemoveValue;

const DIGEST_SIZE: usize = 20;

const CONNECTION_TIMEOUT: Duration = Duration::from_secs(1);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10); // TODO: is this a good value?

#[derive(Debug)]
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
        let piece_count = METAINFO.info.pieces.chunks_exact(DIGEST_SIZE).len();
        Self {
            sender: spawn_peer_thread(peer, sender),
            choked: true,
            interested: false,
            peer_choked: true,
            peer_interested: false,
            has: bitvec![u8, Msb0; 0; piece_count],
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
    let PeerResponse::MessageReceived(addr, msg) = resp else {
        println!("handle_peer_response(): received unhandled response type");
        return Ok(());
    };

    let peer_info = state
        .peers
        .get_mut(&addr)
        .expect("Main thread has no context for peer");

    use peers::Message::*;
    match msg {
        Choke => {
            // when we receive choke we should remove all requests from "requested" queue for this peer
            peer_info.peer_choked = true;
        }
        Unchoke => {
            println!("Peer {:?} has unchoked us", addr);
            peer_info.peer_choked = false;
        }
        Interested => {
            peer_info.peer_interested = true;
        }
        NotInterested => {
            peer_info.peer_interested = false;
        }
        Have(piece) => {
            if let Some(mut idx) = peer_info.has.get_mut(piece as usize) {
                *idx = true;
            } else {
                eprintln!("Peer {:?} sent Have with invalid piece", addr);
            }
        }
        Bitfield(bytes) => {
            if bytes.len() == peer_info.has.as_raw_slice().len() {
                peer_info.has = BitVec::from_slice(&bytes);
            } else {
                eprintln!("Peer {:?} sent Bitfield with invalid length", addr);
                eprintln!(
                    " --> sent len={:?}, expected len={:?}",
                    bytes.len(),
                    peer_info.has.as_raw_slice().len()
                );
            }
        }
        Piece(piece, offset, data) => {
            let block = Block::new(piece as usize, offset as usize, &data);

            println!("YOOO OMG OMG I GOT A PIECE YOO NO WAY FR???");
            println!(" --> piece info: {:?}", block.info());

            // remove request from the queue
            if state.requested.remove_value((block.info(), addr)) {
                // process the block
                if let Err(e) = state.file.process_block(block) {
                    eprintln!("Failed to process piece from peer {:?}: {:?}", addr, e);
                }
            } else {
                eprintln!("Peer {:?} send Piece we did not request", addr);
            }
        }

        // ignore requests for now because andrei's "totally finished" file subsystem
        // doesn't have a way to read a block from the file....
        Request(_, _, _) => (),
        Cancel(_, _, _) => (),

        // ignore keepalives for now (we do our own timeouts)
        Keepalive => (),
    };

    //println!(
    //    "<--- Current bitfield for {:?} is {:?} --->",
    //    addr, peer_info.has
    //);

    Ok(())
}

fn main() -> Result<()> {
    // we do a little arg parsing
    lazy_static::initialize(&ARGS);

    // this is how each thread will communicate back with main thread
    let (tx, rx) = channel::unbounded();

    let tracker_sender = tracker::spawn_tracker_thread(tx.clone());

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
    let tracker_resp = match rx
        .recv()
        .expect("Failed to receive tracker response from tracker thread")
    {
        Response::Tracker(Ok(r)) => r,
        Response::Tracker(Err(e)) => bail!("Error receiving response from tracker: {:?}", e),
        _ => unreachable!(),
    };

    //println!("Tracker response: {:#?}", tracker_resp);

    // create main thread state
    let hashes: Vec<[u8; DIGEST_SIZE]> = METAINFO
        .info
        .pieces
        .chunks_exact(DIGEST_SIZE)
        .map(|x| x.try_into().unwrap())
        .collect();
    let mut state = MainState {
        // Map from SocketAddr->PeerInfo. Also serves as "list" of peers
        peers: HashMap::new(),

        // File I/O subsystem context
        file: DownloadFile::new(
            METAINFO.info.name.clone(),
            &hashes,
            METAINFO.info.piece_length,
            METAINFO.info.length,
        )?,

        // timer thread to handle block timeouts and periodic game theory
        timer_sender: spawn_timer_thread(tx.clone()),

        // queue of outgoing requests we are awaiting
        requested: HashMap::new(),
    };

    // Connect to some initial peers
    let mut peer_iter = tracker_resp.peers.iter();
    while let Some(p) = peer_iter.next() {
        if state.peers.len() >= ARGS.max_connections {
            break;
        }

        let addr = (&p.ip[..], p.port)
            .to_socket_addrs()
            .unwrap()
            .next()
            .unwrap();
        eprintln!("Connecting to peer {:?}", addr);
        let Ok(stream) = TcpStream::connect_timeout(&addr, CONNECTION_TIMEOUT) else {
            println!(" --> Peer timed out");
            continue;
        };
        eprintln!(" --> Connected");
        state.peers.insert(addr, PeerInfo::new(stream, tx.clone()));

        // send the peer our bitfield
        let peer_info = state.peers.get(&addr).unwrap();
        let bytes = state.file.bitfield().to_vec();
        let msg = PeerRequest::SendMessage(Message::Bitfield(bytes));
        peer_info
            .sender
            .send(msg)
            .expect("Main failed to communicate with newly-created peer thread");
    }

    // Start listening
    let server = TcpListener::bind("0.0.0.0:5001")?;
    connections::spawn_accept_thread(server, tx.clone());

    // Main loop
    for resp in rx.iter() {
        match resp {
            Response::Connection(data) => {
                println!("{:?}", data.peer);

                let addr = data.peer.peer_addr()?;
                let peer_info = PeerInfo::new(data.peer, tx.clone());
                let peer_info = state.peers.entry(addr).or_insert(peer_info);

                peer_info
                    .sender
                    .send(PeerRequest::SendMessage(peers::Message::Choke))?; // TODO; question mark?
            }
            Response::Peer(data) => {
                handle_peer_response(&mut state, data)?;
            }
            Response::Tracker(data) => {
                println!("main thread received response {:#?}", data);
            }
            Response::Timer(data) => {
                println!("Timed out. Token={}", data.id);

                // remove from requested queue
                state.requested.remove(&data.id);
            }
        }

        // TODO: move this into a helper function
        // after handling event, refill pipelines
        let requests = strategy::pick_blocks(&state);
        //println!("SHOULD MAKE REQUESTS FOR: {:#?}", stuff);
        for (block, addr) in requests {
            let peer_info = state.peers.get(&addr).unwrap();

            // Try to send the request to the peer
            let msg = PeerRequest::SendMessage(Message::Request(
                block.piece as u32,
                block.range.start as u32,
                (block.range.end - block.range.start) as u32,
            ));
            println!("Peer {:?}: sent Request: {:?}", addr, msg);
            if peer_info.sender.send(msg).is_err() {
                println!(
                    "Main: peer {:?} appears to have died. Removing from peer context map...",
                    addr
                );
                state.peers.remove(&addr);
            }

            // Associate a timer with the request
            let id: u64 = rand::thread_rng().gen();
            let timer_req = TimerRequest {
                timer_len: REQUEST_TIMEOUT,
                id,
                repeat: false,
            };
            state
                .timer_sender
                .send(timer_req)
                .expect("Main thread failed to communicate with timer thread!");

            // Add to the requests queue
            state.requested.insert(id, (block, addr));
        }
    }

    Ok(())
}
