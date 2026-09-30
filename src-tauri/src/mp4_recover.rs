//! Lofty 0.22 skips a 64-bit MP4 atom (`size == 1`) eight bytes too far, because it
//! subtracts a normal 8-byte header after it has already consumed the 16-byte one.
//! The next atom, usually `moov`, is then missed (`No "moov" atom found`), and a
//! later full-file scan hits the end of the file while saving tags.
//!
//! When that 64-bit size still fits in 32 bits, rewrite the header as a normal
//! 8-byte atom and drop the extra 8 size bytes. Sample data is not re-encoded.
//! Chunk offsets at or after the removed bytes move back by 8.

use std::fs::File;
use std::io::{Cursor, Read};
use std::path::Path;

use lofty::config::{ParseOptions, ParsingMode};
use lofty::file::FileType;
use lofty::probe::Probe;

use crate::error::AppResult;

struct Atom {
    start: usize,
    size: usize,
    header: usize,
    extended: bool,
    kind: [u8; 4],
    children: Vec<Atom>,
}

/// Read tags the same way the editor does, including MP4 files whose `mdat` uses a 64-bit size.
pub fn open(path: &Path) -> AppResult<lofty::file::TaggedFile> {
    match read_lofty(path) {
        Ok(file) => Ok(file),
        Err(error) => match for_lofty(path)? {
            Some(bytes) => Ok(Probe::new(Cursor::new(bytes))
                .options(parse_options())
                .set_file_type(FileType::Mp4)
                .read()?),
            None => Err(error),
        },
    }
}

fn parse_options() -> ParseOptions {
    ParseOptions::new().parsing_mode(ParsingMode::Relaxed)
}

pub(crate) fn read_lofty(path: &Path) -> AppResult<lofty::file::TaggedFile> {
    Ok(Probe::open(path)?
        .options(parse_options())
        .guess_file_type()?
        .read()?)
}

/// Bytes Lofty can parse, when the file uses 64-bit atom sizes that fit in 32 bits.
/// `None` means the file is not an MP4, has no extended atoms, or is too large
/// to rewrite in memory (a 600 MB audiobook is left alone).
pub const RECOVER_CAP: u64 = 48 * 1024 * 1024;

pub fn for_lofty(path: &Path) -> AppResult<Option<Vec<u8>>> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    if !(8..=RECOVER_CAP).contains(&len) {
        return Ok(None);
    }
    let mut data = Vec::with_capacity(len as usize);
    file.read_to_end(&mut data)?;
    if data.len() < 8 || &data[4..8] != b"ftyp" {
        return Ok(None);
    }
    Ok(rewrite(&data))
}

fn rewrite(data: &[u8]) -> Option<Vec<u8>> {
    let atoms = parse_range(data, 0, data.len())?;
    let mut extended = Vec::new();
    collect_extended(&atoms, &mut extended);
    if extended.is_empty() {
        return None;
    }
    if extended
        .iter()
        .any(|atom| atom.size < 16 || atom.size - 8 > u32::MAX as usize)
    {
        return None;
    }

    let mut out = data.to_vec();
    write_shrunk_sizes(&mut out, &atoms)?;
    patch_chunk_offsets(&mut out, &atoms, &extended)?;

    extended.sort_by_key(|atom| atom.start);
    for atom in extended.iter().rev() {
        let at = atom.start + 8;
        if at + 8 > out.len() {
            return None;
        }
        out.drain(at..at + 8);
    }
    Some(out)
}

fn collect_extended(atoms: &[Atom], extended: &mut Vec<Atom>) {
    for atom in atoms {
        if atom.extended {
            extended.push(Atom {
                start: atom.start,
                size: atom.size,
                header: atom.header,
                extended: atom.extended,
                kind: atom.kind,
                children: Vec::new(),
            });
        }
        collect_extended(&atom.children, extended);
    }
}

fn shrink_of(atom: &Atom) -> usize {
    let nested: usize = atom.children.iter().map(shrink_of).sum();
    nested + if atom.extended { 8 } else { 0 }
}

