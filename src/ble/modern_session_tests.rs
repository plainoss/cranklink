//! Fixed-vector coverage for modern motor authentication sessions.
use super::*;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

const SERIAL: &str = "010203040506";
const AUTH: &str = "718dd37093b36dc5c414917637a03804";
const CHALLENGE: &str = "63dfcbda01f8b42165be9d869752794f";
const PROOF: &str = "5501d4b4f69bc199ac0e846be1f85fee";
#[derive(Default)]
struct State {
    writes: Vec<Vec<u8>>,
    pending: VecDeque<Notification>,
    series: u8,
    mode: u8,
    identity_fault: u8,
    padded_product: bool,
}
struct Mock(Rc<RefCell<State>>);
fn frames(op: u8, data: &str) -> Vec<Vec<u8>> {
    let data = hex::decode(data).unwrap();
    [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)]
        .into_iter()
        .map(|(tag, start, len)| {
            let mut frame = vec![13, 0x33, op, tag];
            frame.extend(&data[start..start + len]);
            frame.resize(10, 0xff);
            frame
        })
        .collect()
}
impl Gatt for Mock {
    fn characteristics(&self) -> Vec<CharacteristicInfo> {
        [FREE_COMMAND, PCE_COMMAND]
            .into_iter()
            .map(|id| CharacteristicInfo {
                id,
                read: false,
                write: true,
                write_without_response: false,
                notify: false,
                indicate: false,
            })
            .collect()
    }
    fn read(&mut self, _: u16) -> Result<Vec<u8>> {
        panic!("unexpected read")
    }
    fn subscribe(&mut self, _: u16) -> Result<()> {
        panic!("already active")
    }
    fn passkey(&mut self) -> Result<String> {
        panic!("already authenticated")
    }
    fn close(&mut self) -> Result<()> {
        Ok(())
    }
    fn receive(&mut self, _: Duration) -> Result<Option<Notification>> {
        Ok(self.0.borrow_mut().pending.pop_front())
    }
    fn write(&mut self, id: u16, bytes: &[u8], without_response: bool) -> Result<()> {
        assert!(!without_response);
        if id == PCE_COMMAND {
            assert_eq!(&bytes[..2], &[0, 6]);
            assert!([0, 13].contains(&bytes[2]));
            self.0.borrow_mut().pending.push_back(Notification {
                characteristic: PCE_RESPONSE,
                value: vec![0x26, 0],
            });
            return Ok(());
        }
        assert_eq!(id, FREE_COMMAND);
        assert_eq!(bytes[0], if bytes[1..3] == [2, 0x14] { 0 } else { 13 });
        let mut s = self.0.borrow_mut();
        s.writes.push(bytes.to_vec());
        let mut reply = vec![bytes[0], bytes[1], bytes[2] + 2];
        match (bytes[1], bytes[2]) {
            (1, 0x3c) => reply.extend([1, 2, 3, 4, 5, 6]),
            (0x16, 0x7c) => reply.extend(b"00-STD\0"),
            (1, 0x2c) => reply.extend(if s.identity_fault == 1 {
                [0x46, 0, 0]
            } else if s.series == 0x37 {
                [0x41, 1, 0]
            } else {
                [0x46, 0, 1]
            }),
            (2, 0x14) => {
                assert_eq!(&bytes[3..], &[13]);
                reply.extend([13, s.series, if s.identity_fault == 2 { 1 } else { 0 }]);
            }
            (1, 0xa4) => {
                if s.identity_fault == 3 {
                    reply.extend(b"short");
                } else {
                    reply.extend(b"\x0bPREFX");
                }
            }
            (1, 0xac) => reply.extend(if s.identity_fault == 4 {
                b"ABCDE\0"
            } else {
                b"ABCDEF"
            }),
            (0x33, 0x68) => {
                let index = match bytes[3] {
                    0x16 => 0,
                    0x26 => 1,
                    0x34 => 2,
                    _ => panic!("selector"),
                };
                assert_eq!(bytes, frames(0x68, AUTH)[index]);
                if index != 2 || s.mode == 4 {
                    return Ok(());
                }
                for (i, mut frame) in frames(0x6a, CHALLENGE).into_iter().enumerate() {
                    frame[0] = 13;
                    if i == 0 {
                        match s.mode {
                            1 => frame[3] = 0x26,
                            2 => {
                                frame.pop();
                            }
                            3 => frame[2] = 0x6b,
                            _ => {}
                        }
                    }
                    s.pending.push_back(Notification {
                        characteristic: FREE_RESPONSE,
                        value: frame,
                    });
                }
                return Ok(());
            }
            (0x33, 0x70) => {
                let index = match bytes[3] {
                    0x16 => 0,
                    0x26 => 1,
                    0x34 => 2,
                    _ => panic!("selector"),
                };
                assert_eq!(bytes, frames(0x70, PROOF)[index]);
                // The first two proof fragments do not produce acknowledgments.
                if index != 2 || s.mode == 8 {
                    return Ok(());
                }
                match s.mode {
                    5 => reply.extend([0x45, 0, 0xff, 0]), // invalid error payload with normal opcode
                    6 => {
                        reply[2] = 0x73;
                        reply.extend([0x45, 0, 0xff, 0]);
                    }
                    7 => reply.extend([0, 0]),
                    _ => reply.extend([0xff; 4]),
                }
            }
            _ => panic!("unexpected operation {}", hex::encode(bytes)),
        }
        if s.padded_product && matches!((bytes[1], bytes[2]), (1, 0x3c | 0xa4 | 0xac)) {
            reply.push(0xff);
        }
        s.pending.push_back(Notification {
            characteristic: FREE_RESPONSE,
            value: reply,
        });
        Ok(())
    }
}
fn session(series: u8) -> (BleSession, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State {
        series,
        ..State::default()
    }));
    let mut session = BleSession::new(Box::new(Mock(state.clone())), None).unwrap();
    session.authenticated = true;
    session.gateway_mode = true;
    session.pc_slot = 0;
    session.linked_slot = Some(13);
    session.target_slot = Some(13);
    (session, state)
}
fn auth_writes(state: &Rc<RefCell<State>>, op: u8) -> Vec<Vec<u8>> {
    state
        .borrow()
        .writes
        .iter()
        .filter(|b| b[1..3] == [0x33, op])
        .cloned()
        .collect()
}
#[test]
fn modern_supported_profiles_match_independent_firmware_vectors_and_never_retry() {
    for series in [0x30, 0x35, 0x37] {
        let (mut s, state) = session(series);
        let result = s.authenticate_unit_settings(SERIAL).unwrap();
        assert_eq!(result["unit_authentication_receipt"], true);
        assert_eq!(result["hardware_validated"], false);
        assert_eq!(result["protocol"], "modern");
        assert_eq!(result["series"], series);
        assert_eq!(auth_writes(&state, 0x68), frames(0x68, AUTH));
        assert_eq!(auth_writes(&state, 0x70), frames(0x70, PROOF));
        assert!(!s.poisoned);
        let count = state.borrow().writes.len();
        assert!(s.authenticate_unit_settings(SERIAL).is_err());
        assert_eq!(state.borrow().writes.len(), count);
    }
}
#[test]
fn modern_product_serial_uses_only_six_characters_from_each_padded_reply() {
    let (mut s, state) = session(0x30);
    state.borrow_mut().padded_product = true;
    s.authenticate_unit_settings(SERIAL).unwrap();
    assert_eq!(auth_writes(&state, 0x68), frames(0x68, AUTH));
    assert_eq!(auth_writes(&state, 0x70), frames(0x70, PROOF));
}
#[test]
fn modern_identity_guards_send_no_credentials() {
    for fault in 0..=5 {
        let (mut s, state) = session(if fault == 5 { 0x23 } else { 0x30 });
        state.borrow_mut().identity_fault = fault;
        let expected = if fault == 0 { "000000000000" } else { SERIAL };
        assert!(s.authenticate_unit_settings(expected).is_err());
        assert!(auth_writes(&state, 0x68).is_empty());
        assert!(auth_writes(&state, 0x70).is_empty());
    }
}
#[test]
fn modern_challenge_errors_send_no_proof_and_fence_session() {
    for mode in 1..=4 {
        let (mut s, state) = session(0x30);
        state.borrow_mut().mode = mode;
        assert!(s.authenticate_unit_settings(SERIAL).is_err());
        assert!(s.poisoned);
        assert_eq!(auth_writes(&state, 0x68).len(), 3);
        assert!(auth_writes(&state, 0x70).is_empty());
        let count = state.borrow().writes.len();
        assert!(s.authenticate_unit_settings(SERIAL).is_err());
        assert!(s.command(13, 0x10, 0x14, &[0], 0x16).is_err());
        assert_eq!(state.borrow().writes.len(), count);
    }
}
#[test]
fn modern_final_status_rejection_malformed_and_timeout_fence_without_retry() {
    for mode in 5..=8 {
        let (mut s, state) = session(0x30);
        state.borrow_mut().mode = mode;
        assert!(s.authenticate_unit_settings(SERIAL).is_err());
        assert!(s.poisoned);
        assert_eq!(auth_writes(&state, 0x70).len(), 3);
        let count = state.borrow().writes.len();
        assert!(s.authenticate_unit_settings(SERIAL).is_err());
        assert!(s.command(13, 0x10, 0x14, &[0], 0x16).is_err());
        assert_eq!(state.borrow().writes.len(), count);
    }
}
#[test]
fn modern_authentication_frames_and_secrets_are_redacted_on_success_and_failure() {
    for mode in [0, 3, 5] {
        let (mut s, _) = session(0x30);
        let path = std::env::temp_dir().join(format!(
            "cranklink-modern-auth-{}-{}-{mode}.jsonl",
            std::process::id(),
            crate::platform::now_ms()
        ));
        s.trace = Some(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .unwrap(),
        );
        // Reuse the same mock while selecting a success or rejection outcome.
        // The mode is set via a fresh session below to avoid exposing trace internals.
        drop(s);
        let (mut s, state) = session(0x30);
        state.borrow_mut().mode = mode;
        s.trace = Some(OpenOptions::new().append(true).open(&path).unwrap());
        let _ = s.authenticate_unit_settings(SERIAL);
        drop(s);
        let trace = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert!(!trace.is_empty());
        for secret in [
            AUTH,
            CHALLENGE,
            PROOF,
            "64def4bb7e9c60d8da44768e405c3414",
            "ce11bad46590040ae495641bc3c52a99",
        ] {
            assert!(!trace.contains(secret));
        }
        for line in trace.lines() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            let bytes = hex::decode(value["bytes"].as_str().unwrap()).unwrap();
            assert!(
                !(bytes.len() >= 3
                    && bytes[1] == 0x33
                    && matches!(bytes[2], 0x68 | 0x6a | 0x6b | 0x70 | 0x72 | 0x73))
            );
        }
    }
}
