use anyhow::{anyhow, Result};
use crossbeam::channel::{self, Select, Sender};
use std::{
    io::{self, BufReader, BufWriter, Read, Write},
    net::{SocketAddr, TcpStream},
    thread,
    time::Duration,
};

use crate::args::{METAINFO, PEER_ID};
use crate::threads::Response;
use crate::tracker::response::Peer;

const PROTO_IDENTIFIER: &str = "BitTorrent protocol";

const TCP_READ_TIMEOUT: Duration = Duration::from_secs(5);

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
pub enum Message {
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
    SendMessage(Message),
}

#[derive(Debug)]
pub enum PeerResponse {
    MessageReceived(SocketAddr, Message),
    Heartbeat,
    Death,
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

// GIANT TODO: handle thread deaths!

pub fn spawn_peer_thread(peer: TcpStream, sender: Sender<Response>) -> Sender<PeerRequest> {
    let (tx, rx) = channel::unbounded();
    let addr = peer.peer_addr().expect("TcpStream not connected to peer!");

    thread::spawn(move || {
        // set timeout for tcp stream
        peer.set_read_timeout(Some(TCP_READ_TIMEOUT))
            .expect("Failed to set read timeout on TcpStream");

        let mut writer = BufWriter::new(peer.try_clone().expect("Failed to clone TcpStream")); // TODO: what if this fails? should tell main thread!
        let mut reader = BufReader::new(peer.try_clone().expect("Failed to clone TcpStream"));

        // do the handshake
        if do_handshake(&mut reader, &mut writer).is_err() {
            eprintln!("Failed to perform handshake!");
            return;
        }

        // create receiving thread
        let (s, r) = channel::unbounded();
        thread::spawn(move || loop {
            // TODO: send heartbeat messages
            match Message::recv(&mut reader) {
                Ok(msg) => {
                    // send message back to main thread
                    if s.send(PeerResponse::MessageReceived(addr, msg)).is_err() {
                        eprintln!("Received thread failed to send response to peer thread");
                        return;
                    }
                }
                Err(e) => {
                    match e.downcast::<io::Error>() {
                        Ok(t) => {
                            // timeout; just continue
                            if t.kind() != io::ErrorKind::WouldBlock {
                                eprintln!("Received thread encountered I/O error: {}", t);
                                return;
                            }
                        }
                        Err(e) => {
                            // unrecoverable error
                            println!("Receiver thread encountered unknown error: {}", e);
                            return;
                        }
                    }

                    // send heartbeat to peer thread
                    s.send(PeerResponse::Heartbeat)
                        .expect("Receiver thread failed to send heartbeat to peer thread");
                }
            }
        });

        let mut sel = Select::new();
        let main_thread_oper = sel.recv(&rx);
        let recv_thread_oper = sel.recv(&r);

        loop {
            let oper = sel.select();
            match oper.index() {
                i if i == main_thread_oper => {
                    let req = oper
                        .recv(&rx)
                        .expect("Peer thread failed to read from main thread channel");

                    use PeerRequest::*;
                    match req {
                        SendMessage(msg) => {
                            // send the message to the remote
                            if let Err(e) = msg.send(&mut writer) {
                                println!("Peer thread failed to send message to remote: {}", e);
                                return;
                            }
                        }
                    }
                }
                i if i == recv_thread_oper => {
                    // TODO: for now, we only forward Message responses. Should we forward heartbeats/deaths?
                    let Ok(resp) = oper.recv(&r) else {
                        eprintln!("Peer thread failed to read from receiver thread channel");
                        return;
                    };

                    // forward the message back to the main thread
                    if let PeerResponse::MessageReceived(_, _) = resp {
                        sender
                            .send(Response::Peer(resp))
                            .expect("Peer thread failed to write to channel");
                    }
                }
                _ => unreachable!(),
            }
        }
    });

    tx
}

#[cfg(test)]
mod tests {

    use std::{
        io::{BufReader, BufWriter},
        sync::mpsc,
        thread,
    };

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
            Bitfield(vec![
                102, 117, 99, 107, 32, 98, 114, 97, 109, 32, 99, 111, 104, 101, 110,
            ]),
            Request(123, 456, 789),
            Piece(5810134, 215970, vec![204, 10, 0]),
            Cancel(789, 456, 123),
        ];
        let num_messages = test_messages.len();

        let (read, write) = pipe::pipe();
        let mut reader = BufReader::new(read);
        let mut writer = BufWriter::new(write);

        let (tx, rx) = mpsc::channel();

        let handle = thread::spawn(move || {
            for _ in 0..num_messages {
                // try to receive message
                let msg = Message::recv(&mut reader).unwrap();
                tx.send(msg).unwrap();
            }
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