fn write_shrunk_sizes(out: &mut [u8], atoms: &[Atom]) -> Option<()> {
    for atom in atoms {
        let shrink = shrink_of(atom);
        if shrink > 0 {
            let new_size = atom.size.checked_sub(shrink)?;
            if atom.extended {
                if new_size > u32::MAX as usize || atom.start + 8 > out.len() {
                    return None;
                }
                out[atom.start..atom.start + 4].copy_from_slice(&(new_size as u32).to_be_bytes());
            } else if new_size > u32::MAX as usize || atom.start + 4 > out.len() {
                return None;
            } else {
                out[atom.start..atom.start + 4].copy_from_slice(&(new_size as u32).to_be_bytes());
            }
        }
        write_shrunk_sizes(out, &atom.children)?;
    }
    Some(())
}

fn parse_range(data: &[u8], start: usize, end: usize) -> Option<Vec<Atom>> {
    if end > data.len() || start > end {
        return None;
    }
    let mut atoms = Vec::new();
    let mut pos = start;
    while pos + 8 <= end {
        let size32 = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?);
        let kind = data[pos + 4..pos + 8].try_into().ok()?;
        let (size, header, extended) = if size32 == 1 {
            if pos + 16 > end {
                return None;
            }
            let size64 = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().ok()?);
            if size64 < 16 || size64 > usize::MAX as u64 {
                return None;
            }
            (size64 as usize, 16, true)
        } else if size32 == 0 {
            (end - pos, 8, false)
        } else {
            if (size32 as usize) < 8 {
                return None;
            }
            (size32 as usize, 8, false)
        };
        if size < header || pos.checked_add(size)? > end {
            return None;
        }
        let mut atom = Atom {
            start: pos,
            size,
            header,
            extended,
            kind,
            children: Vec::new(),
        };
        if is_container(&kind) {
            if let Some(children) = parse_range(data, pos + header, pos + size) {
                atom.children = children;
            }
        }
        pos += size;
        atoms.push(atom);
    }
    Some(atoms)
}

fn is_container(kind: &[u8; 4]) -> bool {
    matches!(
        kind,
        b"moov"
            | b"trak"
            | b"mdia"
            | b"minf"
            | b"stbl"
            | b"udta"
            | b"edts"
            | b"moof"
            | b"traf"
            | b"mvex"
            | b"dinf"
    )
}

fn patch_chunk_offsets(out: &mut [u8], atoms: &[Atom], extended: &[Atom]) -> Option<()> {
    for atom in atoms {
        if atom.kind == *b"stco" || atom.kind == *b"co64" {
            patch_one_table(out, atom, extended)?;
        }
        patch_chunk_offsets(out, &atom.children, extended)?;
    }
    Some(())
}

