pub mod request {
    #[derive(Debug)]
    pub enum Event {
        Started,
        Completed,
        Stopped,
    }

    #[derive(Debug)]
    pub struct Request {
        pub info_hash: [u8; 20],
        pub peer_id: [u8; 20],
        pub my_port: u16,
        pub uploaded: usize,
        pub downloaded: usize,
        pub left: usize,
        pub event: Option<Event>,
    }
}

pub mod response {
    use std::borrow::Cow;
    use std::net::Ipv4Addr;

    use bendy::value::Value;
    use serde::Deserializer;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq)]
    pub struct Peer {
        //#[serde(rename = "peer id", with = "serde_bytes")]
        //pub peer_id: Vec<u8>,
        pub ip: String,

        pub port: u16,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    pub struct Response {
        #[serde(default)]
        pub interval: u64,

        #[serde(default)]
        pub peers: Vec<Peer>,

        #[serde(rename = "failure reason", default)]
        pub(super) failure_reason: String,
    }

    fn deserialize_peers<'de, D>(deserializer: D) -> Result<Vec<Peer>, D::Error>
    where
        D: Deserializer<'de>
    {
        let mut peers = Vec::new();

        match Value::deserialize(deserializer)? {
            Value::Bytes(bytes) => {
                const IP_SIZE: usize = 4;
                const PORT_SIZE: usize = 2;
                const ENTRY_SIZE: usize = IP_SIZE + PORT_SIZE;

                for chunk in bytes.chunks_exact(6) {
                    let ip = Ipv4Addr::from(u32::from_be_bytes(chunk[0..4].try_into().unwrap())).to_string();
                    let port = u16::from_be_bytes(chunk[4..6].try_into().unwrap());

                    peers.push(Peer {
                        ip,
                        port,
                    });
                }
            }
            Value::List(list) => {
                for val in list {
                    let Value::Dict(map) = val else {
                        return Err(serde::de::Error::custom("peers list entry was not a Dict"))
                    };

                    let Some(Value::Bytes(ip)) = map.get(&Cow::Borrowed(&b"ip"[..])) else {
                        return Err(serde::de::Error::custom("peers list entry does not contain key 'ip'"))
                    };

                    let Some(&Value::Integer(port)) = map.get(&Cow::Borrowed(&b"port"[..])) else {
                        return Err(serde::de::Error::custom("peers list entry does not contain key 'port'"))
                    };

                    let ip = ip.clone();
                    let ip = String::from_utf8(ip.into_owned()).map_err(serde::de::Error::custom)?;

                    peers.push(Peer {
                        ip,
                        port: port.try_into().map_err(serde::de::Error::custom)?,
                    });
                }
            }
            _ => return Err(serde::de::Error::custom("peers entry was not Bytes or List")),
        }

        Ok(peers)
    }

    impl std::fmt::Debug for Peer {
        // don't print peer_id since it's annoying
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Peer")
                .field("ip", &self.ip)
                .field("port", &self.port)
                .finish()
        }
    }
}

use std::thread;

use anyhow::{anyhow, Result};
use bendy::serde::from_bytes;
use crossbeam::channel::{self, Sender};

use request::Request;
use response::Response;

use crate::args::{ARGS, METAINFO, PEER_ID};
use crate::http::http_get;
use crate::threads;

impl Request {
    pub fn send(&self, url: &str) -> Result<Response> {
        // Try to send the HTTP request
        use request::Event::*;
        let port = self.my_port.to_string();
        let uploaded = self.uploaded.to_string();
        let downloaded = self.downloaded.to_string();
        let left = self.left.to_string();
        let query: [(&str, &[u8]); 7] = [
            ("info_hash", &self.info_hash),
            ("peer_id", &self.peer_id),
            ("port", port.as_bytes()),
            ("uploaded", uploaded.as_bytes()),
            ("downloaded", downloaded.as_bytes()),
            ("left", left.as_bytes()),
            (
                "event",
                match self.event {
                    Some(Started) => "started".as_bytes(),
                    Some(Completed) => "completed".as_bytes(),
                    Some(Stopped) => "stopped".as_bytes(),
                    None => "empty".as_bytes(),
                },
            ),
        ];

        let http_response = http_get(url, &query)?;
        let tracker_response = from_bytes::<Response>(&http_response.content)?;

        if tracker_response.interval == 0 {
            Err(anyhow!(tracker_response.failure_reason))
        } else {
            Ok(tracker_response)
        }
    }
}

#[derive(Debug)]
pub struct TrackerRequest {
    pub url: String,
    pub request: Request,
}

pub fn spawn_tracker_thread(sender: Sender<threads::Response>) -> Sender<TrackerRequest> {
    let (tx, rx) = channel::unbounded::<TrackerRequest>();

    thread::spawn(move || {
        // main loop for tracker-interaction thread
        for req in rx {
            let result = req.request.send(&req.url);
            sender.send(threads::Response::Tracker(result)).expect("hi");
        }
    });

    tx
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use super::request::Request;

    #[test]
    fn send_test_1() {
        use super::request::Event::*;
        let test_req = Request {
            info_hash: hex!("d4437aed681cb06c5ecbcf2c7f590ae8a3f73aeb"),
            peer_id: "deadbeefdeadbeefbeef".as_bytes().try_into().unwrap(),
            my_port: 5000,
            uploaded: 420,
            downloaded: 69,
            left: 1337,
            event: Some(Started),
        };

        test_req.send("http://128.8.126.63:21212/announce").unwrap();
    }
}
