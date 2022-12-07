
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

mod response {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    pub struct Peer {
        #[serde(rename = "peer id")]
        peer_id: String,

        ip: String,

        port: u16,
    }

    #[derive(Serialize, Deserialize, PartialEq, Debug)]
    pub struct Response {
        #[serde(default)]
        interval: u32,

        #[serde(default)]
        peers: Vec<Peer>,
    }
}

use anyhow::Result;
use bendy::serde::from_bytes;

use request::Request;
use response::Response;
use urlencoding::{encode_binary, encode};

use crate::http::http_get;

impl Request {
    fn send(&self, url: &str) -> Result<Response> {
        // Try to send the HTTP request
        use request::Event::*;
        let query = [
            ("info_hash", encode_binary(&self.info_hash).into_owned()),
            ("peer_id", encode(&self.peer_id).into_owned()),
            ("port", self.my_port.to_string()),
            ("uploaded", self.uploaded.to_string()),
            ("downloaded", self.downloaded.to_string()),
            ("left", self.left.to_string()),
            ("event", match self.event {
                Some(Started) => String::from("started"),
                Some(Completed) => String::from("completed"),
                Some(Stopped) => String::from("stopped"),
                None => String::from("empty"),
            })
        ];
        let http_response = http_get(url, &query)?;
        println!("http_response: {:?}", String::from_utf8_lossy(&http_response.content));

        let tracker_response = from_bytes::<Response>(&http_response.content)?;

        println!("response: {:?}", tracker_response);

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
            peer_id: String::from("supercool"),
            my_port: 5000,
            uploaded: 420,
            downloaded: 69,
            left: 1337,
            event: Some(Started),
        };

        test_req.send("http://128.8.126.63:21212/announce").unwrap();
    }
}
