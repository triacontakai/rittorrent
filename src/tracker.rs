mod request {
    pub enum Event {
        Started,
        Completed,
        Stopped,
    }

    pub struct Request {
        pub info_hash: [u8; 20],
        pub peer_id: String,
        pub my_port: u16,
        pub uploaded: usize,
        pub downloaded: usize,
        pub left: usize,
        pub event: Option<Event>,
    }
}

pub mod response {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    pub struct Peer {
        #[serde(rename = "peer id", with = "serde_bytes")]
        pub peer_id: Vec<u8>,

        pub ip: String,

        pub port: u16,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    pub struct Response {
        #[serde(default)]
        pub interval: u32,

        #[serde(default)]
        pub peers: Vec<Peer>,
    }
}

use anyhow::Result;
use bendy::serde::from_bytes;

use request::Request;
use response::Response;

use crate::http::http_get;

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
            ("peer_id", &self.peer_id.as_bytes()),
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

        Ok(tracker_response)
    }
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
            peer_id: String::from("deadbeefdeadbeefbeef"),
            my_port: 5000,
            uploaded: 420,
            downloaded: 69,
            left: 1337,
            event: Some(Started),
        };

        test_req.send("http://128.8.126.63:21212/announce").unwrap();
    }
}
