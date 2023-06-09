use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    ops::Range,
    path::Path,
};

use bitvec::prelude::*;
use sha1::{Digest, Sha1};

use anyhow::{bail, Result};

const DIGEST_SIZE: usize = 20;
const BLOCK_SIZE: usize = 16384;

#[derive(Clone, Debug, PartialEq)]
pub struct BlockInfo {
    pub piece: usize,
    pub range: Range<usize>,
}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Block {
    piece: usize,
    offset: usize,
    data: Vec<u8>,
}

#[derive(Debug)]
struct Piece {
    unfilled: Vec<Range<usize>>, // this is really more of a Set, but we want to be able to return it as a slice
    all_blocks: Vec<Range<usize>>,
    offset: usize,
    length: usize,
    hash: [u8; DIGEST_SIZE],
}

#[derive(Debug)]
pub struct DownloadFile {
    pieces: Vec<Piece>,
    bitfield: BitVec<u8, Msb0>,
    file: File,
    downloaded: usize,
    total_size: usize,
}

impl Block {
    pub fn new(piece: usize, offset: usize, data: &[u8]) -> Self {
        Block {
            piece,
            offset,
            data: data.to_vec(),
        }
    }

    pub fn info(&self) -> BlockInfo {
        BlockInfo {
            piece: self.piece,
            range: self.offset..(self.offset + self.data.len()),
        }
    }
}

impl Piece {
    fn is_complete(&self) -> bool {
        //self.range.start.checked_add(self.offset).unwrap() == self.range.end
        self.unfilled.is_empty()
    }
}

fn get_block_ranges(start: usize, end: usize, size: usize) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();

    let mut current = start;

    while current + size < end {
        ranges.push(current..current + size);
        current += size;
    }

    if current < end {
        ranges.push(current..end);
    }
    ranges
}

impl DownloadFile {
    pub fn new(
        file_name: impl AsRef<Path>,
        hashes: &[[u8; DIGEST_SIZE]],
        piece_size: usize,
        total_size: usize,
    ) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .truncate(true)
            .create(true)
            .open(file_name)?;

