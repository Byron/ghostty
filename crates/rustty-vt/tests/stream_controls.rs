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

#[test]
fn invalid_csi_parameter_counts_leave_terminal_state_unchanged() {
    let mut terminal = Terminal::new(20, 6, 10);
    terminal.feed(b"first\r\nsecond\x1b[?69h\x1b[2;18s\x1b[2;5r\x1b[3;5H\x1b[1\"q\x1b[>4;2m");
    let before = rustty_vt::snapshot::encode_to_vec(&terminal).unwrap();
    let mut commands: Vec<String> = "@ABCDEFGIJKLMPSTWXZ`abdeg"
        .chars()
        .map(|byte| format!("1;3{byte}"))
        .collect();
    commands.extend(
        [
            "1;2;3H", "1;2;3f", "1;2;3r", "1;2;3s", "?1;3J", "?1;3K", "0;1\"q", ">0;0;0m", "g",
        ]
        .map(String::from),
    );
    for command in commands {
        assert!(
            terminal
                .feed(format!("\x1b[{command}").as_bytes())
                .is_empty()
        );
        assert!(
            rustty_vt::snapshot::encode_to_vec(&terminal).unwrap() == before,
            "CSI {command} changed state despite an invalid parameter count"
        );
    }
}

#[test]
fn csi_parameter_validation_preserves_optional_and_variable_length_commands() {
    let mut terminal = Terminal::new(20, 6, 0);
    terminal.feed(b"\x1b[3;4H\x1b7\x1b[E");
    assert_eq!(
        (terminal.screen().cursor.row, terminal.screen().cursor.col),
        (3, 0)
    );
    terminal.feed(b"\x1b[1;2;3u\x1b[1;31m\x1b[?1;25l");
    assert_eq!(
        (terminal.screen().cursor.row, terminal.screen().cursor.col),
        (2, 3)
    );
    assert!(terminal.screen().cursor.style.bold);
    assert_eq!(
        terminal.screen().cursor.style.foreground,
        rustty_vt::Color::Indexed(1)
    );
    assert!(!terminal.modes.dec(1));
    assert!(!terminal.modes.dec(25));
}
