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
use log::{debug, error, info, trace, warn};
use rand::Rng;
use threads::Response;
use timer::{spawn_timer_thread, TimerRequest};
use tracker::{request, TrackerRequest};

use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process;
use std::time::{Duration, Instant};
use std::{collections::HashMap, net::TcpListener};

use std::io::Write;

use anyhow::{bail, Result};
use bitvec::prelude::*;
use crossbeam::channel::{self, Sender};

use crate::args::{ARGS, METAINFO};
use crate::file::{Block, BlockInfo};
use crate::peers::{spawn_peer_thread, Message, PeerRequest, PeerResponse};
use crate::timer::TimerInfo;
use crate::utils::RemoveValue;

const DIGEST_SIZE: usize = 20;

#[derive(Clone, Debug)]
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

    // statistics (and their distributions)
    pub uploaded: usize,
    pub downloaded: usize,

    // "recent" statistics
    pub uploaded_recently: usize,
    pub downloaded_recently: usize,
}

impl PeerInfo {
    // Consumes a TcpStream, creates a new peer thread
    fn new(peer: TcpStream, sender: Sender<Response>) -> Self {
        let piece_count = METAINFO.info.pieces.chunks_exact(DIGEST_SIZE).len();
        Self {
            sender: spawn_peer_thread(peer, sender),
            choked: false,
            interested: false,
            peer_choked: true,
            peer_interested: false,
            has: bitvec![u8, Msb0; 0; piece_count],
            uploaded: 0,
            downloaded: 0,
            uploaded_recently: 0,
            downloaded_recently: 0,
        }
    }
}

pub struct MainState {
    pub peers: HashMap<SocketAddr, PeerInfo>,
    pub file: DownloadFile,
    pub timer_sender: Sender<TimerRequest>,
    pub requested: HashMap<timer::Token, (file::BlockInfo, SocketAddr)>,
}

impl MainState {
    pub fn uploaded(&self) -> usize {
        self.peers.values().fold(0, |acc, p| acc + p.uploaded)
    }

    pub fn downloaded(&self) -> usize {
        self.peers.values().fold(0, |acc, p| acc + p.downloaded)
    }
}

fn broadcast_has(state: &mut MainState, piece: usize) {
    trace!("Sending Has for piece {:?}", piece);
    state.peers.retain(|&addr, peer_info| {
        // don't send to peer who already has this piece
        if let Some(idx) = peer_info.has.get(piece) {
            if *idx {
                return true;
            }
        }

        let msg = PeerRequest::SendMessage(Message::Have(piece as u32));
        if peer_info.sender.send(msg).is_err() {
            warn!(
                "Main: peer {:?} appears to have died. Removing from peer context map...",
                addr
            );
            return false;
        }
        true
    });
}

fn rescan_interest(
    my_has: &BitVec<u8, Msb0>,
    peer_info: &mut PeerInfo,
    addr: SocketAddr,
) -> Result<()> {
    let interested = peer_info.has.iter().zip(my_has).any(|(p, s)| *p && !*s);
    if interested != peer_info.interested {
        peer_info.interested = interested;

        // Tell the peer about this change
        let msg = PeerRequest::SendMessage(if interested {
            Message::Interested
        } else {
            Message::NotInterested
        });
        trace!(
            "Interest state for peer {:?} changed to {:?}",
            addr,
            interested
        );
        peer_info.sender.send(msg)?;
    }

    Ok(())
}

