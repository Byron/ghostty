use rustty_vt::{Color, CursorShape, Effect, Selection, Terminal, Underline};

fn lines(t: &Terminal) -> Vec<String> {
    t.screen().rows.iter().map(|row| row.text()).collect()
}

fn invariant(t: &Terminal) {
    let screen = t.screen();
    assert_eq!(screen.rows.len(), t.rows as usize);
    assert!(screen.cursor.row < t.rows as usize);
    assert!(screen.cursor.col < t.cols as usize);
    for row in screen.all_rows() {
        assert_eq!(row.cells.len(), t.cols as usize);
        for (i, cell) in row.cells.iter().enumerate() {
            if cell.width == 0 {
                assert!(i > 0 && row.cells[i - 1].width == 2);
            }
            if cell.width == 2 {
                assert!(i + 1 < row.cells.len() && row.cells[i + 1].width == 0);
            }
            assert!(cell.text.chars().count() <= 65);
        }
    }
}

#[test]
fn wrapping_scrolling_and_tracked_references() {
    let mut t = Terminal::new(4, 2, 2);
    t.feed(b"abcd");
    assert_eq!((t.screen().cursor.row, t.screen().cursor.col), (0, 3));
    assert!(t.screen().cursor.pending_wrap);
    let point = t.screen().point(0, 1).unwrap();
    let pin = t.screen_mut().track(point);
    t.feed(b"efghijkl");
    assert_eq!(lines(&t), ["efgh", "ijkl"]);
    assert_eq!(t.screen().history[0].text(), "abcd");
    assert_eq!(t.screen().resolve(pin), Some(point));
    t.feed(b"mnopqrstuvwx");
    assert_eq!(t.screen().resolve(pin), None);
    assert_eq!(t.screen().history.len(), 2);
    invariant(&t);
}

#[test]
fn vt_overwrites_wide_cells_as_a_unit() {
    let mut t = Terminal::new(4, 2, 10);
    t.feed("abc界".as_bytes());
    assert!(t.screen().rows[0].cells[3].spacer_head);
    assert!(t.screen().rows[0].wrapped);
    assert_eq!(t.screen().rows[1].cells[0].width, 2);
    t.feed(b"\x1b[2;2HX");
    assert!(t.screen().rows[1].cells[0].text.is_empty());
    assert_eq!(t.screen().rows[1].cells[1].text, "X");
    invariant(&t);
    t.feed("\x1b[H界\x1b[2GX".as_bytes());
    invariant(&t);
}

#[test]
fn graphemes_have_bounded_storage_and_track_width() {
    let mut t = Terminal::new(6, 2, 10);
    t.feed("a\u{301}".as_bytes());
    assert_eq!(t.screen().rows[0].cells[0].text, "a\u{301}");
    t.feed(b"\x1b[?2027h");
    t.feed("👩🏽‍🚀".as_bytes());
    assert_eq!(t.screen().rows[0].cells[1].text, "👩🏽‍🚀");
    assert_eq!(t.screen().rows[0].cells[1].width, 2);
    for _ in 0..200 {
        t.feed("\u{301}".as_bytes());
    }
    invariant(&t);
    let mut t = Terminal::new(3, 3, 10);
    t.feed("\x1b[?2027hab❤\u{fe0f}".as_bytes());
    assert_eq!(t.screen().rows[1].cells[0].text, "❤\u{fe0f}");
    assert_eq!(t.screen().rows[1].cells[0].width, 2);
    invariant(&t);
}

#[test]
fn erase_retains_background_but_clears_other_style_and_protects_cells() {
    let mut t = Terminal::new(5, 2, 0);
    t.feed(b"\x1b[1;31;44mabc\x1b[2K");
    for c in &t.screen().rows[0].cells {
        assert_eq!(c.style.background, Color::Indexed(4));
        assert!(!c.style.bold);
        assert_eq!(c.style.foreground, Color::Default);
    }
    t.feed(b"\x1b[H\x1b[1\"qA\x1b[0\"qB\x1b[?2K");
    assert_eq!(t.screen().rows[0].cells[0].text, "A");
    assert!(t.screen().rows[0].cells[1].text.is_empty());
}

#[test]
fn alternate_screen_modes_preserve_the_primary() {
    let mut t = Terminal::new(6, 2, 10);
    t.feed(b"main\x1b[?1049h\x1b[Hother\x1b[?1049l");
    assert_eq!(lines(&t)[0], "main");
    assert_eq!(t.screen().cursor.col, 4);
    t.feed(b"\x1b[?47h");
    assert_eq!(lines(&t)[0], "other");
    t.feed(b"\x1b[?1047l\x1b[?47h");
    assert!(lines(&t).iter().all(|line| line.is_empty()));
    invariant(&t);
}

