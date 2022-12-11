use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    ops::{Range, IndexMut},
    path::Path,
};

use bitvec::prelude::*;
use sha1::{Digest, Sha1};

use anyhow::{bail, Result};

const DIGEST_SIZE: usize = 20;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Block<'a> {
    piece: usize,
    offset: usize,
    data: &'a [u8],
}

#[derive(Debug)]
struct Piece {
    range: Range<usize>,
    offset: usize,
    hash: [u8; DIGEST_SIZE],
    hasher: Sha1,
}

#[derive(Debug)]
pub struct DownloadFile {
    pieces: Vec<Piece>,
    bitfield: BitVec<u8, Msb0>,
    file: File,
    piece_size: usize,
}

impl<'a> Block<'a> {
    pub fn new(piece: usize, offset: usize, data: &'a [u8]) -> Self {
        Block {
            piece,
            offset,
            data,
        }
    }
}

impl Piece {
    fn is_complete(&self) -> bool {
        self.range.start.checked_add(self.offset).unwrap() == self.range.end
    }
}

impl DownloadFile {
    pub fn new(
        file_name: impl AsRef<Path>,
        hashes: &[[u8; DIGEST_SIZE]],
        piece_size: usize,
        total_size: usize,
    ) -> Result<Self> {
        let file = File::create(file_name)?;

        Self::new_from_file(file, hashes, piece_size, total_size)
    }

    fn new_from_file(
        file: File,
        hashes: &[[u8; DIGEST_SIZE]],
        piece_size: usize,
        total_size: usize,
    ) -> Result<Self> {
        let mut pieces = Vec::new();
        let mut offset = 0;

        file.set_len(total_size as u64)?;

        for hash in hashes {
            pieces.push(Piece {
                range: (offset..offset + piece_size),
                offset: 0,
                hash: *hash,
                hasher: Sha1::new(),
            });

            offset += piece_size;
        }

        // fix the end offset of the last piece
        let mut last = pieces.last_mut().unwrap();
        last.range.end = total_size;

        let num_pieces = pieces.len();

        Ok(DownloadFile {
            pieces,
            bitfield: bitvec![u8, Msb0; 0; num_pieces],
            file,
            piece_size,
        })
    }

    pub fn is_complete(&self) -> bool {
        self.bitfield.all()
    }

    pub fn bitfield(&self) -> &[u8] {
        self.bitfield.as_raw_slice()
    }

    pub fn process_block(&mut self, block: Block) -> Result<()> {
        if block.piece >= self.pieces.len() {
            bail!("piece index out of bounds");
        }

        let piece = &mut self.pieces[block.piece];

        // check if piece is contiguous to what we already have
        if block.offset != piece.offset {
            bail!("block does not match start of what we have read");
        }

        // check that block fits in bounds of piece
        let Some(current_pos) = piece.range.start.checked_add(piece.offset) else {
            bail!("block out of bounds");
        };
        let Some(new_pos) = current_pos.checked_add(block.data.len()) else {
            bail!("block out of bounds");
        };
        if new_pos > piece.range.end {
            bail!("block out of bounds");
        }

        // check if we have already completed the piece
        if piece.is_complete() {
            return Ok(());
        }

        // seek to position of piece in file and write block
        self.file
            .seek(SeekFrom::Start((piece.range.start + piece.offset) as u64))?;
        self.file.write_all(block.data)?;

        piece.hasher.update(block.data);

        // move piece offset forward based on what was written
        piece.offset += block.data.len();

        // if we are done with the piece, finalize hash and check if it is correct
        // if not correct, restart and reset hasher
        if piece.is_complete() {
            let mut hash = [0u8; DIGEST_SIZE];
            piece.hasher.finalize_into_reset((&mut hash).into());

            if hash == piece.hash {

                // add piece to completed bitmap
                // (no IndexMut<usize> for BitSlice ????)
                *self.bitfield.get_mut(block.piece).unwrap() = true;

                Ok(())
            } else {
                piece.offset = 0;
                Ok(())
            }
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Seek, SeekFrom};

    use hex_literal::hex;
    use tempfile;

    use super::{Block, DownloadFile};

    #[test]
    fn file_one_piece_success() {
        let data = vec![0; 1024];
        let hashes = &[hex!("60cacbf3d72e1e7834203da608037b1bf83b40e8")];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file = DownloadFile::new_from_file(temp_file, hashes, 1024, data.len()).unwrap();

        let block = Block::new(0, 0, &data[..]);

        file.process_block(block).unwrap();
        assert!(file.pieces[0].is_complete());

        // check file contents
        let mut buf = Vec::new();
        file.file.seek(SeekFrom::Start(0)).unwrap();

        file.file.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, data);
    }

