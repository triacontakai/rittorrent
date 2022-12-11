use std::{fs::File, io::Read, path::PathBuf};

use bendy::serde::from_bytes;
use clap::{Parser, ValueEnum};
use lazy_static::lazy_static;
use rand::RngCore;

use crate::torrent::MetaInfo;

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
    pub torrent: String,

    /// Maximum number of peer connections to maintain
    #[arg(short, long, default_value_t = 10)]
    pub max_connections: usize,

    /// Port to listen on. Random if not provided
    #[arg(short, long)]
    pub port: Option<u16>,

    // Force a specific tracker protocol to be used
    #[arg(short = 'r', long)]
    pub tracker_type: TrackerType,
}

const PEER_ID_LEN: usize = 20;

lazy_static! {
    // Command-line arguments
    pub static ref ARGS: Args = Args::parse();

    // Ranodmly-generated peer id
    pub static ref PEER_ID: [u8; PEER_ID_LEN] = {
        let mut data = [0u8; PEER_ID_LEN];
        rand::thread_rng().fill_bytes(&mut data);
        data
    };

    // Parsed metainfo file
    pub static ref METAINFO: MetaInfo = {
        let torrent_path = PathBuf::from(&ARGS.torrent);
        let mut torrent_file = File::open(torrent_path)
            .expect("Failed to open provided torrent file");
        let mut result = Vec::new();
        torrent_file
            .read_to_end(&mut result)
            .expect("Failed to read from provided torrent file");

        from_bytes::<MetaInfo>(&result)
            .expect("Failed to parse provided torrent file")
    };
}