#[test]
fn margins_scroll_only_the_defined_region() {
    let mut t = Terminal::new(5, 4, 10);
    t.feed(b"one\r\ntwo\r\nthree\r\nfour");
    t.feed(b"\x1b[2;3r\x1b[3;1H\n");
    assert_eq!(lines(&t), ["one", "three", "", "four"]);
    assert!(t.screen().history.is_empty());
    t.feed(b"\x1b[?6h\x1b[1;2HX");
    assert_eq!(t.screen().rows[1].cells[1].text, "X");
    invariant(&t);
}

#[test]
fn reflow_preserves_text_selection_and_reference() {
    let mut t = Terminal::new(6, 3, 100);
    t.feed("ab界cdefgh".as_bytes());
    let start = t.screen().point(0, 0).unwrap();
    let end = t.screen().point(1, 3).unwrap();
    t.screen_mut().selection = Some(Selection {
        start,
        end,
        rectangular: false,
    });
    let before = t.screen().selection_text().unwrap();
    let pin = t.screen_mut().track(start);
    t.resize(4, 4);
    assert_eq!(t.screen().selection_text().unwrap(), before);
    assert_eq!(t.screen().resolve(pin).unwrap().col, 0);
    invariant(&t);
    t.resize(10, 4);
    assert_eq!(t.screen().selection_text().unwrap(), before);
    invariant(&t);
}

#[test]
fn effects_and_terminal_replies_are_ordered() {
    let mut t = Terminal::new(80, 24, 100);
    let e = t.feed(
        b"\x1b]2;test\x07\x1b[4;3H\x1b[6n\x07\x1b]133;C\x07\x1b]133;D;2\x07\x1b]52;c;aGVsbG8=\x07",
    );
    assert_eq!(
        e,
        [
            Effect::Title("test".into()),
            Effect::Write(b"\x1b[4;3R".to_vec()),
            Effect::Bell,
            Effect::CommandStart,
            Effect::CommandEnd { exit_code: Some(2) },
            Effect::Clipboard {
                selection: "c".into(),
                data: Some(b"hello".to_vec())
            }
        ]
    );
    assert_eq!(
        t.feed(b"\x1b[?117$p"),
        [Effect::Write(b"\x1b[?117;4$y".to_vec())]
    );
}

#[test]
fn sgr_colon_subparameters_and_cursor_shapes() {
    let mut t = Terminal::new(10, 2, 0);
    t.feed(b"\x1b[38:2::12:34:56;4:3;58:5:128mX\x1b[5 q");
    let c = &t.screen().rows[0].cells[0];
    assert_eq!(c.style.foreground, Color::Rgb(12, 34, 56));
    assert_eq!(c.style.underline, Underline::Curly);
    assert_eq!(c.style.underline_color, Color::Indexed(128));
    assert_eq!(t.screen().cursor.shape, CursorShape::Bar);
    assert!(t.screen().cursor.blink);
}

#[test]
fn chunk_boundaries_do_not_change_terminal_results() {
    let bytes = "one\r\n\x1b[38;2;50;60;70m界\x1b[?2027h👩🏽‍🚀\x1b]8;;https://example.org\x1b\\link\x1b]8;;\x07\x1b[6n".as_bytes();
    let mut full = Terminal::new(10, 4, 30);
    let effects = full.feed(bytes);
    for size in 1..=bytes.len() {
        let mut split = Terminal::new(10, 4, 30);
        let mut actual = Vec::new();
        for chunk in bytes.chunks(size) {
            actual.extend(split.feed(chunk));
            invariant(&split);
        }
        assert_eq!(actual, effects);
        assert_eq!(split.screen().rows, full.screen().rows);
        assert_eq!(split.screen().cursor, full.screen().cursor);
    }
}

#[test]
fn generated_edits_keep_wide_cells_and_cursor_in_bounds() {
    let choices = [
        "界", "a", "👩", "\u{301}", "\r", "\n", "\x1b[2K", "\x1b[1P", "\x1b[2@", "\x1b[1D",
        "\x1b[4C", "\x1b[2X", "\x1b[2J", "\x1b[H", "\x1b[M", "\x1b[L",
    ];
    let mut t = Terminal::new(7, 4, 20);
    let mut state: u64 = 0x12345678;
    for n in 0..2000 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        t.feed(choices[state as usize % choices.len()].as_bytes());
        if n % 31 == 0 {
            t.resize(2 + (state % 9) as u16, 2 + ((state >> 4) % 5) as u16);
        }
        invariant(&t);
    }
}
