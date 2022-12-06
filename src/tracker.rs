mod request {
    use std::net::SocketAddr;

    pub enum Event {
        Started,
        Completed,
        Stopped,
    }

    pub struct Request {
        info_hash: [u8; 20],
        peer_id: String,
        my_addr: SocketAddr,
        uploaded: usize,
        downloaded: usize,
        left: usize,
        event: Option<Event>,
    }
}

mod response {
    use std::net::SocketAddr;

    pub struct Peer {
        addr: SocketAddr,
    }

    pub struct Response {
        interval: u32,
        peers: Vec<Peer>,
    }
}

use std::net::ToSocketAddrs;

use response::Response;
use request::Request;

impl Request {
    fn send(&self, dest: impl ToSocketAddrs) -> Response {
        unimplemented!("fuck you")
    }
}