fn handle_peer_response(state: &mut MainState, resp: PeerResponse) -> Result<()> {
    let PeerResponse::MessageReceived(addr, msg) = resp else {
        warn!("handle_peer_response(): received unhandled response type");
        return Ok(());
    };

    let Some(peer_info) = state.peers.get_mut(&addr) else {
        bail!("Main thread has no context for peer {:?}", addr);
    };

    use peers::Message::*;
    match msg {
        Choke => {
            info!("Peer {:?} has choked us", addr);

            // remove all entries in requested with this peer
            //state.requested.retain(|&id, (_, p)| {
            //    if *p != addr {
            //        // cancel the timeout
            //        state
            //            .timer_sender
            //            .send(TimerRequest::Cancel(id))
            //            .expect("Failed to communicate with timer thread!");

            //        return false;
            //    }
            //    true
            //});

            peer_info.peer_choked = true;
        }
        Unchoke => {
            info!("Peer {:?} has unchoked us", addr);
            peer_info.peer_choked = false;
        }
        Interested => {
            info!("Peer {:?} is interested in us", addr);
            peer_info.peer_interested = true;
        }
        NotInterested => {
            peer_info.peer_interested = false;
        }
        Have(piece) => {
            let piece = piece as usize;
            if let Some(mut idx) = peer_info.has.get_mut(piece) {
                *idx = true;
            } else {
                warn!("Peer {:?} sent Have with invalid piece", addr);
            }

            // Update my interested status
            // baaaa this is really bad
            if !peer_info.interested {
                if let Some(idx) = state.file.bitvec().get(piece) {
                    if !*idx {
                        peer_info.interested = true;
                        let msg = PeerRequest::SendMessage(Message::Interested);
                        peer_info.sender.send(msg)?;
                    }
                }
            }
        }
        Bitfield(bytes) => {
            if bytes.len() == peer_info.has.as_raw_slice().len() {
                peer_info.has = BitVec::from_slice(&bytes);

                // Update my interested status
                rescan_interest(state.file.bitvec(), peer_info, addr)?;
            } else {
                warn!("Peer {:?} sent Bitfield with invalid length", addr);
            }
        }
        Piece(piece, offset, data) => {
            let block = Block::new(piece as usize, offset as usize, &data);

            // remove request from the queue
            if let Some(token) = state.requested.remove_value((block.info(), addr)) {
                // ask the timer thread to terminate this timeout
                state
                    .timer_sender
                    .send(TimerRequest::Cancel(token))
                    .expect("Main thread failed to communicate with timer thread!");

                // process the block
                let result = state.file.process_block(block);
                if let Ok(_) = result {
                    // keep statistics
                    peer_info.uploaded += data.len();
                    peer_info.uploaded_recently += data.len();

                    // Update my interested status
                    rescan_interest(state.file.bitvec(), peer_info, addr)?;
                } else if let Err(e) = result {
                    warn!("Failed to process piece from peer {:?}: {:?}", addr, e);
                }
            } else {
                let len = data.len();
                warn!("Peer {:?} send Piece we did not request\n ---> piece={piece}, offset={offset}, len={len}", addr);
            }

            // did we just finish processing the piece?
            if let Ok(true) = state.file.piece_is_complete(piece as usize) {
                // broadcast to every peer that we have this piece
                broadcast_has(state, piece as usize);
            }
        }
        Request(piece, offset, length) => {
            info!("I GOT A REQUEST");

            let block_info = BlockInfo {
                piece: piece as usize,
                range: (offset as usize)..(offset as usize + length as usize),
            };
            info!(" --> request info: {:?}", block_info);

            // ignore request if we're choking this peer
            if peer_info.choked {
                warn!("Warning: Peer {:?} made request while choked", addr);
            } else {
                let stuff = state.file.get_block(block_info);
                let Ok(data) = stuff else {
                    bail!("Peer {:?} made Request for piece we do not have", addr);
                };

                // keep statistics
                peer_info.downloaded += data.len();
                peer_info.downloaded_recently += data.len();

                // send a Piece response
                let msg = PeerRequest::SendMessage(Message::Piece(piece, offset, data));
                peer_info.sender.send(msg)?;
            }
        }
        Cancel(_, _, _) => (),

        // ignore keepalives for now (we do our own timeouts)
        Keepalive => (),
    };

    Ok(())
}

