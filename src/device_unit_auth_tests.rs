use super::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct State {
    mode: u8,
    series: Option<u8>,
    firmware: Option<[u8; 3]>,
    params: Option<Vec<u8>>,
    pending: VecDeque<u8>,
    writes: Vec<Vec<u8>>,
}
struct AuthPort(Rc<RefCell<State>>);
impl Read for AuthPort {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut state = self.0.borrow_mut();
        let n = buf.len().min(state.pending.len());
        for byte in &mut buf[..n] {
            *byte = state.pending.pop_front().unwrap();
        }
        Ok(n)
    }
}
impl Write for AuthPort {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let frame = Decoder::default().feed(bytes).remove(0).unwrap();
        let p = frame.payload;
        assert_eq!(frame.control, 0x48);
        assert_eq!(p[0], 13);
        let mut state = self.0.borrow_mut();
        state.writes.push(p.clone());
        let mut reply = vec![0, p[1], p[2] + 2];
        match (p[1], p[2]) {
            (1, 0x3c) => reply.extend(if state.series.is_some() {
                [1, 2, 3, 4, 5, 6, 0xff]
            } else {
                [6, 5, 4, 3, 2, 1, 0xff]
            }),
            (1, 0x1c) => reply.extend([
                if state.mode == 9 {
                    0x99
                } else {
                    state.series.unwrap_or(0x23)
                },
                0,
                0xff,
                0xff,
            ]),
            (0x16, 0x7c) => reply.extend(state.params.clone().unwrap_or_else(|| {
                if state.series == Some(0x35) {
                    b"01\0\0\0\0\0".to_vec()
                } else if state.series.is_some() {
                    b"00\0\0\0\0\0".to_vec()
                } else {
                    b"00-CRG\0".to_vec()
                }
            })),
            (1, 0x2c) => reply.extend(state.firmware.unwrap_or([0x41, 10, 0])),
            (1, 0xa4) => reply.extend(if state.mode == 10 {
                &b"short"[..]
            } else {
                &b"\x0bPREFX"[..]
            }),
            (1, 0xac) => reply.extend(if state.mode == 11 {
                b"ABCDE "
            } else {
                b"ABCDEF"
            }),
            (0x33, 0x68) => {
                let index = fragment_index(p[3]);
                assert_eq!(p, modern_request(0x68, index));
                if index != 2 {
                    return Ok(bytes.len());
                }
                for (index, mut fragment) in modern_challenge().into_iter().enumerate() {
                    if index == 0 {
                        match state.mode {
                            1 => fragment[3] = 0x26,
                            2 => {
                                fragment.pop();
                            }
                            3 => fragment[2] = 0x6b,
                            _ => {}
                        }
                    }
                    state.pending.extend(protocol::encode(0x48, &fragment));
                }
                return Ok(bytes.len());
            }
            (0x33, 0x70) => {
                let index = fragment_index(p[3]);
                assert_eq!(p, modern_request(0x70, index));
                if index != 2 {
                    return Ok(bytes.len());
                }
                match state.mode {
                    4 => {
                        reply[2] = 0x73;
                        reply.extend([0x45, 0, 0xff, 0]);
                    }
                    5 => reply.extend([0, 0]),
                    6 => reply.extend([0x45, 0, 0xff, 0]),
                    7 => reply.extend([0xff; 7]),
                    _ => reply.extend([0xff; 4]),
                }
            }
            (0x16, 0xd8) => {
                let mut fragments = vec![
                    vec![0, 0x16, 0xda, 0x16, 0, 1, 2, 3, 4, 5],
                    vec![0, 0x16, 0xda, 0x26, 6, 7, 8, 9, 10, 11],
                    vec![0, 0x16, 0xda, 0x34, 12, 13, 14, 15, 0xff, 0xff],
                ];
                match state.mode {
                    1 => fragments[0][3] = 0x26,
                    2 => {
                        fragments[0].pop();
                    }
                    3 => fragments = vec![vec![0, 0x16, 0xdb, 0x46, 0, 0xff, 0xff]],
                    _ => {}
                }
                for f in fragments {
                    state.pending.extend(protocol::encode(0x48, &f));
                }
                return Ok(bytes.len());
            }
            (0x16, 0xe0) => {
                if p[3] != 0x34 {
                    return Ok(bytes.len());
                }
                match state.mode {
                    4 => {
                        reply[2] = 0xe3;
                        reply.extend([0x45, 0, 0xff, 0]);
                    }
                    5 => reply.extend([0, 0]),
                    6 => reply.extend([0x45, 0, 0xff, 0]),
                    _ => reply.extend([0xff; 4]),
                }
            }
            (0x32, 0x10) => reply.extend([0, 0, 0xff, 0xff]),
            _ => panic!("unexpected command"),
        }
        state.pending.extend(protocol::encode(0x48, &reply));
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Port for AuthPort {
    fn set_timeout(&mut self, _: Duration) -> std::io::Result<()> {
        Ok(())
    }
}
fn fragment_index(selector: u8) -> usize {
    match selector {
        0x16 => 0,
        0x26 => 1,
        0x34 => 2,
        _ => panic!("invalid selector"),
    }
}
fn modern_block_frames(address: u8, opcode: u8, hex: &str) -> Vec<Vec<u8>> {
    let block = hex::decode(hex).unwrap();
    [(0x16, 0, 6), (0x26, 6, 6), (0x34, 12, 4)]
        .into_iter()
        .map(|(selector, offset, length)| {
            let mut frame = vec![address, 0x33, opcode, selector];
            frame.extend_from_slice(&block[offset..offset + length]);
            frame.resize(10, 0xff);
            frame
        })
        .collect()
}
fn modern_request(opcode: u8, index: usize) -> Vec<u8> {
    let block = if opcode == 0x68 {
        "718dd37093b36dc5c414917637a03804"
    } else {
        "5501d4b4f69bc199ac0e846be1f85fee"
    };
    modern_block_frames(13, opcode, block)[index].clone()
}
fn modern_challenge() -> Vec<Vec<u8>> {
    modern_block_frames(0, 0x6a, "63dfcbda01f8b42165be9d869752794f")
}
fn session(mode: u8) -> (Session, Rc<RefCell<State>>) {
    let state = Rc::new(RefCell::new(State {
        mode,
        ..State::default()
    }));
    let session = Session {
        port: Box::new(AuthPort(state.clone())),
        decoder: Decoder::default(),
        incoming: VecDeque::new(),
        trace: None,
        pc_slot: 0,
        normal_slot: Some(13),
        normal_is_motor: true,
        target_slot: Some(13),
        adapter_supply: false,
        unit_auth_attempted: false,
        discovered_since_initialize: false,
    };
    (session, state)
}
#[test]
fn wired_auth_matches_independent_fixture_and_redacts_exchange() {
    let (mut s, state) = session(0);
    let path =
        std::env::temp_dir().join(format!("cranklink-wired-auth-{}.jsonl", std::process::id()));
    s.trace = Some(
        OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&path)
            .unwrap(),
    );
    assert_eq!(
        s.authenticate_unit_settings("060504030201").unwrap()["unit_authentication_receipt"],
        true
    );
    let vector = serde_json::json!({
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
    });
    let writes: Vec<_> = state
        .borrow()
        .writes
        .iter()
        .filter(|p| matches!(p[2], 0xd8 | 0xe0))
        .cloned()
        .collect();
    assert_eq!(
        writes[0][3..],
        hex::decode(vector["auth_key_hex"].as_str().unwrap()).unwrap()
    );
    for (actual, expected) in writes[1..]
        .iter()
        .zip(vector["e0_parameters_hex"].as_array().unwrap())
    {
        assert_eq!(
            actual[3..],
            hex::decode(expected.as_str().unwrap()).unwrap()
        );
    }
    let count = state.borrow().writes.len();
    assert!(s.authenticate_unit_settings("060504030201").is_err());
    assert_eq!(count, state.borrow().writes.len());
    s.normal_stop().unwrap();
    drop(s);
    let trace = std::fs::read_to_string(&path).unwrap();
    assert!(trace.contains("authentication-verified"));
    for field in ["auth_key_hex", "proof_hex", "challenge_hex"] {
        if let Some(secret) = vector[field].as_str() {
            assert!(!trace.contains(secret));
        }
    }
    for line in trace.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        if row["direction"] == "tx" || row["direction"] == "rx" {
            let bytes = hex::decode(row["bytes"].as_str().unwrap()).unwrap();
            for frame in Decoder::default().feed(&bytes).into_iter().flatten() {
                assert!(
                    !(frame.payload[1] == 0x16
                        && matches!(frame.payload[2], 0xd8 | 0xda | 0xe0 | 0xe2))
                );
            }
        }
    }
    std::fs::remove_file(path).unwrap();
}
#[test]
fn wired_auth_identity_and_exchange_failures_stop_without_retry_or_settings() {
    for mode in [1, 2, 3, 4, 5, 6, 9] {
        let (mut s, state) = session(mode);
        assert!(s.authenticate_unit_settings("060504030201").is_err());
        let writes = state.borrow().writes.clone();
        if mode <= 3 || mode == 9 {
            assert!(!writes.iter().any(|p| p[2] == 0xe0));
        }
        if mode == 9 {
            assert!(!writes.iter().any(|p| p[2] == 0xd8));
        }
        if mode != 9 {
            let count = writes.len();
            assert!(s.authenticate_unit_settings("060504030201").is_err());
            assert_eq!(count, state.borrow().writes.len());
        }
        s.normal_stop().unwrap();
    }
    let (mut s, state) = session(0);
    assert!(s.authenticate_unit_settings("010203040506").is_err());
    assert_eq!(state.borrow().writes.len(), 1);
}