    #[test]
    fn file_one_piece_badhash() {
        let data = vec![1; 1024];
        let hashes = &[hex!("60cacbf3d72e1e7834203da608037b1bf83b40e8")];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file = DownloadFile::new_from_file(temp_file, hashes, 1024, data.len()).unwrap();

        let block = Block::new(0, 0, &data[..]);

        file.process_block(block).unwrap();
        assert!(!file.pieces[0].is_complete());
    }

    #[test]
    fn file_one_piece_badhash_then_success() {
        let data = vec![1; 1024];
        let hashes = &[hex!("60cacbf3d72e1e7834203da608037b1bf83b40e8")];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file = DownloadFile::new_from_file(temp_file, hashes, 1024, data.len()).unwrap();

        let block = Block::new(0, 0, &data[..]);
        file.process_block(block).unwrap();
        assert!(!file.pieces[0].is_complete());

        let data_good = vec![0; 1024];
        let block = Block::new(0, 0, &data_good[..]);
        file.process_block(block).unwrap();

        assert!(file.pieces[0].is_complete());

        // check file contents
        let mut buf = Vec::new();
        file.file.seek(SeekFrom::Start(0)).unwrap();

        file.file.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, data_good);
    }

    #[test]
    fn file_two_piece_partial_success() {
        let data1 = vec![0; 1024];
        let data2 = vec![1; 1024];
        let hashes = &[
            hex!("60cacbf3d72e1e7834203da608037b1bf83b40e8"),
            hex!("376f19001dc171e2eb9c56962ca32478caaa7e39"),
        ];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file = DownloadFile::new_from_file(temp_file, hashes, 1024, 2048).unwrap();

        let (data2_0, data2_1) = data2.split_at(512);

        let block1 = Block::new(0, 0, &data1[..]);
        let block2_0 = Block::new(1, 0, &data2_0[..]);
        let block2_1 = Block::new(1, 512, &data2_1[..]);

        file.process_block(block1).unwrap();
        file.process_block(block2_0).unwrap();
        assert!(!file.pieces[1].is_complete());
        file.process_block(block2_1).unwrap();
        assert!(file.pieces[0].is_complete());
        assert!(file.pieces[1].is_complete());

        // check file contents
        let mut buf = Vec::new();
        file.file.seek(SeekFrom::Start(0)).unwrap();

        file.file.read_to_end(&mut buf).unwrap();
        assert_eq!(buf[..1024], data1);
        assert_eq!(buf[1024..], data2);
    }

    #[test]
    fn file_one_piece_toobig() {
        let data = vec![1; 1025];
        let hashes = &[hex!("60cacbf3d72e1e7834203da608037b1bf83b40e8")];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file = DownloadFile::new_from_file(temp_file, hashes, 1024, 1024).unwrap();

        let block = Block::new(0, 0, &data[..]);

        let res = file.process_block(block);
        assert!(res.is_err());
    }

    #[test]
    fn file_one_piece_irregular_size_success() {
        let data = vec![0; 727];
        let hashes = &[hex!("baa70378f8c072730b9d16869f32a65b7e5d8237")];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file = DownloadFile::new_from_file(temp_file, hashes, 727, data.len()).unwrap();

        let block = Block::new(0, 0, &data[..]);

        file.process_block(block).unwrap();
        assert!(file.pieces[0].is_complete());

        // check file contents
        let mut buf = Vec::new();
        file.file.seek(SeekFrom::Start(0)).unwrap();

        file.file.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, data);
    }

    #[test]
    fn file_two_piece_bitmap() {
        let data1 = vec![0; 1024];
        let data2 = vec![1; 1024];
        let hashes = &[
            hex!("60cacbf3d72e1e7834203da608037b1bf83b40e8"),
            hex!("376f19001dc171e2eb9c56962ca32478caaa7e39"),
        ];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file = DownloadFile::new_from_file(temp_file, hashes, 1024, 2048).unwrap();

        let (data2_0, data2_1) = data2.split_at(512);

        let block1 = Block::new(0, 0, &data1[..]);
        let block2_0 = Block::new(1, 0, &data2_0[..]);
        let block2_1 = Block::new(1, 512, &data2_1[..]);

        file.process_block(block1).unwrap();
        assert_ne!(file.bitfield()[0] & 0x80, 0);
        assert_eq!(file.bitfield()[0] & 0x40, 0);

        file.process_block(block2_0).unwrap();
        file.process_block(block2_1).unwrap();
        assert_ne!(file.bitfield()[0] & 0x80, 0);
        assert_ne!(file.bitfield()[0] & 0x40, 0);
    }
}