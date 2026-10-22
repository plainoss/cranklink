use super::tests::{frame, scripted, scripted_motor};
use super::*;

// Each script lists every expected request in order; ScriptPort fails on any
// other command, so a script that ends empty pins the whole wire sequence.
const KEYS: [[u8; 3]; 5] = [
    [0x31, 0x41, 0x59],
    [0x26, 0x53, 0x58],
    [0x97, 0x93, 0x23],
    [0x84, 0x62, 0x64],
    [0x33, 0x83, 0x27],
];
/// Answer to a send-only request; Session::wait drops it.
fn idle() -> Frame {
    frame(0x88, &[0x70, 0, 0, 0x78])
}
fn key(i: u8) -> Frame {
    let k = KEYS[usize::from(i - 1)];
    frame(0x88, &[i, k[0], k[1], k[2]])
}
fn key_ack(i: u8) -> Frame {
    frame(0x88, &[0x11, i, 0, 0x19 + i])
}
fn update_protocol() -> Vec<(Frame, Frame)> {
    vec![
        (frame(3, &[0x60]), frame(0x23, &[0])),
        (frame(4, &[]), frame(0x24, &[0xe0])),
    ]
}
fn target(slot: u8) -> (Frame, Frame) {
    (frame(6, &[slot]), frame(0x26, &[0]))
}
fn identity(serial: [u8; 6]) -> Vec<(Frame, Frame)> {
    vec![
        (frame(0x88, &[0x2e, 0, 0, 0]), frame(0x88, &[0x3a, 0x50, 6])),
        (frame(0x88, &[0x2f, 0, 0, 0]), frame(0x88, &[0x3b, 1, 1])),
        (frame(0x88, &[0x30, 0, 0, 0]), frame(0x88, &[0x3c, 0x23, 0])),
        (
            frame(0x88, &[0x29, 0, 0, 0]),
            frame(0x88, &[0x35, serial[0], serial[1], serial[2]]),
        ),
        (
            frame(0x88, &[0x2a, 0, 0, 0]),
            frame(0x88, &[0x36, serial[3], serial[4], serial[5]]),
        ),
    ]
}
fn fast16(p: &[u8]) -> Vec<u8> {
    let mut v = vec![0u8; 16];
    v[..p.len()].copy_from_slice(p);
    v
}
fn slot_mask(command: u8, mask: u32) -> (Frame, Frame) {
    let mut p = vec![command];
    p.extend_from_slice(&mask.to_le_bytes());
    (frame(7, &p), frame(0x27, &[command, 0]))
}
fn adapter_mode(mode: u8) -> (Frame, Frame) {
    (frame(7, &[mode]), frame(0x27, &[mode, 0]))
}
fn fast_boot(slot: u8, command: u8, param: u8, reply: &[u8]) -> (Frame, Frame) {
    (
        frame(0xa8, &fast16(&[slot, command, param])),
        frame(0xa8, &fast16(reply)),
    )
}

#[test]
fn application_boot_entry_retains_slot_and_repeats_key5_after_handoff() {
    let mut script = vec![(
        frame(0x48, &[13, 1, 0xc0, 13]),
        frame(0x48, &[0, 1, 0xc2, 0]),
    )];
    script.extend(update_protocol());
    script.push(target(13));
    for i in 1..=4 {
        script.push((key(i), key_ack(i)));
    }
    script.push((key(5), idle()));
    script.extend(update_protocol());
    script.push(target(13));
    script.push((key(5), key_ack(5)));
    script.extend(identity([1, 2, 3, 4, 5, 6]));
    let (mut session, steps) = scripted_motor(13, script);
    // Application entry keeps the PC-linked slot; the recovery slot is unused.
    let boot = session.boot_start_at(true, Some(31)).unwrap();
    assert_eq!(boot.slot, 13);
    assert_eq!(boot.version, [5, 0, 6, 1]);
    assert_eq!((boot.board, boot.series, boot.unit), (1, 0x23, 0));
    assert_eq!(boot.serial, [1, 2, 3, 4, 5, 6]);
    assert!(session.normal_slot.is_none() && !session.normal_is_motor);
    assert!(steps.borrow().is_empty());
}

#[test]
fn recovery_boot_entry_uses_slot_31_and_sends_each_key_once() {
    let mut script = update_protocol();
    script.push(target(31));
    for i in 1..=5 {
        script.push((key(i), key_ack(i)));
    }
    script.extend(identity([1, 2, 3, 4, 5, 6]));
    let (mut session, steps) = scripted(script);
    let boot = session.boot_start_at(false, None).unwrap();
    assert_eq!(boot.slot, 31);
    assert!(steps.borrow().is_empty());
}

fn fast_prepare_script(slot: u8, board: u8) -> Vec<(Frame, Frame)> {
    let mut script = update_protocol();
    script.push(target(slot));
    for i in 1..=4 {
        script.push((key(i), key_ack(i)));
    }
    script.push((key(5), idle()));
    script.push(target(slot));
    script.push((key(5), key_ack(5)));
    script.push(slot_mask(0, 1));
    script.push(adapter_mode(5));
    script.push(target(slot));
    // The 4B receipt arrives only after the switch to 2 MHz (mode 6).
    script.push((
        frame(0x88, &[0x4b, 0, 0, 0]),
        frame(0xa8, &fast16(&[slot, 0x31])),
    ));
    script.push(adapter_mode(6));
    script.push(fast_boot(slot, 0x4c, 0xff, &[slot, 0x55, 0xff]));
    script.push(slot_mask(0, !(1u32 << slot)));
    script.push(slot_mask(1, 1u32 << slot));
    script.push(fast_boot(slot, 0x4c, slot, &[slot, 0x55, slot]));
    script.push(fast_boot(slot, 0x2f, 0, &[slot, 0x3b, 0, board]));
    script
}

#[test]
fn fast_prepare_programs_exact_slot_masks_and_checks_board() {
    let (mut session, steps) = scripted_motor(13, fast_prepare_script(13, 1));
    assert_eq!(session.fast_prepare(13).unwrap(), 13);
    assert!(session.normal_slot.is_none() && !session.normal_is_motor);
    assert!(steps.borrow().is_empty());
    // A disallowed board is refused with nothing sent after its query.
    let (mut session, steps) = scripted_motor(13, fast_prepare_script(13, 3));
    assert!(session
        .fast_prepare(13)
        .unwrap_err()
        .to_string()
        .contains("board mismatch"));
    assert!(steps.borrow().is_empty());
}

#[test]
fn fast_finish_restores_application_and_adapter_mode() {
    let (mut session, steps) = scripted(vec![
        fast_boot(13, 0x4c, 13, &[13, 0x55, 13]),
        fast_boot(13, 0x28, 0, &[13, 0x31]),
        slot_mask(0, 1),
        adapter_mode(5),
    ]);
    session.fast_finish(13).unwrap();
    assert!(steps.borrow().is_empty());
}
