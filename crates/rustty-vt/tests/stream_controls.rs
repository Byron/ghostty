use rustty_vt::Terminal;

#[test]
fn raw_c1_controls_execute_cursor_and_protection_actions_inside_sequences() {
    let mut terminal = Terminal::new(10, 6, 0);
    terminal.feed(b"\x1b[3;4H\x1b[3\x8d");
    assert_eq!(
        (terminal.screen().cursor.row, terminal.screen().cursor.col),
        (1, 3)
    );
    terminal.feed(b"\x1b[3\x84");
    assert_eq!(
        (terminal.screen().cursor.row, terminal.screen().cursor.col),
        (2, 3)
    );
    terminal.feed(b"\x1b[3\x85");
    assert_eq!(
        (terminal.screen().cursor.row, terminal.screen().cursor.col),
        (3, 0)
    );
    terminal.feed(b"\x1b[3\x96X\x1b[3\x97Y");
    let screen = terminal.screen();
    assert!(screen.rows[3].cells[0].protected);
    assert!(!screen.rows[3].cells[1].protected);
    assert!(!screen.cursor.protected);
    terminal.feed(b"\x1b[2K");
    assert_eq!(terminal.screen().rows[3].text(), "X");
}

#[test]
fn ground_state_c1_bytes_keep_utf8_decoding_semantics() {
    let mut terminal = Terminal::new(10, 6, 0);
    terminal.feed(b"\x1b[3;4H\xc2\x8d\xc2\x96X\x8d\x96");
    assert_eq!(
        (terminal.screen().cursor.row, terminal.screen().cursor.col),
        (2, 6)
    );
    assert!(!terminal.screen().cursor.protected);
    assert_eq!(terminal.screen().rows[2].text(), "   X\u{fffd}\u{fffd}");
}
