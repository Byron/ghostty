//! Unit-level Criterion benchmarks; no PTY, session, font, renderer, or app.
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rustty_vt::{Cell, Color, Row, Screen, ScrollbackLimits, Terminal, unicode};
use serde::Deserialize;
use std::{hint::black_box, path::Path, process::Command, time::Duration};

const COLS: u16 = 128;
const ROWS: u16 = 32;
const PATTERNS: [(&str, &str); 4] = [
    ("ascii", "abcdefgh"),
    ("chinese", "天地玄黄宇宙洪荒"),
    ("combining", "a\u{301}b\u{302}c\u{303}d\u{308}"),
    ("emoji", "👩\u{200d}💻👨\u{200d}🚀"),
];
const OPERATIONS: [&str; 9] = [
    "width",
    "print",
    "scalar",
    "read",
    "clone",
    "reflow",
    "feed",
    "stream",
    "stream_styled",
];
const HISTORY_LINES: usize = 1_024;
const PRIME_BATCHES: usize = 32;
const STREAM_RECORDS: usize = 32;

fn is_stream(operation: &str) -> bool {
    matches!(operation, "stream" | "stream_styled")
}

fn input(operation: &str, name: &str, text: &str) -> String {
    if operation == "feed" {
        return format!("\x1b[H{text}");
    }
    if !is_stream(operation) {
        return text.to_owned();
    }
    let pattern = PATTERNS.iter().find(|&&(n, _)| n == name).unwrap().1;
    // 192 display columns: one full row and a half row before CRLF.
    // Width is stated explicitly because ZWJ clusters are narrower than the
    // sum of their scalar widths.
    let repeats = match name {
        "ascii" => 24,
        "chinese" => 12,
        _ => 48,
    };
    let record = pattern.repeat(repeats);
    let mut result = String::new();
    for line in 0..STREAM_RECORDS {
        if operation == "stream_styled" {
            result.push_str(&format!(
                "\x1b[{};{}m",
                if line % 2 == 0 { 1 } else { 22 },
                31 + line % 4,
            ));
        }
        result.push_str(&record);
        if operation == "stream_styled" {
            result.push_str("\x1b[0m");
        }
        result.push_str("\r\n");
    }
    result
}

fn stream_checksum(screen: &Screen) -> u64 {
    let mut checksum = 0_u64;
    for row in &screen.rows {
        for (col, cell) in row.cells.iter().enumerate() {
            checksum = checksum.wrapping_mul(16_777_619);
            checksum = checksum.wrapping_add(cell_sum(screen, row, col));
            if cell.codepoint.is_some() {
                let color = match cell.style.foreground {
                    Color::Default => 0,
                    Color::Indexed(index) => u64::from(index) + 1,
                    Color::Rgb(..) => panic!("unexpected benchmark style"),
                };
                checksum =
                    checksum.wrapping_add((color + 257 * u64::from(cell.style.bold)) * 0x11_0000);
            }
        }
    }
    checksum
}

fn check_stream(terminal: &Terminal, name: &str, styled: bool) -> u64 {
    let screen = terminal.screen();
    assert!(!screen.history.is_empty());
    assert!(screen.history.len() <= HISTORY_LINES);
    assert_eq!(screen.cursor.row, usize::from(ROWS) - 1);
    assert_eq!(screen.cursor.col, 0);
    assert!(!screen.cursor.pending_wrap);
    // The active screen ends with 15 complete records, the preceding record's
    // final 64 columns, and one empty row. Check against the input scalars,
    // independently of either engine's printing implementation.
    let pattern = PATTERNS.iter().find(|&&(n, _)| n == name).unwrap().1;
    let repeats = match name {
        "ascii" => 24,
        "chinese" => 12,
        _ => 48,
    };
    assert_eq!(
        text_sum(screen),
        pattern.chars().map(u64::from).sum::<u64>() * (15 * repeats + repeats / 3),
    );
    for (row_index, row) in screen.rows.iter().enumerate() {
        let record = 16 + row_index.div_ceil(2);
        for cell in row.cells.iter().filter(|cell| cell.codepoint.is_some()) {
            assert_eq!(
                cell.style.foreground,
                if styled {
                    Color::Indexed(1 + (record % 4) as u8)
                } else {
                    Color::Default
                },
            );
            assert_eq!(cell.style.bold, styled && record % 2 == 0);
        }
    }
    stream_checksum(screen)
}

