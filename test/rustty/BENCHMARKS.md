# Portable terminal primitive benchmarks

The Rustty benchmarks use Criterion and call the headless `rustty-vt` APIs
directly. They do not construct a session, PTY, font system, renderer, GPU, or
window. Criterion is a development dependency only. No application build is
needed, and timing does not include terminal construction or corpus decoding.

```sh
cargo bench --offline -p rustty-vt --bench primitives
# Filter a single primitive and corpus:
cargo bench --offline -p rustty-vt --bench primitives -- rustty/print/chinese
# Exercise every workload once, without collecting timing statistics:
cargo bench --offline -p rustty-vt --bench primitives -- --test
```

## Before and after

Keep the harness, input, compiler flags, and machine the same across revisions.
Build both revisions before measuring, then run them **serially** with other
builds and benchmarks stopped. Criterion supplies warmup, sampling, confidence
intervals, outlier analysis, and saved baseline comparisons.

```sh
cargo bench --offline -p rustty-vt --bench primitives -- --save-baseline before
# After the implementation change:
cargo bench --offline -p rustty-vt --bench primitives -- --baseline before
```

Set `CRITERION_HOME` to the same absolute directory when using separate
worktrees or target directories. Results otherwise live in `target/criterion`.
For an exploratory run, append `--sample-size 20 --warm-up-time 0.3
--measurement-time 1`; use the defaults for longer measurements.

To measure the earlier String-backed cells, copy this same benchmark to that
revision and adapt only `cell_sum` to read `row.cells[col].text`, and `scalar`
to read `cell.text.chars().next()`. All setup and measured operations stay the
same. The benchmark prints the compiled cell size to identify its layout.

## Ghostty comparison

The optional Zig executable imports only `libghostty-vt` and the Zig standard
library. It avoids the application dependencies and macOS instrumentation in
the general `ghostty-bench` runner. This target also cross-compiles to Linux.

```sh
zig build vt-primitives test-vt-primitives \
  -Demit-lib-vt=true -Demit-macos-app=false -Doptimize=ReleaseFast
GHOSTTY_PRIMITIVES_BIN="$PWD/zig-out/bin/vt-primitives" \
  cargo bench --offline -p rustty-vt --bench primitives
```

With that environment variable, Criterion registers matching `ghostty/*`
groups. `iter_custom` returns the duration reported by the **native operation
loop**; process startup, file reads, UTF-8 decoding, terminal setup, JSON, IPC,
and final terminal destruction are excluded. Both engines use identical
pre-generated UTF-8 corpora and validate content checksums. Ghostty's own unit
check additionally inspects combining and ZWJ suffixes and cloned text.
The native process primes a fresh terminal for each Criterion batch; Rust
retains its primed terminal across batches. Each batch repeats its operation
many times to measure steady behavior.

For a Rust-only saved baseline, filter the comparison to `rustty`, then run
the `ghostty` groups separately: Criterion's strict `--baseline` requires an
existing baseline for every selected group. Compare the resulting Criterion
estimates using the same units. Native pages, allocators, and retained metadata
differ between engines, so these are comparisons of equivalent terminal
operations, not of Rust versus Zig in isolation.

## Workloads and units

All terminals have 128 columns and 32 rows, zero scrollback, and DEC 2027
grapheme handling enabled. Each corpus repeats its fixed pattern 128 times:

| Corpus | Pattern | Input scalars | Allocating text path |
| --- | --- | ---: | --- |
| ASCII | `abcdefgh` | 1,024 | None with inline cells |
| Chinese | `天地玄黄宇宙洪荒` | 1,024 | None: each ideograph is one inline scalar |
| Combining | `áb̂c̃d̈` | 1,024 | 512 two-scalar graphemes |
| Emoji | `👩‍💻👨‍🚀` | 768 | 256 three-scalar ZWJ graphemes |

| Primitive | Measured work | Throughput unit |
| --- | --- | --- |
| `width` | Unicode width lookup on predecoded scalars | Input scalar |
| `print` | Home cursor and overwrite the same populated viewport via `Terminal::print` | Input scalar |
| `scalar` | Sum the first codepoint of every active cell | Cell slot (4,096 per scan) |
| `read` | Sum every codepoint, including page-owned grapheme suffixes | Cell slot (4,096 per scan) |
| `clone` | Copy the visible screen and destroy the copy | Cell slot (4,096 per copy) |
| `reflow` | Resize 128 → 64 → 128 columns, preserving all text | Complete resize round trip |

`print` measures warmed overwrites, including replacement of previous text;
it bypasses the UTF-8/VT parser. `scalar` isolates inline access, whereas `read`
uses the public text iterator, decoding UTF-8 only for graphemes. Empty cells and
wide-cell continuations are scanned too. `clone` includes destruction in both
engines. Reflow fits within the active screen even for the Chinese corpus.

