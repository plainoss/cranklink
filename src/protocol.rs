//! SM-PCE02 framing; retains routing bytes for command correlation.
//!
//! A serial frame is `BB || escape(control || payload || FCS) || BB`, where
//! the unescaped bytes sum to zero modulo 256. Escape `BB` as `BD 9B` and
//! `BD` as `BD 9D`, including either value in the FCS. Host writes are padded
//! to at least 128 bytes with `FF` after the closing delimiter; this is not a
//! receive-record size. An `FF` inside a frame remains payload data.
//!
//! Reads may split a frame or escape, or contain multiple frames. The decoder
//! retains partial state and validates the FCS before exposing a frame. Data
//! between delimiters that is entirely `FF` is fill, not a frame: after a lost
//! delimiter the decoder treats the next `BB` as an opening one instead.
use serde::Serialize;
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Frame {
    pub control: u8,
    pub payload: Vec<u8>,
}
impl Frame {
    /// View an ordinary `48` payload after its leading routing byte. Retain
    /// the original payload when correlating a reply with a target/session.
    /// Other controls keep their complete payload: in particular, `A8` fast
    /// bridge records contain an addressed slot that must not be stripped.
    pub fn normalized(&self) -> &[u8] {
        if self.control == 0x48 {
            self.payload.get(1..).unwrap_or_default()
        } else {
            &self.payload
        }
    }
}
pub fn encode(control: u8, payload: &[u8]) -> Vec<u8> {
    let mut data = vec![control];
    data.extend_from_slice(payload);
    let fcs = 0u8.wrapping_sub(data.iter().fold(0u8, |a, &b| a.wrapping_add(b)));
    data.push(fcs);
    let mut out = vec![0xbb];
    for b in data {
        if matches!(b, 0xbb | 0xbd) {
            out.extend_from_slice(&[0xbd, b ^ 0x20]);
        } else {
            out.push(b);
        }
    }
    out.push(0xbb);
    if out.len() < 128 {
        out.resize(128, 0xff);
    }
    out
}
#[derive(Default)]
pub struct Decoder {
    active: bool,
    escaped: bool,
    data: Vec<u8>,
}
impl Decoder {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Result<Frame, String>> {
        let mut out = Vec::new();
        for &b in bytes {
            if !self.active {
                if b == 0xbb {
                    self.active = true;
                    self.data.clear();
                    self.escaped = false;
                }
                continue;
            }
            if b == 0xbb {
                if self.escaped {
                    out.push(Err("truncated escape".into()));
                } else if self.data.iter().all(|&x| x == 0xff) {
                    // Empty, or fill collected after losing sync: this BB opens.
                    self.data.clear();
                    continue;
                } else if self.data.len() < 2 {
                    out.push(Err("short frame".into()));
                } else if self.data.iter().fold(0u8, |a, &x| a.wrapping_add(x)) != 0 {
                    out.push(Err("invalid FCS".into()));
                } else {
                    out.push(Ok(Frame {
                        control: self.data[0],
                        payload: self.data[1..self.data.len() - 1].to_vec(),
                    }));
                }
                let closed = !self.data.is_empty() || self.escaped;
                self.data.clear();
                self.escaped = false;
                self.active = !closed;
                continue;
            }
            if self.escaped {
                if !matches!(b, 0x9b | 0x9d) {
                    out.push(Err("invalid escape".into()));
                    self.active = false;
                    self.data.clear();
                    self.escaped = false;
                    continue;
                }
                self.data.push(b ^ 0x20);
                self.escaped = false;
            } else if b == 0xbd {
                self.escaped = true;
            } else {
                self.data.push(b);
            }
            if self.data.len() > 65536 {
                out.push(Err("frame exceeds 65536 bytes".into()));
                self.active = false;
                self.data.clear();
                self.escaped = false;
            }
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn golden() {
        let b = encode(0x48, &[0x0d, 0x16, 0x7c, 0]);
        assert_eq!(&b[..8], &[0xbb, 0x48, 0x0d, 0x16, 0x7c, 0, 0x19, 0xbb]);
        assert_eq!(b.len(), 128);
        assert!(b[8..].iter().all(|&x| x == 0xff));
    }
    #[test]
    fn boundaries() {
        for n in 0..300 {
            let p = vec![1; n];
            let b = encode(2, &p);
            assert_eq!(b.len(), (n + 4).max(128));
        }
    }
    #[test]
    fn every_split_and_byte() {
        let p: Vec<u8> = (0..=255).collect();
        let b = encode(0x48, &p);
        for split in 0..=b.len() {
            let mut d = Decoder::default();
            let mut frames = d.feed(&b[..split]);
            frames.extend(d.feed(&b[split..]));
            assert_eq!(
                frames,
                vec![Ok(Frame {
                    control: 0x48,
                    payload: p.clone()
                })]
            );
        }
    }
    #[test]
    fn coalesced_and_checksum() {
        let mut b = encode(3, &[4]);
        b.extend(encode(5, &[6]));
        let mut d = Decoder::default();
        assert_eq!(d.feed(&b).len(), 2);
        let mut c = encode(3, &[4]);
        c[2] = 5;
        assert!(d.feed(&c)[0].is_err());
    }
}

#[cfg(test)]
mod robustness {
    use super::*;
    #[test]
    fn checksum_is_escaped() {
        for control in [0x43, 0x45] {
            let b = encode(control, &[]);
            assert_eq!(b[2], 0xbd);
            assert_eq!(
                Decoder::default().feed(&b),
                vec![Ok(Frame {
                    control,
                    payload: vec![]
                })]
            );
        }
    }
    #[test]
    fn frame_can_contain_ff_and_consecutive_delimiters() {
        let mut b = vec![0x31, 0xff, 0xbb, 0xbb];
        b.extend(encode(0xff, &[0xff]));
        assert_eq!(
            Decoder::default().feed(&b),
            vec![Ok(Frame {
                control: 0xff,
                payload: vec![0xff]
            })]
        );
    }
    fn padded_replies() -> Vec<u8> {
        let mut b = Vec::new();
        for payload in [[0x30, 1], [0x3a, 2], [0x3a, 3]] {
            b.extend(encode(0x88, &payload));
        }
        b
    }
    fn decoded(frames: Vec<Result<Frame, String>>) -> Vec<Frame> {
        frames.into_iter().filter_map(Result::ok).collect()
    }
    #[test]
    fn resyncs_on_ff_fill_after_a_bad_escape() {
        let mut d = Decoder::default();
        let mut b = vec![0xbb, 0x48, 0xbd, 0x00, 0x01, 0xbb];
        b.resize(128, 0xff);
        b.extend(padded_replies());
        let frames = d.feed(&b);
        assert_eq!(frames[0], Err("invalid escape".into()));
        assert_eq!(decoded(frames).len(), 3);
    }
    #[test]
    fn resyncs_on_ff_fill_when_the_stream_starts_mid_frame() {
        let mut b = encode(0x88, &[0x70, 0, 0, 0x78])[3..].to_vec();
        b.extend(padded_replies());
        let frames = Decoder::default().feed(&b);
        assert!(frames.iter().all(Result::is_ok), "{frames:?}");
        assert_eq!(decoded(frames).len(), 3);
    }
    #[test]
    fn malformed_escape_and_overflow_recover() {
        let mut d = Decoder::default();
        assert!(d.feed(&[0xbb, 0x48, 0xbd, 0xbb])[0].is_err());
        assert!(d.feed(&encode(1, &[2]))[0].is_ok());
        let mut b = vec![0xbb];
        b.resize(65539, 1);
        assert!(d.feed(&b).iter().any(Result::is_err));
        assert!(d.feed(&encode(1, &[2]))[0].is_ok());
    }
}

#[cfg(test)]
mod normalization {
    use super::*;
    #[test]
    fn cargo_reply_and_fast_routing_remain_distinct() {
        let bytes = hex::decode("bb480d167e30302d43524700aebb").unwrap();
        let f = Decoder::default().feed(&bytes).remove(0).unwrap();
        assert_eq!(f.payload[0], 13);
        assert_eq!(
            f.normalized(),
            &[0x16, 0x7e, 0x30, 0x30, 0x2d, 0x43, 0x52, 0x47, 0]
        );
        let fast = Frame {
            control: 0xa8,
            payload: f.payload.clone(),
        };
        assert_eq!(fast.normalized(), f.payload);
    }
}
