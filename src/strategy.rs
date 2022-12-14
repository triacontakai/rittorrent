use std::{collections::HashMap, net::SocketAddr};

use rand::seq::SliceRandom;

use crate::{
    file::{self, BlockInfo},
    MainState,
};

// TODO: this should be a cmdline argument
const PIPELINE_DEPTH: usize = 3;

//pub trait BlockStrategy {
//    fn pick_blocks(state: MainState) -> Vec<(file::BlockInfo, SocketAddr)>;
//}
//
//pub struct RarestFirst;
//
//impl RarestFirst {
//    pub fn new() -> Self { Self {} }
//}
//
//impl BlockStrategy for RarestFirst {
//}

// TODO: wrap this in a trait maybe?
//pub fn pick_blocks(state: &MainState) -> Vec<(file::BlockInfo, SocketAddr)> {
//    let ret = Vec::new();
//
//    // random order
//    let mut addrs: Vec<SocketAddr> = state.peers.keys().map(|x| *x).collect();
//    addrs.shuffle(&mut rand::thread_rng());
//
//    let mut iter = addrs.iter();
//    while let Some(addr) = iter.next() {
//        println!("Processing peer {:?}", addr);
//    }
//
//    ret
//}

// for now pick randomly lol.
pub fn pick_blocks(state: &MainState) -> Vec<(file::BlockInfo, SocketAddr)> {
    let mut ret = Vec::new();

    // random order
    let mut addrs: Vec<SocketAddr> = state.peers.keys().map(|x| *x).collect();
    addrs.shuffle(&mut rand::thread_rng());

    let mut iter = addrs.iter();
    while let Some(&addr) = iter.next() {
        // get the peer info
        let peer_info = state.peers.get(&addr).unwrap();

        // if we're being choked, don't do anything
        if peer_info.peer_choked {
            continue;
        }

        // find current # of outstanding requests
        let mut count = state
            .requested
            .iter()
            .filter(|&(_, (_, a))| *a == addr)
            .count();

        // keep requesting blocks until we reach pipeline depth
        let mut iter_ones = peer_info.has.iter_ones();
        'outer: while let Some(piece) = iter_ones.next() {
            // What blocks are outstanding for this piece?
            let Some(ranges) = state.file.get_blocks(piece) else {
                continue;
            };

            for range in ranges {
                // if we have reached pipeline depth, stop making requests
                if count >= PIPELINE_DEPTH {
                    break 'outer;
                }

                // construct BlockInfo
                let block_info = BlockInfo {
                    piece: piece,
                    range: range.clone(),
                };

                // if we already have an outstanding request for this
                // block, don't make another one
                if state.requested.values().any(|(b, _)| *b == block_info) {
                    continue;
                }

                // if we're already making a request for this block, don't
                // make another one
                if ret.iter().any(|(b, _)| *b == block_info) {
                    continue;
                }

                // otherwise, add this block
                ret.push((block_info, addr));

                // and increment count
                count += 1;
            }
        }
    }

    ret
}
