use anyhow::Result;
use std::{
    io::{BufReader, BufWriter, Read, Write},
    net::{SocketAddr, TcpStream},
    sync::mpsc::{self, Sender},
    thread,
};

use crate::args::{METAINFO, PEER_ID};
use crate::threads::Response;
use crate::tracker::response::Peer;

const PROTO_IDENTIFIER: &str = "BitTorrent protocol";

#[derive(Debug)]
pub enum PeerRequest {
    GetInfo,
}

#[derive(Copy, Clone, Debug)]
pub struct PeerInfo {
    // basic info
    addr: SocketAddr,

    // protocol options
    choked: bool,
    interested: bool,
}

impl PeerInfo {
    fn new(addr: SocketAddr) -> Self {
        Self {
            addr: addr,
            choked: true,
            interested: false,
        }
    }
}

#[derive(Debug)]
pub enum PeerResponse {
    PeerInfo(PeerInfo),
}

// lol
pub fn connect_to_peer(peer: Peer) -> Result<TcpStream> {
    Ok(TcpStream::connect((peer.ip, peer.port))?)
}

fn do_handshake(
    reader: &mut BufReader<TcpStream>,
    writer: &mut BufWriter<TcpStream>,
) -> Result<()> {
    const HEADER_LEN: usize = 49 + PROTO_IDENTIFIER.len();

    // First, let's send our end of the handshake
    writer.write(&[PROTO_IDENTIFIER.len() as u8])?; // pstrlen
    writer.write(PROTO_IDENTIFIER.as_bytes())?; // pstr
    writer.write(&[0u8; 8])?; // reserved
    writer.write(&METAINFO.info_hash())?; // info_hash
    writer.write(&*PEER_ID)?; // peer_id
    writer.flush();

    // Next, let's receive the other end of the handshake
    let mut buf = [0u8; HEADER_LEN];
    reader.read_exact(&mut buf)?;

    // TODO: some sanity checking, possibly?

    Ok(())
}

pub fn spawn_peer_thread(peer: TcpStream, sender: Sender<Response>) -> Sender<PeerRequest> {
    let (tx, rx) = mpsc::channel();
    let peer_addr = peer.peer_addr().expect("TcpStream not connected to peer");

    thread::spawn(move || {
        // initially construct peer info
        let info = PeerInfo::new(peer_addr);

        let mut reader = BufReader::new(peer.try_clone().expect("Failed to clone TcpStream"));
        let mut writer = BufWriter::new(peer.try_clone().expect("Failed to clone TcpStream"));

        // do the handshake
        do_handshake(&mut reader, &mut writer).expect("Failed to perform handshake");
        println!("Performed handshake!");

        for req in rx {
            println!("Received request: {:#?}", req);

            use PeerRequest::*;
            match req {
                GetInfo => {
                    sender
                        .send(Response::Peer(PeerResponse::PeerInfo(info)))
                        .expect("Peer thread could not respond to request");
                }
            }
        }
    });

    tx
}
