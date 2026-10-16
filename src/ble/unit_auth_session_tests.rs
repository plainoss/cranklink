//! Full session tests use fixed expected wire packets, independently of the
//! implementation's key derivation and encryption helpers.
use super::*;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

#[derive(Default)]
struct State {
    writes: Vec<Vec<u8>>,
    pending: VecDeque<Notification>,
    mode: u8,
    wrong_model: bool,
    old_version: bool,
    firmware_4110: bool,
    padded_receipt: bool,
}
struct Mock(Rc<RefCell<State>>);
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
        panic!("no GATT reads in unit auth")
    }
    fn subscribe(&mut self, _: u16) -> Result<()> {
        panic!("session already active")
    }
    fn passkey(&mut self) -> Result<String> {
        panic!("gateway already authenticated")
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
            (2, 0x14) => {
                assert_eq!(&bytes[3..], &[13]);
                reply.extend([13, 0x23, 0]);
            }
            (1, 0x3c) => reply.extend([6, 5, 4, 3, 2, 1, 0xff]),
            (0x16, 0x7c) => reply.extend(if s.wrong_model {
                b"00-STD\0"
            } else {
                b"00-CRG\0"
            }),
            (1, 0x2c) => reply.extend(if s.firmware_4110 {
                [0x41, 10, 0]
            } else if s.old_version {
                [0x41, 0, 0]
            } else {
                [0x42, 0, 0]
            }),
            (0x16, 0xd8) => {
                let mode = s.mode;
                if mode == 4 {
                    return Ok(());
                } // timeout, no challenge
                if mode == 3 {
                    reply[2] = 0xdb;
                    reply.push(1);
                } else {
                    let fragments = [
                        vec![13, 0x16, 0xda, 0x16, 0, 1, 2, 3, 4, 5],
                        vec![13, 0x16, 0xda, 0x26, 6, 7, 8, 9, 10, 11],
                        vec![13, 0x16, 0xda, 0x34, 12, 13, 14, 15, 0xff, 0xff],
                    ];
                    for (index, mut fragment) in fragments.into_iter().enumerate() {
                        if mode == 1 && index == 0 {
                            fragment[3] = 0x26;
                        }
                        if mode == 2 && index == 0 {
                            fragment.pop();
                        }
                        s.pending.push_back(Notification {
                            characteristic: FREE_RESPONSE,
                            value: fragment,
                        });
                    }
                    return Ok(());
                }
            }
            (0x16, 0xe0) => {
                if bytes[3] != 0x34 {
                    return Ok(());
                }
                match s.mode {
                    5 => {
                        reply[2] = 0xe3;
                        reply.extend([0x45, 0, 0xff, 0]);
                    }
                    6 => return Ok(()), // timeout after all three proof writes
                    7 => reply.extend([0, 0]), // malformed success length
                    8 => reply.extend([0x45, 0, 0xff, 0]), // error payload with forged normal opcode
                    _ => {
                        reply.extend([0xff; 4]);
                        if s.padded_receipt {
                            reply.extend([0x19, 0x83, 0x42]);
                        }
                    }
                }
            }
            _ => panic!("unexpected unit operation {}", hex::encode(bytes)),
        }
        s.pending.push_back(Notification {
            characteristic: FREE_RESPONSE,
            value: reply,
        });
        Ok(())
    }
}
fn session() -> (BleSession, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State::default()));
    let mut session = BleSession::new(Box::new(Mock(state.clone())), None).unwrap();
    session.authenticated = true;
    session.gateway_mode = true;
    session.pc_slot = 0;
    session.linked_slot = Some(13);
    session.target_slot = Some(13);
    (session, state)
}
fn fixture() -> serde_json::Value {
    serde_json::json!({
        "serial_value_hex": "010203040506",
        "serial_wire_hex": "060504030201",
        "auth_key_hex": "ce7784983cf94c",
        "update_key_hex": "296a5c64004c870bb97006b09cfa2417",
        "challenge_hex": "000102030405060708090a0b0c0d0e0f",
        "proof_hex": "c1c2e41b71333f93090513f5816f8627",
        "e0_parameters_hex": [
            "16c1c2e41b7133",
            "263f93090513f5",
            "34816f8627ffff"
        ]
    })
}
fn auth_writes(state: &Rc<RefCell<State>>, opcode: u8) -> Vec<Vec<u8>> {
    state
        .borrow()
        .writes
        .iter()
        .filter(|bytes| bytes[1] == 0x16 && bytes[2] == opcode)
        .cloned()
        .collect()
}
#[test]
fn inactive_or_unlinked_session_never_sends_unit_authentication() {
    for variant in 0..3 {
        let (mut s, state) = session();
        match variant {
            0 => s.authenticated = false,
            1 => s.gateway_mode = false,
            _ => s.linked_slot = None,
        }
        assert!(s.authenticate_unit_settings("060504030201").is_err());
        assert!(state.borrow().writes.is_empty());
    }
}
#[test]
fn full_exchange_matches_independent_numeric_serial_fixture_in_wire_order() {
    let (mut s, state) = session();
    let vector = fixture();
    assert_eq!(vector["serial_value_hex"], "010203040506");
    let result = s.authenticate_unit_settings("060504030201").unwrap();
    assert_eq!(result["unit_authentication_receipt"], true);
    let expected = hex::decode(vector["auth_key_hex"].as_str().unwrap()).unwrap();
    assert_eq!(
        auth_writes(&state, 0xd8),
        vec![[vec![13, 0x16, 0xd8], expected].concat()]
    );
    let proofs: Vec<_> = vector["e0_parameters_hex"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            [
                vec![13, 0x16, 0xe0],
                hex::decode(v.as_str().unwrap()).unwrap(),
            ]
            .concat()
        })
        .collect();
    assert_eq!(auth_writes(&state, 0xe0), proofs);
    assert!(!s.poisoned);
    let count = state.borrow().writes.len();
    assert!(s.authenticate_unit_settings("060504030201").is_err());
    assert_eq!(
        state.borrow().writes.len(),
        count,
        "no second handshake on an existing session"
    );
}
#[test]
fn wrong_serial_or_unsupported_model_version_sends_no_authentication_material() {
    for variant in 0..3 {
        let (mut s, state) = session();
        state.borrow_mut().wrong_model = variant == 1;
        state.borrow_mut().old_version = variant == 2;
        let serial = if variant == 0 {
            "010203040506"
        } else {
            "060504030201"
        };
        assert!(s.authenticate_unit_settings(serial).is_err());
        assert!(auth_writes(&state, 0xd8).is_empty());
        assert!(auth_writes(&state, 0xe0).is_empty());
    }
}
#[test]
fn invalid_order_length_rejection_or_challenge_timeout_never_sends_proof_or_retries() {
    for mode in 1..=4 {
        let (mut s, state) = session();
        state.borrow_mut().mode = mode;
        assert!(s.authenticate_unit_settings("060504030201").is_err());
        assert!(s.poisoned);
        assert_eq!(auth_writes(&state, 0xd8).len(), 1);
        assert!(auth_writes(&state, 0xe0).is_empty());
        let count = state.borrow().writes.len();
        assert!(s.authenticate_unit_settings("060504030201").is_err());
        assert_eq!(state.borrow().writes.len(), count);
    }
}
#[test]
fn rejected_malformed_or_timed_out_proof_fences_session_without_repeating_writes() {
    for mode in 5..=8 {
        let (mut s, state) = session();
        state.borrow_mut().mode = mode;
        assert!(s.authenticate_unit_settings("060504030201").is_err());
        assert!(s.poisoned);
        assert_eq!(auth_writes(&state, 0xd8).len(), 1);
        assert_eq!(auth_writes(&state, 0xe0).len(), 3);
        let count = state.borrow().writes.len();
        assert!(s.authenticate_unit_settings("060504030201").is_err());
        assert_eq!(state.borrow().writes.len(), count);
        assert!(s.command(13, 0x10, 0x14, &[0], 0x16).is_err());
        assert_eq!(state.borrow().writes.len(), count);
    }
}
#[test]
fn authentication_credentials_challenge_and_proof_are_excluded_from_traces() {
    let (mut s, _) = session();
    let path = std::env::temp_dir().join(format!(
        "cranklink-unit-auth-session-{}-{}.jsonl",
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
    s.authenticate_unit_settings("060504030201").unwrap();
    drop(s);
    let trace = std::fs::read_to_string(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(
        !trace.is_empty(),
        "ordinary identity operations should still be traceable"
    );
    let vector = fixture();
    for key in [
        "auth_key_hex",
        "update_key_hex",
        "challenge_hex",
        "proof_hex",
    ] {
        assert!(!trace.contains(vector[key].as_str().unwrap()));
    }
    for line in trace.lines() {
        let value: serde_json::Value = serde_json::from_str(line).unwrap();
        let bytes = hex::decode(value["bytes"].as_str().unwrap()).unwrap();
        assert!(
            !(bytes.len() >= 3
                && bytes[1] == 0x16
                && matches!(bytes[2], 0xd8 | 0xda | 0xdb | 0xe0 | 0xe2 | 0xe3))
        );
    }
}

#[test]
fn stock_4110_uses_legacy_auth_and_accepts_only_transport_padded_success() {
    let (mut s, state) = session();
    state.borrow_mut().firmware_4110 = true;
    state.borrow_mut().padded_receipt = true;
    let result = s.authenticate_unit_settings("060504030201").unwrap();
    assert_eq!(result["firmware"], "4.1.10.0");
    assert_eq!(result["reply_parameters_hex"], "ffffffff");
    assert!(state
        .borrow()
        .writes
        .iter()
        .any(|w| w[1..3] == [0x16, 0xd8]));
    assert!(!state.borrow().writes.iter().any(|w| w[1] == 0x33));
    assert!(s.authenticate_unit_settings("060504030201").is_err());
}

#[test]
fn ble_receipt_padding_cannot_hide_errors_wrong_routes_or_short_status() {
    assert!(unit_auth::validate_ble_receipt(
        &[0, 0x16, 0xe2, 255, 255, 255, 255, 1, 2, 3],
        0,
        0x16,
        0xe2
    )
    .is_ok());
    for bytes in [
        vec![0, 0x16, 0xe3, 255, 255, 255, 255, 1, 2, 3],
        vec![1, 0x16, 0xe2, 255, 255, 255, 255, 1, 2, 3],
        vec![0, 0x16, 0xe2, 0, 255, 255, 255, 1, 2, 3],
        vec![0, 0x16, 0xe2, 255, 255, 255, 255, 1],
    ] {
        assert!(unit_auth::validate_ble_receipt(&bytes, 0, 0x16, 0xe2).is_err());
    }
}
