use std::any::Any;
use std::collections::HashMap;

use bendy::{serde::to_bytes, value::Value};
use serde::{Deserialize, Serialize};
use sha1::digest::Digest;
use sha1::Sha1;

const DIGEST_SIZE: usize = 20;

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct MetaInfo<'a> {
    pub announce: String,

    #[serde(borrow = "'a")]
    pub info: Info<'a>,
}

#[derive(Serialize, Deserialize, PartialEq, Debug, Clone)]
pub struct Info<'a> {
    #[serde(rename = "piece length")]
    pub piece_length: usize,

    #[serde(with = "serde_bytes")]
    pub pieces: Vec<u8>,

    pub name: String,

    pub length: usize,

    #[serde(flatten, borrow = "'a")]
    pub remaining: HashMap<String, Value<'a>>,
}

impl MetaInfo<'_> {
    pub fn info_hash(&self) -> [u8; DIGEST_SIZE] {
        let mut hasher = Sha1::new();
        hasher.update(to_bytes(&self.info).unwrap());
        hasher.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use bendy::serde::{from_bytes, to_bytes};
    use hex_literal::hex;
    use std::{fs::File, io::Read, path::PathBuf};

    use super::MetaInfo;

    #[test]
    fn meta_file_deserialize_flatland() {
        let mut flatland_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        flatland_path.push("resources/flatland.torrent");

        let mut flatland_file = File::open(flatland_path).unwrap();
        let mut result = Vec::new();
        flatland_file.read_to_end(&mut result).unwrap();

        let info = from_bytes::<MetaInfo>(&result).unwrap();

        assert_eq!(info.announce, "http://128.8.126.63:21212/announce");

        let hash = info.info_hash();
        assert_eq!(hash, hex!("d4437aed681cb06c5ecbcf2c7f590ae8a3f73aeb"));
    }

    #[test]
    fn meta_file_deserialize_debian() {
        let mut debian_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        debian_path.push("resources/debian-11.5.0-amd64-netinst.iso.torrent");

        let mut debian_file = File::open(debian_path).unwrap();
        let mut result = Vec::new();
        debian_file.read_to_end(&mut result).unwrap();

        let info = from_bytes::<MetaInfo>(&result).unwrap();

        assert_eq!(info.announce, "http://bttracker.debian.org:6969/announce");

        let hash = info.info_hash();
        assert_eq!(hash, hex!("d55be2cd263efa84aeb9495333a4fabc428a4250"));
    }
}