fn patch_one_table(out: &mut [u8], atom: &Atom, extended: &[Atom]) -> Option<()> {
    let wide = atom.kind == *b"co64";
    let width = if wide { 8 } else { 4 };
    let base = atom.start + atom.header;
    if base + 8 > out.len() {
        return None;
    }
    let count = u32::from_be_bytes(out[base + 4..base + 8].try_into().ok()?) as usize;
    let table = base + 8;
    if table + count * width > out.len() {
        return None;
    }
    for index in 0..count {
        let at = table + index * width;
        let old = if wide {
            u64::from_be_bytes(out[at..at + 8].try_into().ok()?)
        } else {
            u64::from(u32::from_be_bytes(out[at..at + 4].try_into().ok()?))
        };
        let shift = 8 * extended
            .iter()
            .filter(|atom| old >= atom.start as u64 + 16)
            .count() as u64;
        let new = old - shift;
        if wide {
            out[at..at + 8].copy_from_slice(&new.to_be_bytes());
        } else {
            out[at..at + 4].copy_from_slice(&(new as u32).to_be_bytes());
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn atom(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(payload);
        out
    }

    fn extended(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let size = 16 + payload.len();
        let mut out = Vec::new();
        out.extend_from_slice(&1u32.to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(&(size as u64).to_be_bytes());
        out.extend_from_slice(payload);
        out
    }

    fn sample() -> Vec<u8> {
        let ftyp = atom(b"ftyp", b"M4A \0\0\0\0M4A mp42");
        let mdat = extended(b"mdat", b"audio-bytes");
        let mvhd = atom(b"mvhd", &[0u8; 20]);
        let stco_payload = {
            let mut payload = vec![0, 0, 0, 0, 0, 0, 0, 1];
            let offset = (ftyp.len() + 16) as u32;
            payload.extend_from_slice(&offset.to_be_bytes());
            payload
        };
        let stco = atom(b"stco", &stco_payload);
        let stbl = atom(b"stbl", &stco);
        let minf = atom(b"minf", &stbl);
        let mdia = atom(b"mdia", &minf);
        let trak = atom(b"trak", &mdia);
        let mut moov_body = mvhd;
        moov_body.extend_from_slice(&trak);
        let moov = atom(b"moov", &moov_body);
        let mut file = ftyp;
        file.extend_from_slice(&mdat);
        file.extend_from_slice(&moov);
        file
    }

    fn next_kind(data: &[u8], after: &[u8; 4]) -> Option<[u8; 4]> {
        let mut pos = 0;
        while pos + 8 <= data.len() {
            let size = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
            let kind: [u8; 4] = data[pos + 4..pos + 8].try_into().ok()?;
            if size < 8 || pos + size > data.len() {
                return None;
            }
            if &kind == after {
                let next = pos + size;
                if next + 8 > data.len() {
                    return None;
                }
                return data[next + 4..next + 8].try_into().ok();
            }
            pos += size;
        }
        None
    }

    #[test]
    fn extended_mdat_becomes_a_normal_atom_before_moov() {
        let file = sample();
        assert!(file.windows(4).any(|bytes| bytes == b"mdat"));
        let fixed = rewrite(&file).expect("recover");
        assert!(next_kind(&fixed, b"mdat") == Some(*b"moov"));
        assert!(!fixed
            .windows(8)
            .any(|bytes| bytes == [0, 0, 0, 1, b'm', b'd', b'a', b't']));
        assert!(fixed.windows(11).any(|bytes| bytes == b"audio-bytes"));
    }

    #[test]
    fn chunk_offset_moves_back_with_the_payload() {
        let file = sample();
        let fixed = rewrite(&file).expect("recover");
        let stco_at = fixed
            .windows(4)
            .position(|bytes| bytes == b"stco")
            .expect("stco");
        let offset = u32::from_be_bytes(fixed[stco_at + 12..stco_at + 16].try_into().unwrap());
        let ftyp_len = 8 + b"M4A \0\0\0\0M4A mp42".len();
        assert_eq!(offset as usize, ftyp_len + 8);
        assert_eq!(
            &fixed[offset as usize..offset as usize + 11],
            b"audio-bytes"
        );
    }

    #[test]
    fn plain_mp4_is_left_alone() {
        let file = atom(b"ftyp", b"M4A \0\0\0\0");
        assert!(rewrite(&file).is_none());
    }

    #[test]
    fn parent_size_shrinks_with_a_nested_extended_atom() {
        let inner = extended(b"trak", b"child");
        let moov = atom(b"moov", &inner);
        let mut file = atom(b"ftyp", b"M4A \0\0\0\0");
        file.extend_from_slice(&moov);
        let old_moov = file.windows(4).position(|bytes| bytes == b"moov").unwrap() - 4;
        let old_size = u32::from_be_bytes(file[old_moov..old_moov + 4].try_into().unwrap());
        let fixed = rewrite(&file).expect("recover");
        let new_moov = fixed.windows(4).position(|bytes| bytes == b"moov").unwrap() - 4;
        let new_size = u32::from_be_bytes(fixed[new_moov..new_moov + 4].try_into().unwrap());
        assert_eq!(new_size, old_size - 8);
        assert_eq!(fixed.len(), file.len() - 8);
    }
}
