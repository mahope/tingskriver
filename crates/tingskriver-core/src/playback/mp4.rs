//! Read the selected MP4 track's single edit without loading media or sample tables.
//! Symphonia 0.5 parses edit lists but does not apply them to packet timestamps.
use super::PlaybackError;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn bytes<const N: usize>(f: &mut File, at: u64) -> Result<[u8; N], PlaybackError> {
    f.seek(SeekFrom::Start(at))
        .map_err(|_| PlaybackError::Decode)?;
    let mut b = [0; N];
    f.read_exact(&mut b).map_err(|_| PlaybackError::Decode)?;
    Ok(b)
}
fn atom(f: &mut File, at: u64, end: u64) -> Result<([u8; 4], u64, u64), PlaybackError> {
    if end.saturating_sub(at) < 8 {
        return Err(PlaybackError::Decode);
    }
    let header = bytes::<8>(f, at)?;
    let mut size = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as u64;
    let mut head = 8;
    if size == 1 {
        if end - at < 16 {
            return Err(PlaybackError::Decode);
        }
        size = u64::from_be_bytes(bytes(f, at + 8)?);
        head = 16;
    }
    if size == 0 {
        size = end - at;
    }
    if size < head || size > end - at {
        return Err(PlaybackError::Decode);
    }
    Ok((
        [header[4], header[5], header[6], header[7]],
        at + head,
        at + size,
    ))
}
fn find(
    f: &mut File,
    mut at: u64,
    end: u64,
    kind: [u8; 4],
    mut index: u32,
) -> Result<Option<(u64, u64)>, PlaybackError> {
    while at < end {
        let (name, body, next) = atom(f, at, end)?;
        if name == kind {
            if index == 0 {
                return Ok(Some((body, next)));
            }
            index -= 1;
        }
        at = next;
    }
    Ok(None)
}
#[allow(clippy::disallowed_methods)]
pub(super) fn media_offset(path: &Path, track: u32) -> Result<u64, PlaybackError> {
    let mut f = File::open(path).map_err(|_| PlaybackError::Open)?;
    let end = f.metadata().map_err(|_| PlaybackError::Open)?.len();
    if end < 8 || bytes::<4>(&mut f, 4)? != *b"ftyp" {
        return Ok(0);
    }
    let Some((moov, moov_end)) = find(&mut f, 0, end, *b"moov", 0)? else {
        return Ok(0);
    };
    let Some((trak, trak_end)) = find(&mut f, moov, moov_end, *b"trak", track)? else {
        return Ok(0);
    };
    let Some((edts, edts_end)) = find(&mut f, trak, trak_end, *b"edts", 0)? else {
        return Ok(0);
    };
    let Some((elst, elst_end)) = find(&mut f, edts, edts_end, *b"elst", 0)? else {
        return Ok(0);
    };
    if elst_end - elst < 8 {
        return Err(PlaybackError::Decode);
    }
    let version = bytes::<1>(&mut f, elst)?[0];
    let count = u32::from_be_bytes(bytes(&mut f, elst + 4)?);
    if count == 0 {
        return Ok(0);
    }
    if count != 1 {
        return Err(PlaybackError::Unsupported);
    }
    let (offset, rate_at) = match version {
        0 if elst_end - elst >= 20 => (
            i32::from_be_bytes(bytes(&mut f, elst + 12)?) as i64,
            elst + 16,
        ),
        1 if elst_end - elst >= 28 => (i64::from_be_bytes(bytes(&mut f, elst + 16)?), elst + 24),
        _ => return Err(PlaybackError::Unsupported),
    };
    if offset < 0 || bytes::<4>(&mut f, rate_at)? != [0, 1, 0, 0] {
        return Err(PlaybackError::Unsupported);
    }
    Ok(offset as u64)
}
