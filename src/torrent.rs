use bendy::serde::to_bytes;
use serde::{Deserialize, Serialize};
use sha1::digest::Digest;
use sha1::Sha1;

const DIGEST_SIZE: usize = 20;

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct MetaInfo {
    announce: String,
    info: Info,
}

#[derive(Serialize, Deserialize, PartialEq, Debug)]
struct Info {
    #[serde(rename = "piece length")]
    piece_length: usize,

    #[serde(with = "serde_bytes")]
    pieces: Vec<u8>,

    name: String,

    length: usize,

    private: bool,
}

impl MetaInfo {
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
    fn meta_file_deserialize() {
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
}
