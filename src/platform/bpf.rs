//! Darwin BPF records use timeval32 and four-byte alignment on both 64-bit targets.
use anyhow::{Result, ensure};

pub(super) fn next_frame<'a>(bytes: &'a [u8], offset: &mut usize) -> Result<Option<&'a [u8]>> {
    if *offset >= bytes.len() {
        return Ok(None);
    }
    let record = &bytes[*offset..];
    ensure!(record.len() >= 18, "truncated BPF record header");
    let captured = u32::from_ne_bytes(record[8..12].try_into()?) as usize;
    let original = u32::from_ne_bytes(record[12..16].try_into()?) as usize;
    let header = u16::from_ne_bytes(record[16..18].try_into()?) as usize;
    ensure!(
        header >= 18 && header <= record.len(),
        "invalid BPF header length"
    );
    ensure!(
        captured == original && captured <= crate::protocol::MAX_FRAME,
        "truncated or oversized BPF packet"
    );
    let end = header
        .checked_add(captured)
        .ok_or_else(|| anyhow::anyhow!("BPF length overflow"))?;
    ensure!(end <= record.len(), "truncated BPF packet data");
    *offset += (end + 3) & !3;
    Ok(Some(&record[header..end]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(packet: &[u8], header: usize) -> Vec<u8> {
        let mut bytes = vec![0; (header + packet.len() + 3) & !3];
        bytes[8..12].copy_from_slice(&(packet.len() as u32).to_ne_bytes());
        bytes[12..16].copy_from_slice(&(packet.len() as u32).to_ne_bytes());
        bytes[16..18].copy_from_slice(&(header as u16).to_ne_bytes());
        bytes[header..header + packet.len()].copy_from_slice(packet);
        bytes
    }
    #[test]
    fn drains_all_records_with_kernel_header_lengths_and_padding() {
        let mut bytes = record(b"first", 18);
        bytes.extend(record(b"second packet", 20));
        let mut offset = 0;
        assert_eq!(
            next_frame(&bytes, &mut offset).unwrap(),
            Some(&b"first"[..])
        );
        assert_eq!(
            next_frame(&bytes, &mut offset).unwrap(),
            Some(&b"second packet"[..])
        );
        assert!(next_frame(&bytes, &mut offset).unwrap().is_none());
    }
    #[test]
    fn rejects_truncated_and_corrupt_records() {
        let bytes = record(b"payload", 18);
        for end in 1..25 {
            assert!(next_frame(&bytes[..end], &mut 0).is_err());
        }
        let mut bad = bytes.clone();
        bad[16..18].copy_from_slice(&17u16.to_ne_bytes());
        assert!(next_frame(&bad, &mut 0).is_err());
        let mut bad = bytes.clone();
        bad[8..12].copy_from_slice(&u32::MAX.to_ne_bytes());
        assert!(next_frame(&bad, &mut 0).is_err());
        let mut bad = bytes;
        bad[12..16].copy_from_slice(&99u32.to_ne_bytes());
        assert!(next_frame(&bad, &mut 0).is_err());
    }
}
