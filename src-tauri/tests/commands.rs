use streamy_desktop::commands::{clamp_title, looks_like_pe_exe};

#[test]
fn clamp_title_caps_length_and_keeps_utf8() {
    let long = "a".repeat(500);
    let c = clamp_title(long);
    assert_eq!(c.chars().count(), 200);
    // Multi-byte char at the boundary is not split.
    let s = "é".repeat(250);
    let c = clamp_title(s);
    assert!(c.chars().all(|ch| ch == 'é'));
    assert_eq!(c.chars().count(), 200);
}

#[test]
fn clamp_title_edge_cases() {
    // Exactly 200 chars — no truncation, no allocation.
    let s = "x".repeat(200);
    let c = clamp_title(s.clone());
    assert_eq!(c, s);
    // Empty string stays empty.
    assert_eq!(clamp_title(String::new()), "");
    // Short string unchanged.
    assert_eq!(clamp_title("hello".into()), "hello");
}

#[test]
fn pe_exe_check_valid_minimal_pe() {
    // Minimal 68-byte PE: MZ header, e_lfanew = 0x40 at offset 0x3C, PE\0\0 at 0x40.
    let mut buf = vec![0u8; 68];
    buf[0] = b'M';
    buf[1] = b'Z';
    // e_lfanew at 0x3C: u32 LE = 0x40
    buf[0x3C] = 0x40;
    buf[0x3D] = 0x00;
    buf[0x3E] = 0x00;
    buf[0x3F] = 0x00;
    // PE\0\0 at offset 0x40
    buf[0x40] = b'P';
    buf[0x41] = b'E';
    buf[0x42] = 0x00;
    buf[0x43] = 0x00;
    assert!(looks_like_pe_exe(&buf));
}

#[test]
fn pe_exe_check_rejects_non_pe() {
    // HTML response (small, no MZ).
    assert!(!looks_like_pe_exe(b"<html>"));
    // Empty buffer.
    assert!(!looks_like_pe_exe(&[]));
    // 2-byte MZ only — too short for the full PE check.
    assert!(!looks_like_pe_exe(b"MZ"));
    // MZ header present but e_lfanew points to garbage (not PE\0\0).
    let mut buf = vec![0u8; 68];
    buf[0] = b'M';
    buf[1] = b'Z';
    // e_lfanew = 0x40 but no PE\0\0 written.
    buf[0x3C] = 0x40;
    assert!(!looks_like_pe_exe(&buf));
}
