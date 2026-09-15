//! Unit-level Criterion benchmarks; no PTY, session, font, renderer, or app.
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use rustty_vt::{Cell, Row, Screen, ScrollbackLimits, Terminal, unicode};
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
const OPERATIONS: [&str; 6] = ["width", "print", "scalar", "read", "clone", "reflow"];

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
        for (name, text, _) in &corpora {
            std::fs::write(directory.join(format!("{name}.txt")), text).unwrap();
        }
        directory
    });
    eprintln!("Rustty Cell: {} bytes", size_of::<Cell>());
    for engine in ["rustty", "ghostty"] {
        if engine == "ghostty" && ghostty.is_none() {
            continue;
        }
        for operation in OPERATIONS {
            let mut group = c.benchmark_group(format!("{engine}/{operation}"));
            for (name, _, codepoints) in &corpora {
                let mut terminal = setup(codepoints);
                let expected_text = text_sum(terminal.screen());
                let checksum = match operation {
                    "width" => width_sum(codepoints),
                    "scalar" => scalar_sum(terminal.screen()),
                    _ => expected_text,
                };
                let units = match operation {
                    "width" | "print" => codepoints.len() as u64,
                    "reflow" => 1,
                    _ => u64::from(COLS) * u64::from(ROWS),
                };
                group.throughput(Throughput::Elements(units));
                if engine == "ghostty" {
                    let binary = ghostty.as_ref().unwrap();
                    let data = data_dir.as_ref().unwrap().join(format!("{name}.txt"));
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
