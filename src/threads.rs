use anyhow::Result;

use crate::connections::ConnectionData;
use crate::peers::PeerResponse;
use crate::tracker;

pub enum Response {
    Connection(ConnectionData),
    Peer(PeerResponse),
    Tracker(Result<tracker::response::Response>),
}
