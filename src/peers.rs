use anyhow::{anyhow, Result};
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

#[derive(Copy, Clone)]
enum MessageType {
    Choke = 0,
    Unchoke = 1,
    Interested = 2,
    NotInterested = 3,
    Have = 4,
    Bitfield = 5,
    Request = 6,
    Piece = 7,
    Cancel = 8,
}

enum Message {
    Keepalive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request(u32, u32, u32),
    Piece(u32, u32, Vec<u8>),
    Cancel(u32, u32, u32),
}

#[derive(Debug)]
pub enum PeerRequest {
    GetInfo,
}

#[derive(Debug)]
pub enum PeerResponse {
    PeerInfo(PeerInfo),
}

#[derive(Copy, Clone, Debug)]
pub struct PeerInfo {
    // basic info
    addr: SocketAddr,

    // protocol options
    choked: bool,
    interested: bool,
}

impl Message {
    fn send(&self, writer: &mut BufWriter<TcpStream>) -> Result<()> {
        let mut buf: Vec<u8> = Vec::new();

        use Message::*;
        match self {
            Keepalive => (),
            Choke => {
                buf.extend(&[MessageType::Choke as u8]);
            }
            Unchoke => {
                buf.extend(&[MessageType::Unchoke as u8]);
            }
            Interested => {
                buf.extend(&[MessageType::Interested as u8]);
            }
            NotInterested => {
                buf.extend(&[MessageType::NotInterested as u8]);
            }
            Have(idx) => {
                buf.extend(&[MessageType::Have as u8]);
                buf.extend(&(*idx as u32).to_be_bytes());
            }
            Bitfield(bytes) => {
                buf.extend(&[MessageType::Bitfield as u8]);
                buf.extend(bytes);
            }
            Request(idx, begin, len) => {
                buf.extend(&[MessageType::Request as u8]);
                buf.extend(&(*idx as u32).to_be_bytes());
                buf.extend(&(*begin as u32).to_be_bytes());
                buf.extend(&(*len as u32).to_be_bytes());
            }
            Piece(idx, begin, piece) => {
                buf.extend(&[MessageType::Piece as u8]);
                buf.extend(&(*idx as u32).to_be_bytes());
                buf.extend(&(*begin as u32).to_be_bytes());
                buf.extend(piece);
            }
            Cancel(idx, begin, len) => {
                buf.extend(&[MessageType::Cancel as u8]);
                buf.extend(&(*idx as u32).to_be_bytes());
                buf.extend(&(*begin as u32).to_be_bytes());
                buf.extend(&(*len as u32).to_be_bytes());
            }
        }

        // actually send the message
        writer.write_all(&(buf.len() as u32).to_be_bytes())?;
        writer.write_all(&buf)?;
        writer.flush()?;

        Ok(())
    }

    fn recv(reader: &mut BufReader<TcpStream>) -> Result<Self> {
        // Receive length first
        let mut length_buf = [0u8; 1];
        reader.read_exact(&mut length_buf)?;

        // Next, read the rest of the message
        let mut buf: Vec<u8> = vec![0; length_buf[0] as usize];
        reader.read_exact(&mut buf)?;

        let Some(&message_type) = buf.get(0) else {
            // if we read nothing, this is a keepalive
            return Ok(Self::Keepalive);
        };

        // Try to parse the message
        if message_type == MessageType::Choke as u8 {
            Ok(Self::Choke)
        } else if message_type == MessageType::Unchoke as u8 {
            Ok(Self::Unchoke)
        } else if message_type == MessageType::Interested as u8 {
            Ok(Self::Interested)
        } else if message_type == MessageType::NotInterested as u8 {
            Ok(Self::NotInterested)
        } else if message_type == MessageType::Have as u8 {

            let Ok(bytes): Result<[u8; 4], _> = buf.try_into() else {
                return Err(anyhow!("Received invalid Have message"));
            };

            Ok(Self::Have(u32::from_be_bytes(bytes)))

        } else if message_type == MessageType::Bitfield as u8 {
            Ok(Self::Bitfield(buf))
            // TODO: finish this
        } else {
            Err(anyhow!("Received unsupported message type"))
        }
    }
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
    writer.write_all(&[PROTO_IDENTIFIER.len() as u8])?; // pstrlen
    writer.write_all(PROTO_IDENTIFIER.as_bytes())?; // pstr
    writer.write_all(&[0u8; 8])?; // reserved
    writer.write_all(&METAINFO.info_hash())?; // info_hash
    writer.write_all(&*PEER_ID)?; // peer_id
    writer.flush()?;

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
