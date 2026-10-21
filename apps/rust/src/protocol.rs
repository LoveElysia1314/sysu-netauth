//! Allocation-free parsing; only credential encoding and response construction allocate.
use anyhow::{Result, bail, ensure};
use encoding_rs::GBK;
use md5::{Digest, Md5};

pub const PAE: [u8; 6] = [1, 0x80, 0xc2, 0, 0, 3];
pub const ETHERTYPE: u16 = 0x888e;
pub const MAX_FRAME: usize = 2048;

#[derive(Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    pub source: [u8; 6],
    pub destination: [u8; 6],
    pub code: u8,
    pub id: u8,
    pub kind: Option<u8>,
    pub data: &'a [u8],
}

pub fn parse(raw: &[u8]) -> Option<Packet<'_>> {
    if raw.len() < 22 || raw[12..14] != ETHERTYPE.to_be_bytes() || raw[15] != 0 {
        return None;
    }
    if !(1..=3).contains(&raw[14]) {
        return None;
    }
    let outer = u16::from_be_bytes([raw[16], raw[17]]) as usize;
    let len = u16::from_be_bytes([raw[20], raw[21]]) as usize;
    if outer < 4 || outer > raw.len() - 18 || len < 4 || len > outer {
        return None;
    }
    let code = raw[18];
    if !(1..=4).contains(&code) || (code <= 2 && len < 5) {
        return None;
    }
    Some(Packet {
        source: raw[6..12].try_into().ok()?,
        destination: raw[..6].try_into().ok()?,
        code,
        id: raw[19],
        kind: (code <= 2).then(|| raw[22]),
        data: if code <= 2 { &raw[23..18 + len] } else { &[] },
    })
}

pub fn gbk(text: &str) -> Result<Vec<u8>> {
    let (bytes, _, invalid) = GBK.encode(text);
    ensure!(
        !invalid,
        "credentials contain characters unavailable in GBK"
    );
    Ok(bytes.into_owned())
}

pub fn control(source: [u8; 6], kind: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(18);
    out.extend_from_slice(&PAE);
    out.extend_from_slice(&source);
    out.extend_from_slice(&ETHERTYPE.to_be_bytes());
    out.extend_from_slice(&[1, kind, 0, 0]);
    out
}

pub fn response(
    source: [u8; 6],
    destination: [u8; 6],
    id: u8,
    kind: u8,
    data: &[u8],
) -> Result<Vec<u8>> {
    ensure!(data.len() + 23 <= MAX_FRAME, "EAP response too large");
    let len = (5 + data.len()) as u16;
    let mut out = Vec::with_capacity(23 + data.len());
    out.extend_from_slice(&destination);
    out.extend_from_slice(&source);
    out.extend_from_slice(&ETHERTYPE.to_be_bytes());
    out.extend_from_slice(&[1, 0]);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(&[2, id]);
    out.extend_from_slice(&len.to_be_bytes());
    out.push(kind);
    out.extend_from_slice(data);
    Ok(out)
}

pub fn answer(
    source: [u8; 6],
    destination: [u8; 6],
    packet: &Packet<'_>,
    username: &[u8],
    password: &[u8],
) -> Result<Option<Vec<u8>>> {
    if packet.code != 1 {
        return Ok(None);
    }
    match packet.kind {
        Some(1) => Ok(Some(response(source, destination, packet.id, 1, username)?)),
        Some(4) => {
            let Some(&len) = packet.data.first() else {
                bail!("empty MD5 challenge");
            };
            ensure!(
                len > 0 && packet.data.len() > len as usize,
                "truncated MD5 challenge"
            );
            let mut hash = Md5::new();
            hash.update([packet.id]);
            hash.update(password);
            hash.update(&packet.data[1..=len as usize]);
            let mut body = Vec::with_capacity(17 + username.len());
            body.push(16);
            body.extend_from_slice(&hash.finalize());
            body.extend_from_slice(username);
            Ok(Some(response(source, destination, packet.id, 4, &body)?))
        }
        _ => Ok(None),
    }
}

pub fn mac_text(mac: [u8; 6]) -> String {
    mac.iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_fixture() {
        let raw = response([0, 17, 34, 51, 68, 85], PAE, 7, 1, b"netid").unwrap();
        assert_eq!(
            raw,
            [
                1, 128, 194, 0, 0, 3, 0, 17, 34, 51, 68, 85, 136, 142, 1, 0, 0, 10, 2, 7, 0, 10, 1,
                110, 101, 116, 105, 100
            ]
        );
        assert_eq!(parse(&raw).unwrap().data, b"netid");
    }
    #[test]
    fn bounds_and_padding() {
        let raw = response([0; 6], PAE, 1, 1, b"name").unwrap();
        for n in 0..raw.len() {
            assert!(parse(&raw[..n]).is_none());
        }
        let mut padded = raw.clone();
        padded.resize(60, 0);
        assert!(parse(&padded).is_some());
        padded[16..18].copy_from_slice(&3u16.to_be_bytes());
        assert!(parse(&padded).is_none());
        let mut bad = raw;
        bad[20..22].copy_from_slice(&255u16.to_be_bytes());
        assert!(parse(&bad).is_none());
    }
    #[test]
    fn gbk_md5() {
        assert_eq!(gbk("密码").unwrap(), [0xc3, 0xdc, 0xc2, 0xeb]);
        assert!(gbk("🔐").is_err());
        let mut data = vec![16];
        data.extend(0..16);
        let p = Packet {
            source: [2; 6],
            destination: PAE,
            code: 1,
            id: 3,
            kind: Some(4),
            data: &data,
        };
        let raw = answer([0; 6], PAE, &p, b"user", &gbk("密码").unwrap())
            .unwrap()
            .unwrap();
        let parsed = parse(&raw).unwrap();
        let mut hash = Md5::new();
        hash.update([3, 0xc3, 0xdc, 0xc2, 0xeb]);
        hash.update((0..16).collect::<Vec<u8>>());
        assert_eq!(&parsed.data[1..17], hash.finalize().as_slice());
        for bad in [&[][..], &[0][..], &[16, 1, 2][..]] {
            assert!(answer([0; 6], PAE, &Packet { data: bad, ..p }, b"u", b"p").is_err());
        }
    }
    #[test]
    fn arbitrary_input_does_not_panic() {
        let mut seed = 7u32;
        for len in 0..4096 {
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 17;
                    seed ^= seed << 5;
                    seed as u8
                })
                .collect();
            let _ = parse(&bytes);
        }
    }
}