fn setup_stream(bytes: &[u8]) -> Terminal {
    let mut terminal = Terminal::with_limits(
        COLS,
        ROWS,
        ScrollbackLimits {
            bytes: None,
            lines: Some(HISTORY_LINES),
        },
    );
    terminal.feed(b"\x1b[?2027h");
    // Emit 2,048 physical rows: warm allocations and force history eviction
    // before timing, rather than measuring unbounded initial growth.
    for _ in 0..PRIME_BATCHES {
        assert!(terminal.feed(bytes).is_empty());
    }
    terminal
}

// Only these two read adapters differ when copying this harness to the earlier
// String-backed implementation. The timed workloads stay identical.
fn cell_sum(screen: &Screen, row: &Row, col: usize) -> u64 {
    screen.cell_text(row, col).chars().map(u64::from).sum()
}

fn scalar(cell: &Cell) -> u64 {
    cell.codepoint.map_or(0, u64::from)
}

fn scalar_sum(screen: &Screen) -> u64 {
    screen
        .rows
        .iter()
        .flat_map(|row| &row.cells)
        .map(scalar)
        .sum()
}

fn text_sum(screen: &Screen) -> u64 {
    screen
        .rows
        .iter()
        .map(|row| {
            (0..row.cells.len())
                .map(|col| cell_sum(screen, row, col))
                .sum::<u64>()
        })
        .sum()
}

fn width_sum(codepoints: &[char]) -> u64 {
    codepoints
        .iter()
        .copied()
        .map(unicode::codepoint_width)
        .map(u64::from)
        .sum()
}

fn overwrite(terminal: &mut Terminal, codepoints: &[char]) {
    let cursor = &mut terminal.screen_mut().cursor;
    cursor.row = 0;
    cursor.col = 0;
    cursor.pending_wrap = false;
    for &cp in codepoints {
        terminal.print(cp);
    }
}

fn setup(codepoints: &[char]) -> Terminal {
    let mut terminal = Terminal::with_limits(COLS, ROWS, ScrollbackLimits::NONE);
    terminal.feed(b"\x1b[?2027h");
    overwrite(&mut terminal, codepoints);
    let expected = codepoints.iter().copied().map(u64::from).sum::<u64>();
    assert_eq!(
        text_sum(terminal.screen()),
        expected,
        "corpus must fit without losing scalars"
    );
    assert_eq!(text_sum(&terminal.screen().snapshot_viewport()), expected);
    terminal
}

#[derive(Deserialize)]
struct NativeResult {
    engine: String,
    operation: String,
    iterations: u64,
    elapsed_ns: u64,
    units_per_iteration: u64,
    cell_bytes: usize,
    checksum: u64,
}

