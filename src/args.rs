use std::{collections::HashMap, fs::File, io::Read, path::PathBuf};

use bendy::serde::from_bytes;
use clap::Parser;
use lazy_static::lazy_static;
use rand::{Rng, RngCore};

use crate::torrent::{Info, MetaInfo};

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
    #[arg(short, long, default_value_t = rand::thread_rng().gen_range(1025..65535))]
    pub port: u16,

    /// Continue seeding after file has been downloaded
    #[arg(short, long, default_value_t = false)]
    pub seed: bool,

    /// Seed a pre-existing file, rather than downloading the file and seeding it.
    #[arg(short = 'e', long, default_value_t = false)]
    pub seed_existing: bool,

    /// Number of outstanding requests to have per-peer
    #[arg(short, long, default_value_t = 10)]
    pub pipeline_depth: usize,

    /// Number of seconds to wait before dropping peer
    #[arg(short, long, default_value_t = 12)]
    pub request_timeout: u64,

    /// Skip getting peers from tracker, only accepting new manual connections
    #[arg(short = 'a', long, default_value_t = false)]
    pub skip_announce: bool,

    /// Add a single peer manually at the download's start
    #[arg(short = 'o', long)]
    pub add_peer: Option<String>,
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
    pub static ref METAINFO: MetaInfo<'static> = {
        let torrent_path = PathBuf::from(&ARGS.torrent);
        let mut torrent_file = File::open(torrent_path)
            .expect("Failed to open provided torrent file");
        let mut result = Vec::new();
        torrent_file
            .read_to_end(&mut result)
            .expect("Failed to read from provided torrent file");

        let metainfo = from_bytes::<MetaInfo>(&result)
            .expect("Failed to parse provided torrent file");

        let announce = metainfo.announce.clone();
        let piece_length = metainfo.info.piece_length;
        let pieces = metainfo.info.pieces.clone();
        let name = metainfo.info.name.clone();
        let length = metainfo.info.length;

        let mut remaining = HashMap::new();
        for (k, v) in metainfo.info.remaining.iter() {
            remaining.insert(k.clone(), v.clone().into_owned());
        }

        MetaInfo {
            announce,
            info: Info {
                piece_length,
                pieces,
                name,
                length,
                remaining,
            }
        }
    };
}
