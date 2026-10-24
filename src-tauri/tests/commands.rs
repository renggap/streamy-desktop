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

/// RecState commit protocol, exercised as a pure state machine across every
/// interleaving the lock can produce. Mirrors the logic in start_recording /
/// Broadcast/recording commit protocol, tested against the REAL BcastState /
/// RecState enums (Idle/Connecting/Starting are unit variants, so no Tauri app
/// handle is needed). The previous version mirrored the logic in a local enum
/// and drifted from production — it kept passing while the stop-during-start
/// bug was live because it asserted the buggy behavior as the spec.
/// Connected/Active arms need real handles and stay covered by the command
/// paths plus these commit/phase assertions.
mod state_protocol {
    use streamy_desktop::commands::{
        bcast_commit, bcast_take_for_disconnect, rec_commit, rec_take_for_stop, BcastState, Commit,
        RecState,
    };

    #[test]
    fn disconnect_during_connect_wins_and_tears_down() {
        let mut g = BcastState::Connecting;
        assert!(bcast_take_for_disconnect(&mut g).is_none()); // no live handle yet
        assert!(
            matches!(g, BcastState::Idle),
            "a stop during Connecting must win: Connecting must not survive"
        );
        // The in-flight start must tear its fresh connection down at commit,
        // not go ON AIR with no consumer.
        assert!(matches!(bcast_commit(&g), Commit::TearDown));
    }

    #[test]
    fn connect_commits_when_undisturbed() {
        assert!(matches!(
            bcast_commit(&BcastState::Connecting),
            Commit::Store
        ));
    }

    #[test]
    fn stop_during_setup_honored() {
        let mut g = RecState::Starting;
        assert!(rec_take_for_stop(&mut g).is_none()); // no live take yet
        assert!(
            matches!(g, RecState::Idle),
            "a stop during Starting must win: Starting must not survive"
        );
        // start sees Idle at commit and tears the fresh take down (true).
        assert!(rec_commit(&g));
    }

    #[test]
    fn start_commits_when_undisturbed() {
        assert!(!rec_commit(&RecState::Starting));
    }

    #[test]
    fn stop_when_idle_is_noop() {
        let mut r = RecState::Idle;
        assert!(rec_take_for_stop(&mut r).is_none());
        assert!(matches!(r, RecState::Idle));

        let mut b = BcastState::Idle;
        assert!(bcast_take_for_disconnect(&mut b).is_none());
        assert!(matches!(b, BcastState::Idle));
    }
}
