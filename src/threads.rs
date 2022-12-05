use crate::connections::ConnectionData;
use crate::peers::PeerResponse;

pub enum Response {
    Connection(ConnectionData),
    Peer(PeerResponse),
}
