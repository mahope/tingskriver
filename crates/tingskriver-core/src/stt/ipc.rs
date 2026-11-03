//! Length-prefixed control frames and raw little-endian PCM on private pipes.

use std::io::{self, Read, Write};

const MAX_CONTROL: usize = 1024 * 1024;
const MAX_SAMPLES: usize = 16000 * 60 * 60;

pub struct Frame {
    pub id: u64,
    pub control: Vec<u8>,
    pub samples: Vec<f32>,
}

pub fn write_frame(
    out: &mut impl Write,
    id: u64,
    control: &[u8],
    samples: &[f32],
) -> io::Result<()> {
    if control.len() > MAX_CONTROL || samples.len() > MAX_SAMPLES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "STT frame too large",
        ));
    }
    out.write_all(&id.to_le_bytes())?;
    out.write_all(&(control.len() as u32).to_le_bytes())?;
    out.write_all(&(samples.len() as u32).to_le_bytes())?;
    out.write_all(control)?;
    #[cfg(target_endian = "little")]
    {
        // f32 has no padding and every byte is initialized. The immutable
        // view cannot outlive or mutate the borrowed samples.
        let pcm = unsafe {
            std::slice::from_raw_parts(
                samples.as_ptr().cast::<u8>(),
                std::mem::size_of_val(samples),
            )
        };
        out.write_all(pcm)?;
    }
    #[cfg(target_endian = "big")]
    {
        let pcm: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        out.write_all(&pcm)?;
    }
    out.flush()
}

pub fn read_frame(input: &mut impl Read) -> io::Result<Option<Frame>> {
    let mut header = [0u8; 16];
    if input.read(&mut header[..1])? == 0 {
        return Ok(None);
    }
    input.read_exact(&mut header[1..])?;
    let id = u64::from_le_bytes(header[..8].try_into().unwrap());
    let control_len = u32::from_le_bytes(header[8..12].try_into().unwrap()) as usize;
    let samples_len = u32::from_le_bytes(header[12..].try_into().unwrap()) as usize;
    if control_len > MAX_CONTROL || samples_len > MAX_SAMPLES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "STT frame too large",
        ));
    }
    let mut control = vec![0; control_len];
    input.read_exact(&mut control)?;
    let mut samples = vec![0.0f32; samples_len];
    // Every f32 bit pattern is valid; the exclusive byte view ends before
    // any sample is read, and read_exact initializes the complete frame.
    let pcm = unsafe {
        std::slice::from_raw_parts_mut(samples.as_mut_ptr().cast::<u8>(), samples_len * 4)
    };
    input.read_exact(pcm)?;
    #[cfg(target_endian = "big")]
    for sample in &mut samples {
        *sample = f32::from_bits(sample.to_bits().swap_bytes());
    }
    Ok(Some(Frame {
        id,
        control,
        samples,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_preserve_request_id_and_fifteen_seconds_of_audio() {
        let samples = vec![0.125f32; 15 * 16000];
        let mut pipe = Vec::new();
        write_frame(&mut pipe, 42, b"transcribe", &samples).unwrap();
        let frame = read_frame(&mut pipe.as_slice()).unwrap().unwrap();
        assert_eq!(frame.id, 42);
        assert_eq!(frame.control, b"transcribe");
        assert_eq!(frame.samples, samples);
    }

    #[test]
    fn truncated_and_oversized_frames_fail_without_allocating_unbounded_memory() {
        assert!(read_frame(&mut &[1u8, 2][..]).is_err());
        let mut header = Vec::new();
        header.extend_from_slice(&1u64.to_le_bytes());
        header.extend_from_slice(&u32::MAX.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        assert!(read_frame(&mut header.as_slice()).is_err());
    }

    #[test]
    fn pcm_preserves_ieee_bits_and_rejects_a_truncated_sample() {
        let bits = [0x8000_0000, 1, 0x3f80_0000, 0x7f80_0000, 0x7fc0_0071];
        let samples: Vec<f32> = bits.iter().copied().map(f32::from_bits).collect();
        let mut pipe = Vec::new();
        write_frame(&mut pipe, 1, &[], &samples).unwrap();
        assert_eq!(&pipe[16..20], &bits[0].to_le_bytes());
        let frame = read_frame(&mut pipe.as_slice()).unwrap().unwrap();
        assert_eq!(
            frame
                .samples
                .iter()
                .map(|s| s.to_bits())
                .collect::<Vec<_>>(),
            bits
        );
        pipe.pop();
        assert!(read_frame(&mut pipe.as_slice()).is_err());
    }
}
