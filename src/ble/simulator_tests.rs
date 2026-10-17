use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    thread,
};

fn peer(responses: Vec<Vec<u8>>) -> (SimulatorGatt, thread::JoinHandle<Vec<Value>>) {
    let (client, server) = UnixStream::pair().unwrap();
    client.set_write_timeout(Some(IO_TIMEOUT)).unwrap();
    let handle = thread::spawn(move || {
        let mut server = BufReader::new(server);
        let mut requests = Vec::new();
        for response in responses {
            let mut line = String::new();
            if server.read_line(&mut line).unwrap() == 0 {
                break;
            }
            requests.push(serde_json::from_str(&line).unwrap());
            if server.get_mut().write_all(&response).is_err() {
                break;
            }
        }
        requests
    });
    (
        SimulatorGatt {
            socket: BufReader::new(client),
            closed: false,
            credential: None,
        },
        handle,
    )
}
fn line(value: Value) -> Vec<u8> {
    let mut b = serde_json::to_vec(&value).unwrap();
    b.push(b'\n');
    b
}

#[test]
fn wire_operations_preserve_gatt_bytes_and_zero_poll_is_empty() {
    let (mut gatt, server) = peer(vec![
        line(json!({"ok":true,"bytes":[1,2,3]})),
        line(json!({"ok":true})),
        line(json!({"ok":true})),
        line(json!({"ok":true,"notification":null})),
        line(json!({"ok":true,"notification":{"characteristic":PCE_RESPONSE,"value":[36,128]}})),
        line(json!({"ok":true})),
    ]);
    assert_eq!(gatt.read(AUTH_NONCE).unwrap(), [1, 2, 3]);
    gatt.subscribe(PCE_RESPONSE).unwrap();
    gatt.write(PCE_COMMAND, &[0, 4], false).unwrap();
    assert!(gatt.receive(Duration::ZERO).unwrap().is_none());
    assert_eq!(
        gatt.receive(Duration::from_millis(50))
            .unwrap()
            .unwrap()
            .value,
        [36, 128]
    );
    assert_eq!(gatt.passkey().unwrap(), "000000");
    gatt.close().unwrap();
    gatt.close().unwrap();
    assert!(gatt.passkey().is_err());
    let requests = server.join().unwrap();
    assert_eq!(
        requests[2],
        json!({"op":"write","id":PCE_COMMAND,"bytes":[0,4],"without_response":false})
    );
    assert_eq!(requests[3], json!({"op":"receive","timeout_ms":0}));
    assert_eq!(requests.last().unwrap(), &json!({"op":"close"}));
}

#[test]
fn exactly_the_configuration_characteristics_are_advertised() {
    let (gatt, server) = peer(vec![]);
    let chars = gatt.characteristics();
    assert_eq!(
        chars.iter().map(|c| c.id).collect::<Vec<_>>(),
        [
            AUTH_CONTROL,
            AUTH_NONCE,
            PCE_RESPONSE,
            PCE_COMMAND,
            FREE_RESPONSE,
            FREE_COMMAND,
            FEATURE
        ]
    );
    assert!(chars
        .iter()
        .filter(|c| c.write)
        .all(|c| matches!(c.id, AUTH_CONTROL | PCE_COMMAND | FREE_COMMAND | FEATURE)));
    drop(gatt);
    server.join().unwrap();
    assert!(SimulatorGatt::open("relative.sock").is_err());
}

#[test]
fn malformed_eof_oversize_and_remote_errors_permanently_close_the_connection() {
    for response in [
        b"\n".to_vec(),
        b"{\"ok\":true".to_vec(),
        vec![b'x'; MAX_LINE + 1],
        line(json!({"ok":false,"error":"rejected"})),
        line(json!({"ok":true})),
        line(json!({"ok":true,"bytes":vec![0;21]})),
    ] {
        let (mut gatt, server) = peer(vec![response]);
        assert!(gatt.read(AUTH_NONCE).is_err());
        assert!(gatt.closed);
        assert!(gatt.write(PCE_COMMAND, &[0, 4], false).is_err());
        assert!(gatt.close().is_ok());
        server.join().unwrap();
    }
}

#[test]
fn transport_timeout_closes_without_retrying_the_operation() {
    let (client, mut server) = UnixStream::pair().unwrap();
    let mut gatt = SimulatorGatt {
        socket: BufReader::new(client),
        closed: false,
        credential: None,
    };
    let responder = thread::spawn(move || {
        let mut buf = [0; 128];
        use std::io::Read;
        let n = server.read(&mut buf).unwrap();
        thread::sleep(Duration::from_millis(60));
        assert!(n > 0);
    });
    assert!(gatt
        .rpc(
            json!({"op":"read","id":AUTH_NONCE}),
            Duration::from_millis(10)
        )
        .is_err());
    assert!(gatt.closed);
    assert!(gatt.subscribe(PCE_RESPONSE).is_err());
    responder.join().unwrap();
}

#[test]
fn invalid_notification_and_unconfirmed_close_are_terminal() {
    for notification in [
        json!({"characteristic":FEATURE,"value":[1]}),
        json!({"characteristic":FREE_RESPONSE,"value":vec![0;21]}),
    ] {
        let (mut gatt, server) = peer(vec![line(json!({"ok":true,"notification":notification}))]);
        assert!(gatt.receive(Duration::ZERO).is_err());
        assert!(gatt.closed);
        server.join().unwrap();
    }
    let (mut gatt, server) = peer(vec![line(json!({"ok":false,"error":"cleanup failed"}))]);
    assert!(gatt.close().is_err());
    assert!(gatt.closed);
    assert!(gatt.close().is_ok());
    server.join().unwrap();
}

#[test]
fn explicit_synthetic_passkey_is_honored_once_and_not_retained() {
    let (mut io, server) = peer(vec![]);
    io = io.with_passkey(Some("000001".to_owned()));
    assert_eq!(io.passkey().unwrap(), "000001");
    assert!(io.credential.is_none());
    assert_eq!(io.passkey().unwrap(), "000000");
    drop(io);
    server.join().unwrap();
}