fn main() -> Result<()> {
    // set the logger
    env_logger::init();

    // we do a little arg parsing
    lazy_static::initialize(&ARGS);

    // this is how each thread will communicate back with main thread
    let (tx, rx) = channel::unbounded();

    let tracker_sender = tracker::spawn_tracker_thread(tx.clone());

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
        file: if ARGS.seed_existing {
            DownloadFile::new_seeding(
                METAINFO.info.name.clone(),
                &hashes,
                METAINFO.info.piece_length,
                METAINFO.info.length,
            )?
        } else {
            DownloadFile::new(
                METAINFO.info.name.clone(),
                &hashes,
                METAINFO.info.piece_length,
                METAINFO.info.length,
            )?
        },

        // timer thread to handle block timeouts and periodic game theory
        timer_sender: spawn_timer_thread(tx.clone()),

        // queue of outgoing requests we are awaiting
        requested: HashMap::new(),
    };

    // send initial starting request
    if !ARGS.skip_announce {
        let tracker_req = TrackerRequest {
            url: METAINFO.announce.clone(),
            request: request::Request {
                info_hash: METAINFO.info_hash(),
                peer_id: *PEER_ID,
                my_port: ARGS.port,
                uploaded: 0,
                downloaded: 0,
                left: state.file.left(),
                event: Some(request::Event::Started),
            },
        };
        tracker_sender
            .send(tracker_req)
            .expect("Failed to send request to tracker thread");
    }

    // Start listening
    let server = TcpListener::bind(("0.0.0.0", ARGS.port))?;
    connections::spawn_accept_thread(server, tx.clone());

    let tracker_timer_id: u64 = rand::thread_rng().gen();

    // Add single peer (if provided)
    if let Some(peer) = &ARGS.add_peer {
        let addr = peer.to_socket_addrs().unwrap().next().unwrap();
        connections::async_connect(tx.clone(), addr);
    }

    // Main loop
    for resp in rx.iter() {
        match resp {
            Response::Connection(data) => {
                debug!("{:?}", data.peer);

                let addr = data.peer.peer_addr()?;

                // Don't accept connection from peer we're connected to!
                if state.peers.contains_key(&addr) {
                    continue;
                }

                let peer_info = PeerInfo::new(data.peer, tx.clone());
                let peer_info = state.peers.entry(addr).or_insert(peer_info);

                // Send the new peer our current bitmap
                let bytes = state.file.bitfield().to_vec();
                let msg = PeerRequest::SendMessage(Message::Bitfield(bytes));
                peer_info.sender.send(msg)?;

                // We don't have any choke/unchoke logic for now;
                // let's just be totally benevolent.
                if let Err(e) = peer_info
                    .sender
                    .send(PeerRequest::SendMessage(peers::Message::Unchoke))
                {
                    error!("Failed to send unchoke to peer at {:?}: {:?}", addr, e);
                }
            }
            Response::Peer(data) => {
                if let Err(e) = handle_peer_response(&mut state, data) {
                    error!("Failed to handle peer response: {:?}", e);
                }
            }
            Response::Tracker(Ok(data)) => {
                debug!("main thread received response {:#?}", data);

                // Create a timer for the next request
                let timer_req = TimerRequest::Timer(TimerInfo {
                    //timer_len: Duration::from_secs(data.interval as u64),
                    timer_len: Duration::from_secs(20),
                    id: tracker_timer_id,
                    repeat: false,
                });
                state
                    .timer_sender
                    .send(timer_req)
                    .expect("Main thread failed to communicate with timer thread!");

                // keep top n peers
                let mut n = ARGS.max_connections / 2;
                let mut s: Vec<SocketAddr> = state.peers.keys().map(|x| *x).collect();
                s.sort_unstable_by(|&addr1, &addr2| {
                    let peer_info1 = state.peers.get(&addr1).unwrap();
                    let peer_info2 = state.peers.get(&addr2).unwrap();

                    peer_info2.uploaded_recently.cmp(&peer_info1.uploaded)
                });
                if n > s.len() {
                    n = s.len();
                }
                for addr in s.drain(n..) {
                    state.peers.remove(&addr);
                }

                // reset uploaded/downloaded recently
                for (_, peer_info) in state.peers.iter_mut() {
                    peer_info.uploaded_recently = 0;
                    peer_info.downloaded_recently = 0;
                }

                let mut peer_iter = data.peers.iter();
                while let Some(p) = peer_iter.next() {
                    if state.peers.len() >= ARGS.max_connections {
                        break;
                    }

                    let addr = (&p.ip[..], p.port)
                        .to_socket_addrs()
                        .unwrap()
                        .next()
                        .unwrap();

                    // don't connect to the same peer twice
                    if state.peers.contains_key(&addr) {
                        continue;
                    }

                    connections::async_connect(tx.clone(), addr);
                }
            }
            Response::Tracker(Err(e)) => {
                error!("tracker failed with error: {:?}", e);
            }
            Response::Timer(data) if { data.id == tracker_timer_id } => {
                // send periodic tracker request
                let tracker_req = TrackerRequest {
                    url: METAINFO.announce.clone(),
                    request: request::Request {
                        info_hash: METAINFO.info_hash(),
                        peer_id: *PEER_ID,
                        my_port: ARGS.port,
                        uploaded: state.uploaded(),
                        downloaded: state.downloaded(),
                        left: state.file.left(),
                        event: None,
                    },
                };
                tracker_sender
                    .send(tracker_req)
                    .expect("Failed to send request to tracker thread");
            }
            Response::Timer(data) => {
                if let Some(&(_, addr)) = state.requested.get(&data.id) {
                    debug!("Timeout occurred for peer {:?}", addr);

                    // remove from requested queue
                    state.requested.remove(&data.id);

                    // actually remove the peer
                    state.peers.remove(&addr);
                } else {
                    warn!("Weird race condition thing?");
                }
            }
        }

        if state.file.is_complete() && (!ARGS.seed && !ARGS.seed_existing) {
            info!("File download complete!");

            // Tell the tracker we're done
            let msg = TrackerRequest {
                url: METAINFO.announce.clone(),
                request: request::Request {
                    info_hash: METAINFO.info_hash(),
                    peer_id: *PEER_ID,
                    my_port: ARGS.port,
                    uploaded: state.uploaded(),
                    downloaded: state.downloaded(),
                    left: 0,
                    event: Some(request::Event::Completed),
                },
            };
            tracker_sender
                .send(msg)
                .expect("Failed to send request to tracker thread");

            process::exit(0);
        }

        // after handling event, refill pipelines
        let requests = strategy::pick_blocks(&state);
        for (block, addr) in requests {
            let Some(peer_info) = state.peers.get(&addr) else {
                continue;
            };

            // Try to send the request to the peer
            let msg = PeerRequest::SendMessage(Message::Request(
                block.piece as u32,
                block.range.start as u32,
                (block.range.end - block.range.start) as u32,
            ));
            if peer_info.sender.send(msg).is_err() {
                warn!(
                    "Main: peer {:?} appears to have died. Removing from peer context map...",
                    addr
                );
                state.peers.remove(&addr);
            }

            // Associate a timer with the request
            let id: u64 = rand::thread_rng().gen();
            let timer_req = TimerRequest::Timer(TimerInfo {
                timer_len: Duration::from_secs(ARGS.request_timeout),
                id,
                repeat: false,
            });
            state
                .timer_sender
                .send(timer_req)
                .expect("Main thread failed to communicate with timer thread!");

            // Add to the requests queue
            state.requested.insert(id, (block, addr));
        }
    }

    debug!("Exited from main loop");

    Ok(())
}