#[test]
fn wired_modern_auth_matches_vectors_for_supported_motor_families() {
    for (series, firmware) in [
        (0x30, [0x44, 1, 0]),
        (0x35, [0x46, 0, 1]),
        (0x37, [0x41, 1, 0]),
    ] {
        let (mut session, state) = session(0);
        state.borrow_mut().series = Some(series);
        state.borrow_mut().firmware = Some(firmware);
        let receipt = session.authenticate_unit_settings("010203040506").unwrap();
        assert_eq!(receipt["unit_authentication_receipt"], true);
        assert_eq!(receipt["protocol"], "modern");
        let auth: Vec<_> = state
            .borrow()
            .writes
            .iter()
            .filter(|p| p[1] == 0x33)
            .cloned()
            .collect();
        assert_eq!(
            auth,
            [0x68, 0x70]
                .into_iter()
                .flat_map(|opcode| (0..3).map(move |index| modern_request(opcode, index)))
                .collect::<Vec<_>>()
        );
        let count = state.borrow().writes.len();
        assert!(session.authenticate_unit_settings("010203040506").is_err());
        assert_eq!(state.borrow().writes.len(), count);
        session.normal_stop().unwrap();
    }
}

#[test]
fn wired_modern_auth_rejects_malformed_exchanges_without_retry_or_settings() {
    for mode in [1, 2, 3, 4, 5, 6, 7, 9, 10, 11] {
        let (mut session, state) = session(mode);
        state.borrow_mut().series = Some(0x30);
        assert!(
            session.authenticate_unit_settings("010203040506").is_err(),
            "mode {mode}"
        );
        let writes = state.borrow().writes.clone();
        if [1, 2, 3, 9, 10, 11].contains(&mode) {
            assert!(!writes.iter().any(|p| p[1..3] == [0x33, 0x70]));
        }
        if [9, 10, 11].contains(&mode) {
            assert!(!writes.iter().any(|p| p[1] == 0x33));
        } else {
            assert!(session.authenticate_unit_settings("010203040506").is_err());
            assert_eq!(state.borrow().writes.len(), writes.len());
        }
        assert!(!writes
            .iter()
            .any(|p| p[1] == 0x16 && matches!(p[2], 0xd8 | 0xe0 | 0xa8 | 0xb0)));
        session.normal_stop().unwrap();
    }
    let (mut session, state) = session(0);
    state.borrow_mut().series = Some(0x30);
    assert!(session.authenticate_unit_settings("FFFFFFFFFFFF").is_err());
    assert_eq!(state.borrow().writes.len(), 1);
}

