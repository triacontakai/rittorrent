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

#[derive(Debug, PartialEq)]
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
    fn send(&self, writer: &mut BufWriter<impl Write>) -> Result<()> {
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

    fn recv(reader: &mut BufReader<impl Read>) -> Result<Self> {
        // Receive length first
        let mut length_buf = [0u8; 4];
        reader.read_exact(&mut length_buf)?;

        let length: usize = u32::from_be_bytes(length_buf) as usize;

        // empty message is a keepalive
        if length == 0 {
            return Ok(Self::Keepalive);
        }

        // Then read the first (type) byte
        let mut type_buf = [0u8; 1];
        reader.read_exact(&mut type_buf)?;
        let message_type = type_buf[0];

        // Next, read the rest of the message
        let mut buf: Vec<u8> = vec![0; length - 1];
        reader.read_exact(&mut buf)?;

        //let Some(&message_type) = buf.get(0) else {
        //    // if we read nothing, this is a keepalive
        //    return Ok(Self::Keepalive);
        //};

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

            if buf.len() == 4 {
                let idx = u32::from_be_bytes(buf[0..4].try_into().unwrap());

                Ok(Self::Have(idx))
            } else {
                Err(anyhow!("Received invalid Have message"))
            }

        } else if message_type == MessageType::Bitfield as u8 {
            Ok(Self::Bitfield(buf))
        } else if message_type == MessageType::Request as u8 {

            if buf.len() == 12 {
                let idx = u32::from_be_bytes(buf[0..4].try_into().unwrap());
                let begin = u32::from_be_bytes(buf[4..8].try_into().unwrap());
                let len = u32::from_be_bytes(buf[8..12].try_into().unwrap());

                Ok(Self::Request(idx, begin, len))
            } else {
                Err(anyhow!("Received invalid Request message"))
            }

        } else if message_type == MessageType::Piece as u8 {

            if buf.len() >= 8 {
                let idx = u32::from_be_bytes(buf[0..4].try_into().unwrap());
                let begin = u32::from_be_bytes(buf[4..8].try_into().unwrap());
                let piece: Vec<u8> = buf[8..].to_vec();

                Ok(Self::Piece(idx, begin, piece))
            } else {
                Err(anyhow!("Received invalid Piece message"))
            }
        
        } else if message_type == MessageType::Cancel as u8 {

            if buf.len() == 12 {
                let idx = u32::from_be_bytes(buf[0..4].try_into().unwrap());
                let begin = u32::from_be_bytes(buf[4..8].try_into().unwrap());
                let len = u32::from_be_bytes(buf[8..12].try_into().unwrap());

                Ok(Self::Cancel(idx, begin, len))
            } else {
                Err(anyhow!("Received invalid Cancel message"))
            }

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
    reader: &mut BufReader<impl Read>,
    writer: &mut BufWriter<impl Write>,
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

#[cfg(test)]
mod tests {

    use std::{io::{BufReader, BufWriter}, thread, sync::mpsc};

    use pipe;

    use super::Message;

    use Message::*;

    #[test]
    fn peer_msg_test() {
        let test_messages: [Message; 10] = [
            Keepalive,
            Choke,
            Unchoke,
            Interested,
            NotInterested,
            Have(12345678),
            Bitfield(vec![102, 117, 99, 107, 32, 98, 114, 97, 109, 32, 99, 111, 104, 101, 110]),
            Request(123, 456, 789),
            Piece(5810134, 215970, vec![204, 10, 0]),
            Cancel(789, 456, 123),
        ];

        let (read, write) = pipe::pipe();
        let mut reader = BufReader::new(read);
        let mut writer = BufWriter::new(write);

        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            // try to receive message
            let msg = Message::recv(&mut reader).unwrap();
            tx.send(msg).unwrap();
        });

        for msg in test_messages {
            // send the message
            msg.send(&mut writer).unwrap();

            // what did the second thread receive?
            let received = rx.recv().unwrap();
            assert_eq!(msg, received);
        }

        handle.join().unwrap();
    }

}