Allocator instrumentation is excluded from timing. The separate
`scalar_allocations` test checks that ordinary scalar printing and reads make
no allocations. Chinese text alone does not exercise the grapheme allocator;
the combining and emoji cases do.

## Optimization measurements, 2026-09-15

The table compares Rustty at `3116bbb` with the four optimizations ending at
`2880a22`. Both versions have 56-byte cells. Ghostty production code is unchanged;
the reference uses the original native benchmark binary. These are medians in
microseconds per workload, with the units defined above, on an Apple M4 Max
running macOS 26.7, Rust 1.98.1, Zig 0.16.0, and Criterion 0.8.2. Rust uses the
workspace release profile (thin LTO, one codegen unit); Ghostty uses ReleaseFast.
Runs were serial, without competing builds or tests, using 20 samples, 0.3 seconds
of warmup, and 1 second of measurement per case. These short samples measure
the primitives, not application CPU or rendering performance.

| Primitive | Corpus | Rust before µs | Rust after µs | Ghostty reference µs |
| --- | --- | ---: | ---: | ---: |
| width | ASCII | 15.615 | 0.478 | 0.345 |
| width | Chinese | 13.735 | 0.478 | 0.342 |
| width | Combining | 15.212 | 0.435 | 0.408 |
| width | Emoji | 11.357 | 0.331 | 0.304 |
| print | ASCII | 41.018 | 41.677 | 4.972 |
| print | Chinese | 109.186 | 55.880 | 11.067 |
| print | Combining | 100.698 | 50.015 | 399.149 |
| print | Emoji | 122.636 | 46.300 | 13.972 |
| scalar | ASCII | 1.538 | 1.532 | 1.336 |
| scalar | Chinese | 1.489 | 1.519 | 1.380 |
| scalar | Combining | 1.561 | 1.541 | 1.329 |
| scalar | Emoji | 1.480 | 1.523 | 1.298 |
| read | ASCII | 10.600 | 1.720 | 1.934 |
| read | Chinese | 12.571 | 1.768 | 1.913 |
| read | Combining | 13.124 | 2.554 | 5.954 |
| read | Emoji | 11.736 | 2.617 | 2.446 |
| clone | ASCII | 10.923 | 10.656 | 5.830 |
| clone | Chinese | 11.746 | 11.842 | 5.834 |
| clone | Combining | 29.078 | 12.835 | 17.411 |
| clone | Emoji | 19.775 | 11.787 | 11.030 |
| reflow | ASCII | 274.793 | 69.065 | 22.501 |
| reflow | Chinese | 270.201 | 74.211 | 26.377 |
| reflow | Combining | 297.831 | 64.418 | 51.870 |
| reflow | Emoji | 275.598 | 56.242 | 32.131 |

Ghostty's indexed Unicode tables and tracked pins informed the Rustty changes:
Unicode properties now use deduplicated blocks instead of binary searches,
and reflow maps only live anchors instead of every cell. Inline text iteration
avoids a scalar-to-UTF-8 round trip. Grapheme payloads use page chunk indices
instead of hashing, and append builds its replacement in bounded stack scratch
before allocating one shared payload.

Full-text reads improve 4.5–7.1×, reflow 3.6–4.9×, Chinese printing 2.0×, and
combining/emoji printing 2.0–2.6×. ASCII printing remains essentially unchanged
and substantially slower than Ghostty. A separate scalar-scan confirmation
using 50 samples, 0.5 seconds of warmup, and 2 seconds of measurement confirmed
small regressions: Chinese 1.485 → 1.535 µs (3.4%) and emoji 1.473 → 1.530 µs
(3.9%) per 4,096-cell scan. No scalar-scan speedup is claimed.

The indexed Unicode table occupies
55,808 shared read-only bytes, replacing 14,994 bytes of production range data;
the old ranges remain a test-only oracle. Grapheme slot metadata is allocated
lazily: scalar-only pages have none, while long or sparse suffix allocations
can leave empty slots in the indexed vector. This change does not shrink cells.

The Ghostty combining-print result hits its existing full grapheme-map cliff
at 512 clusters. It does not represent general combining-text throughput:
the earlier density control measured about 404 µs at 512 clusters and 10 µs
at 513, where priming triggers page growth. Ghostty production code was not
modified to remove this cliff. Native reflow also includes OS page-recycling
costs, so the comparison is specific to this machine and allocator.

Validation passed all 291 `rustty-vt` tests, all 24 benchmark smoke cases, strict
all-target VT Clippy, formatting, the app check, and native benchmark unit
checks. Differential Unicode, snapshot, resize, graphics-anchor, and grapheme
suites passed 7,970 of 7,973 comparisons. The remaining three are chunking
variants of the preexisting `pages/graphemes/wrap/3/1/1/alternate` discrepancy:
Rustty preserves a ZWJ when wrapping a widening grapheme in a one-row alternate
screen; Ghostty drops it. This also reproduces before the inline-cell changes.