fn native(
    binary: &Path,
    operation: &str,
    data: &Path,
    iterations: u64,
    units: u64,
    checksum: u64,
) -> Duration {
    let output = Command::new(binary)
        .arg(operation)
        .arg(data)
        .arg(iterations.to_string())
        .output()
        .expect("run the headless Ghostty primitive benchmark");
    assert!(
        output.status.success(),
        "Ghostty {operation}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: NativeResult =
        serde_json::from_slice(&output.stdout).expect("native benchmark JSON");
    assert_eq!(result.engine, "ghostty");
    assert_eq!(result.operation, operation);
    assert_eq!(result.iterations, iterations);
    assert_eq!(result.units_per_iteration, units);
    assert_eq!(
        result.checksum, checksum,
        "engines must process identical content"
    );
    assert_eq!(result.cell_bytes, 8);
    assert!(result.elapsed_ns > 0);
    // Criterion measures the native loop's clock. Process launch, input reads,
    // setup, validation, IPC and destruction of the terminal are all excluded.
    Duration::from_nanos(result.elapsed_ns)
}

fn primitives(c: &mut Criterion) {
    let ghostty = std::env::var_os("GHOSTTY_PRIMITIVES_BIN").map(std::path::PathBuf::from);
    let corpora: Vec<_> = PATTERNS
        .iter()
        .map(|&(name, pattern)| {
            let text = pattern.repeat(128);
            let codepoints = text.chars().collect::<Vec<_>>();
            (name, text, codepoints)
        })
        .collect();
    // Prepare the exact same UTF-8 corpora for native measurements before any
    // timers start. Each process owns this tiny scratch directory.
    let data_dir = ghostty.as_ref().map(|_| {
        let directory =
            std::env::temp_dir().join(format!("rustty-primitives-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    });
    eprintln!("Rustty Cell: {} bytes", size_of::<Cell>());
    for engine in ["rustty", "ghostty"] {
        if engine == "ghostty" && ghostty.is_none() {
            continue;
        }
        for operation in OPERATIONS {
            let mut group = c.benchmark_group(format!("{engine}/{operation}"));
            for (name, text, codepoints) in &corpora {
                let input = input(operation, name, text);
                let mut terminal = if is_stream(operation) {
                    setup_stream(input.as_bytes())
                } else {
                    setup(codepoints)
                };
                let expected_text = text_sum(terminal.screen());
                let checksum = match operation {
                    "stream" | "stream_styled" => {
                        check_stream(&terminal, name, operation == "stream_styled")
                    }
                    "width" => width_sum(codepoints),
                    "scalar" => scalar_sum(terminal.screen()),
                    _ => expected_text,
                };
                let units = match operation {
                    "width" | "print" => codepoints.len() as u64,
                    "reflow" => 1,
                    "feed" | "stream" | "stream_styled" => input.len() as u64,
                    _ => u64::from(COLS) * u64::from(ROWS),
                };
                group.throughput(
                    if matches!(operation, "feed" | "stream" | "stream_styled") {
                        Throughput::Bytes(units)
                    } else {
                        Throughput::Elements(units)
                    },
                );
                if engine == "ghostty" {
                    let binary = ghostty.as_ref().unwrap();
                    let data = data_dir
                        .as_ref()
                        .unwrap()
                        .join(format!("{operation}-{name}.txt"));
                    std::fs::write(&data, &input).unwrap();
                    group.bench_function(*name, |b| {
                        b.iter_custom(|iterations| {
                            native(binary, operation, &data, iterations, units, checksum)
                        })
                    });
                } else {
                    // Dispatch outside the hot loop and make inputs/results
                    // opaque to the optimizer. Allocator counting stays in its
                    // separate test so it cannot penalize allocating revisions.
                    group.bench_function(*name, |b| match operation {
                        "width" => b.iter(|| black_box(width_sum(black_box(codepoints)))),
                        "print" => {
                            b.iter(|| overwrite(black_box(&mut terminal), black_box(codepoints)))
                        }
                        "feed" | "stream" | "stream_styled" => b.iter(|| {
                            black_box(black_box(&mut terminal).feed(black_box(input.as_bytes())))
                        }),
                        "scalar" => b.iter(|| black_box(scalar_sum(black_box(terminal.screen())))),
                        "read" => b.iter(|| black_box(text_sum(black_box(terminal.screen())))),
                        "clone" => b.iter(|| {
                            let copy = black_box(terminal.screen()).snapshot_viewport();
                            black_box(&copy);
                            // Copy destruction is included on both engines.
                            drop(copy);
                        }),
                        "reflow" => b.iter(|| {
                            let terminal = black_box(&mut terminal);
                            terminal.resize(COLS / 2, ROWS);
                            terminal.resize(COLS, ROWS);
                        }),
                        _ => unreachable!(),
                    });
                }
                if is_stream(operation) {
                    assert_eq!(
                        check_stream(&terminal, name, operation == "stream_styled"),
                        checksum
                    );
                }
                assert_eq!(
                    text_sum(terminal.screen()),
                    expected_text,
                    "operation changed text"
                );
            }
            group.finish();
        }
    }
    if let Some(directory) = data_dir {
        std::fs::remove_dir_all(directory).unwrap();
    }
}

criterion_group!(benches, primitives);
criterion_main!(benches);
