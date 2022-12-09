use clap::{ValueEnum, Parser};
use lazy_static::lazy_static;

#[derive(ValueEnum, Clone, Debug)]
enum TrackerType {
    Http,
    Udp,
}

/// A moderately functional BitTorrent client written in Rust
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Name of the torrent file to download
    #[arg(short, long)]
    torrent: String,

    /// Maximum number of peer connections to maintain
    #[arg(short, long, default_value_t = 10)]
    max_connections: usize,

    /// Port to listen on. Random if not provided
    #[arg(short, long)]
    port: Option<u16>,

    // Force a specific tracker protocol to be used
    #[arg(short = 'r', long)]
    tracker_type: TrackerType,
}

lazy_static! {
    pub static ref ARGS: Args = Args::parse();
}
