use super::*;
use std::{cell::RefCell, rc::Rc};

#[derive(Default)]
struct State {
    mode: u8,
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
            (1, 0x3c) => reply.extend([6, 5, 4, 3, 2, 1, 0xff]),
            (1, 0x1c) => reply.extend([0x23, 0, 0xff, 0xff]),
            (0x16, 0x7c) => reply.extend(b"00-CRG\0"),
            (1, 0x2c) => reply.extend(if state.mode == 9 {
                [0x41, 0, 0]
            } else {
                [0x41, 10, 0]
            }),
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
        target_slot: Some(13),
        adapter_supply: false,
        unit_auth_attempted: false,
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