#[test]
fn wired_modern_auth_keeps_credentials_out_of_success_and_failure_traces() {
    for mode in [0, 4] {
        let (mut session, state) = session(mode);
        state.borrow_mut().series = Some(0x30);
        let path = std::env::temp_dir().join(format!(
            "cranklink-wired-modern-auth-{}-{mode}.jsonl",
            std::process::id()
        ));
        session.trace = Some(
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
                .unwrap(),
        );
        assert_eq!(
            session.authenticate_unit_settings("010203040506").is_ok(),
            mode == 0
        );
        assert_eq!(session.trace.is_some(), mode == 0);
        session.normal_stop().unwrap();
        drop(session);
        let trace = std::fs::read_to_string(&path).unwrap();
        assert_eq!(trace.contains("authentication-verified"), mode == 0);
        for secret in [
            "718dd37093b36dc5c414917637a03804",
            "63dfcbda01f8b42165be9d869752794f",
            "5501d4b4f69bc199ac0e846be1f85fee",
        ] {
            assert!(!trace.contains(secret));
        }
        for line in trace.lines() {
            let row: serde_json::Value = serde_json::from_str(line).unwrap();
            if row["direction"] == "tx" || row["direction"] == "rx" {
                let bytes = hex::decode(row["bytes"].as_str().unwrap()).unwrap();
                for frame in Decoder::default().feed(&bytes).into_iter().flatten() {
                    assert!(frame.payload[1] != 0x33);
                }
            }
        }
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn wired_legacy_auth_accepts_other_firmware_patch_revisions_in_known_family() {
    let (mut session, state) = session(0);
    state.borrow_mut().firmware = Some([0x41, 11, 7]);
    let receipt = session.authenticate_unit_settings("060504030201").unwrap();
    assert_eq!(receipt["unit_authentication_receipt"], true);
    assert_eq!(receipt["firmware"], "4.1.11.7");
    assert_eq!(receipt["protocol"], "legacy");
    session.normal_stop().unwrap();
}

#[test]
fn wired_older_families_respect_authorization_introduction_without_protocol_guessing() {
    for (series, params, firmware, required) in [
        (0x12, [0; 7], [0x46, 99, 0], false),
        (0x12, [0; 7], [0x47, 0, 0], true),
        (0x21, [0; 7], [0x43, 99, 0], false),
        (0x21, [0; 7], [0x44, 0, 0], true),
        (0x20, [0x10, 0, 0, 0, 0, 0, 0], [0x43, 99, 0], false),
        (0x20, [0x10, 0, 0, 0, 0, 0, 0], [0x44, 0, 0], true),
        (0x20, [0x18, 1, 0, 0, 0, 0, 0], [0x40, 0, 0], true),
        (0x38, [0x10, 0, 0, 0, 0, 0, 0], [0x41, 0, 0], true),
        (0x08, [0; 7], [0x33, 0, 0], false),
    ] {
        let (mut session, state) = session(0);
        state.borrow_mut().series = Some(series);
        state.borrow_mut().params = Some(params.to_vec());
        state.borrow_mut().firmware = Some(firmware);
        let receipt = session.authenticate_unit_settings("010203040506").unwrap();
        let writes = state.borrow().writes.clone();
        let auth: Vec<_> = writes.iter().filter(|p| p[1..3] == [0x16, 0xd8]).collect();
        assert!(!writes.iter().any(|p| p[1] == 0x33));
        if required {
            assert_eq!(receipt["protocol"], "legacy");
            assert_eq!(auth.len(), 1);
            assert_eq!(&auth[0][3..], &hex::decode("3072389bc4278a").unwrap());
            assert_eq!(writes.iter().filter(|p| p[1..3] == [0x16, 0xe0]).count(), 3);
        } else {
            assert_eq!(receipt["unit_authentication_required"], false);
            assert!(auth.is_empty());
            assert!(!writes.iter().any(|p| p[1..3] == [0x16, 0xe0]));
        }
        let count = state.borrow().writes.len();
        assert!(session.authenticate_unit_settings("010203040506").is_err());
        assert_eq!(state.borrow().writes.len(), count);
        session.normal_stop().unwrap();
    }
}

#[test]
fn wired_ambiguous_e6000_sends_no_authentication_material() {
    let (mut session, state) = session(0);
    state.borrow_mut().series = Some(0x08);
    state.borrow_mut().params = Some(vec![0; 7]);
    state.borrow_mut().firmware = Some([0x34, 0, 0]);
    let error = session
        .authenticate_unit_settings("010203040506")
        .unwrap_err();
    assert!(error.to_string().contains("unambiguous"));
    assert!(!state
        .borrow()
        .writes
        .iter()
        .any(|p| { p[1] == 0x33 || (p[1] == 0x16 && matches!(p[2], 0xd8 | 0xe0)) }));
    session.normal_stop().unwrap();
}

#[test]
fn wired_no_auth_required_still_requires_active_session_and_matching_serial() {
    for inactive in [false, true] {
        let (mut session, state) = session(0);
        state.borrow_mut().series = Some(0x12);
        state.borrow_mut().params = Some(vec![0; 7]);
        state.borrow_mut().firmware = Some([0x46, 99, 0]);
        if inactive {
            session.normal_slot = None;
        }
        assert!(session.authenticate_unit_settings("FFFFFFFFFFFF").is_err());
        assert_eq!(state.borrow().writes.len(), usize::from(!inactive));
        assert!(!session.unit_auth_attempted);
    }
}
