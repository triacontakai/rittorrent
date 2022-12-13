use anyhow::Result;

use crate::connections::ConnectionData;
use crate::peers::PeerResponse;
use crate::timer::TimerResponse;
use crate::tracker;

#[derive(Debug)]
pub enum Response {
    Connection(ConnectionData),
    Peer(PeerResponse),
    Tracker(Result<tracker::response::Response>),
    Timer(TimerResponse),
}