        Self::new_from_file(file, hashes, piece_size, total_size)
    }

    pub fn new_seeding(
        file_name: impl AsRef<Path>,
        hashes: &[[u8; DIGEST_SIZE]],
        piece_size: usize,
        total_size: usize,
    ) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(file_name)?;
        let mut download_file = Self::new_from_file(file, hashes, piece_size, total_size)?;
        download_file.downloaded = download_file.total_size;

        for mut bit in download_file.bitfield.iter_mut() {
            *bit = true;
        }

        // loop through each piece and empty unfilled, since we have entire file
        for piece in download_file.pieces.iter_mut() {
            piece.unfilled.clear();
        }

        Ok(download_file)
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

        // loop through all but last piece
        for hash in hashes.iter().rev().skip(1).rev() {
            let all_blocks = get_block_ranges(0, piece_size, BLOCK_SIZE);
            let unfilled = all_blocks.clone();

            pieces.push(Piece {
                unfilled,
                all_blocks,
                offset,
                length: piece_size,
                hash: *hash,
            });

            offset += piece_size;
        }

        // special case for last piece since it can be short
        let all_blocks = get_block_ranges(0, total_size - offset, BLOCK_SIZE);
        let unfilled = all_blocks.clone();
        pieces.push(Piece {
            unfilled,
            all_blocks,
            offset,
            length: total_size - offset,
            hash: *hashes.last().expect("invalid size of hash list"),
        });

        let num_pieces = pieces.len();

        Ok(DownloadFile {
            pieces,
            bitfield: bitvec![u8, Msb0; 0; num_pieces],
            file,
            downloaded: 0,
            total_size,
        })
    }

    pub fn is_complete(&self) -> bool {
        self.bitfield.all()
    }

    pub fn bitfield(&self) -> &[u8] {
        self.bitfield.as_raw_slice()
    }

    // Return a copy of our current BitVec
    pub fn bitvec(&self) -> &BitVec<u8, Msb0> {
        &self.bitfield
    }

    /// Return a `Some(&[Range<usize])` containing all the unfilled ranges for the given piece
    /// Returns [None] if `piece` is out of bounds
    pub fn get_unfilled(&self, piece: usize) -> Option<&[Range<usize>]> {
        self.pieces.get(piece).map(|x| &x.unfilled[..])
    }

    pub fn piece_is_complete(&self, piece: usize) -> Result<bool> {
        let Some(piece) = self.pieces.get(piece) else {
            bail!("invalid piece index");
        };

        Ok(piece.is_complete())
    }

    /// Returns number of bytes left to download.
    /// This has a resolution of piece sizes, and only goes down when we get a full valid piece.
    pub fn left(&self) -> usize {
        self.total_size
            .checked_sub(self.downloaded)
            .expect("violated invariant total_size >= downloaded")
    }

    /// Returns the bytes matching the given [BlockInfo]
    /// Returns [None] if the passed [BlockInfo] does not exist
    pub fn get_block(&mut self, block: BlockInfo) -> Result<Vec<u8>> {
        let Some(piece) = self.pieces.get(block.piece) else {
            bail!("invalid piece index");
        };

        if !piece.is_complete() {
            bail!("piece is not complete");
        }

        let range = 0..piece.length;
        if block.range.start < range.start || block.range.end > range.end {
            bail!("block range invalid");
        }

        let mut data = vec![0u8; block.range.end - block.range.start];
        self.file
            .seek(SeekFrom::Start((piece.offset + block.range.start) as u64))?;
        self.file.read_exact(&mut data)?;

        Ok(data)
    }

    /// Pass a block to the DownloadFile in order to be processed
    /// Returns [Err] if block is for an out-of-range piece/file operations failed, and [Ok] otherwise
    pub fn process_block(&mut self, block: Block) -> Result<()> {
        let Some(piece) = self.pieces.get_mut(block.piece) else {
            bail!("piece out of range");
        };

        let range = block.offset..(block.offset + block.data.len());

        // if the piece is already done we don't need to do any work
        if piece.is_complete() {
            return Ok(());
        }

        // find this block
        let Some(idx) = piece.unfilled.iter().position(|x| *x == range) else {
            return Ok(());
        };

        // seek to position in file and write this block, since by this point we know it is unfilled
        self.file
            .seek(SeekFrom::Start((range.start + piece.offset) as u64))?;
        self.file.write_all(&block.data[..])?;

        // this block now counts as filled, so remove from unfilled
        piece.unfilled.swap_remove(idx);

        // if piece is complete, do hashing to verify integrity
        if piece.is_complete() {
            let mut hasher = Sha1::new();
            let mut buf = vec![0u8; 4096];

            self.file.seek(SeekFrom::Start(piece.offset as u64))?;
            let mut remaining = piece.length;
            while remaining > 0 {
                let to_read = buf.len().min(remaining);
                let bytes_read = self.file.read(&mut buf[..to_read])?;

                hasher.update(&buf[..bytes_read]);
                remaining -= bytes_read;
            }

            let hash = hasher.finalize();
            if hash == piece.hash.into() {
                *self.bitfield.get_mut(block.piece).unwrap() = true;
                self.downloaded += piece.length;
                Ok(())
            } else {
                piece.unfilled = piece.all_blocks.clone();
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

    use crate::file::{BlockInfo, BLOCK_SIZE};

    use super::{get_block_ranges, Block, DownloadFile, DIGEST_SIZE};

    #[test]
    fn get_block_ranges_test() {
        let ranges = get_block_ranges(0, 33, 10);
        assert_eq!(&ranges, &[(0..10), (10..20), (20..30), (30..33)]);
    }

    #[test]
    fn get_block_ranges_test_current_is_end() {
        let ranges = get_block_ranges(0, 0, 10);
        assert_eq!(&ranges, &[]);
    }

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
        assert_eq!(file.left(), 0);
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
        let data1 = vec![0; BLOCK_SIZE * 2];
        let data2 = vec![1; BLOCK_SIZE * 2];
        let hashes = &[
            hex!("5188431849b4613152fd7bdba6a3ff0a4fd6424b"),
            hex!("d3a26f5cc20679c826302154ccd89edd238cfaca"),
        ];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file =
            DownloadFile::new_from_file(temp_file, hashes, BLOCK_SIZE * 2, BLOCK_SIZE * 4).unwrap();

        let (data1_0, data1_1) = data1.split_at(BLOCK_SIZE);
        let (data2_0, data2_1) = data2.split_at(BLOCK_SIZE);

        let block1_0 = Block::new(0, 0, &data1_0[..]);
        let block1_1 = Block::new(0, BLOCK_SIZE, &data1_1[..]);
        let block2_0 = Block::new(1, 0, &data2_0[..]);
        let block2_1 = Block::new(1, BLOCK_SIZE, &data2_1[..]);

        file.process_block(block1_0).unwrap();
        file.process_block(block1_1).unwrap();
        file.process_block(block2_0).unwrap();
        assert!(file.pieces[0].is_complete());
        assert!(!file.pieces[1].is_complete());
        file.process_block(block2_1).unwrap();
        eprintln!("{:?}", file.pieces[1].unfilled);
        assert!(file.pieces[0].is_complete());
        assert!(file.pieces[1].is_complete());

        // check file contents
        let mut buf = Vec::new();
        file.file.seek(SeekFrom::Start(0)).unwrap();

        file.file.read_to_end(&mut buf).unwrap();
        assert_eq!(buf[..BLOCK_SIZE * 2], data1);
        assert_eq!(buf[BLOCK_SIZE * 2..], data2);
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
        let data1 = vec![0; BLOCK_SIZE * 2];
        let data2 = vec![1; BLOCK_SIZE * 2];
        let hashes = &[
            hex!("5188431849b4613152fd7bdba6a3ff0a4fd6424b"),
            hex!("d3a26f5cc20679c826302154ccd89edd238cfaca"),
        ];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file =
            DownloadFile::new_from_file(temp_file, hashes, BLOCK_SIZE * 2, BLOCK_SIZE * 4).unwrap();

        let (data1_0, data1_1) = data1.split_at(16384);
        let (data2_0, data2_1) = data2.split_at(16384);

        let block1_0 = Block::new(0, 0, &data1_0[..]);
        let block1_1 = Block::new(0, BLOCK_SIZE, &data1_1[..]);
        let block2_0 = Block::new(1, 0, &data2_0[..]);
        let block2_1 = Block::new(1, BLOCK_SIZE, &data2_1[..]);

        file.process_block(block1_0).unwrap();
        file.process_block(block1_1).unwrap();
        file.process_block(block2_0).unwrap();
        assert_eq!(file.bitfield(), &[0x80]);
        file.process_block(block2_1).unwrap();
        assert_eq!(file.bitfield(), &[0xc0]);

        // check file contents
        let mut buf = Vec::new();
        file.file.seek(SeekFrom::Start(0)).unwrap();

        file.file.read_to_end(&mut buf).unwrap();
        assert_eq!(buf[..BLOCK_SIZE * 2], data1);
        assert_eq!(buf[BLOCK_SIZE * 2..], data2);
    }

    #[test]
    fn file_get_block_success() {
        let data = vec![0; 1024];
        let hashes = &[hex!("60cacbf3d72e1e7834203da608037b1bf83b40e8")];
        let temp_file = tempfile::tempfile().unwrap();

        let mut file = DownloadFile::new_from_file(temp_file, hashes, 1024, data.len()).unwrap();

        let block = Block::new(0, 0, &data[..]);

        file.process_block(block).unwrap();
        assert!(file.pieces[0].is_complete());

        // check file contents
        let buf = file
            .get_block(BlockInfo {
                piece: 0,
                range: 0..1024,
            })
            .unwrap();
        assert_eq!(buf, data);
    }

    #[test]
    fn new_seeding_invariants() {
        let temp_file = tempfile::NamedTempFile::new().unwrap();
        let hashes = &[[0u8; DIGEST_SIZE]; 4];
        let file =
            DownloadFile::new_seeding(temp_file.path(), hashes, BLOCK_SIZE * 4, BLOCK_SIZE * 16)
                .unwrap();

        assert!(file.is_complete());
        assert_eq!(file.bitfield(), &[0b11110000]);
    }
}
