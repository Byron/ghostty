# Portable terminal primitive benchmarks

The Rustty benchmarks use Criterion and call the headless `rustty-vt` APIs
directly. They do not construct a session, PTY, font system, renderer, GPU, or
window. Criterion is a development dependency only. No application build is
needed. Timing excludes terminal construction and input generation; the `feed`
and `stream` workloads include UTF-8 decoding and VT parsing.

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
loop**; process startup, file reads, terminal setup, JSON, IPC, and final
terminal destruction are excluded. Predecoding for the original primitives is
also excluded; `feed` and `stream` decode inside their measured parser calls. Both engines use identical
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

All terminals have 128 columns and 32 rows and DEC 2027 grapheme handling
enabled. The six original primitives and `feed` disable scrollback. Their
corpora repeat a fixed pattern 128 times:

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
| `feed` | Parse CUP (home cursor) followed by the same UTF-8 text as `print` | Input byte |
| `stream` | Parse 32 plain wrapped records, scrolling and evicting bounded history | Input byte |
| `stream_styled` | Same records with alternating SGR foreground colors and bold | Input byte |

`print` measures warmed overwrites, including replacement of previous text;
it bypasses the UTF-8/VT parser. `scalar` isolates inline access, whereas `read`
uses the public text iterator, decoding UTF-8 only for graphemes. Empty cells and
wide-cell continuations are scanned too. `clone` includes destruction in both
engines. Reflow fits within the active screen even for the Chinese corpus.

`feed` exposes UTF-8 decoding, parser dispatch, and any printable-run batching in
addition to cell replacement. It includes a three-byte CUP sequence. Stream
iterations contain 32 records of 192 display columns followed by CRLF: 6,144
printed columns and 64 physical rows. Record lengths are equal in display
columns across corpora, using 24 ASCII, 12 Chinese, or 48 combining/emoji pattern
repetitions. `stream_styled` cycles palette colors 1–4, alternates bold, and
resets SGR before each CRLF.

| Stream corpus | Printable scalars per iteration | Plain input bytes | Styled input bytes |
| --- | ---: | ---: | ---: |
| ASCII | 6,144 | 6,208 | 6,576 |
| Chinese | 3,072 | 9,280 | 9,648 |
| Combining | 12,288 | 18,496 | 18,864 |
| Emoji | 9,216 | 33,856 | 34,224 |

Both stream engines request a 1,024-line history limit with no byte limit and
prime with 2,048 physical rows before measurement, so allocations and history
eviction are already active. Native pages and resource admission can retain
different exact row counts below the same requested limit. Rust keeps the
terminal across Criterion batches; the native helper primes a fresh terminal
for each batch. History eviction is page-granular, so longer measurements
average over allocation/recycling phases better than single iterations.

Outside the timers, the harness checks the analytically expected visible text,
every populated cell's foreground/bold, final cursor, and bounded nonempty
history. An order-sensitive checksum over active cell contents and styles must
match the native helper. These checks are exercised by `--test` as well as
ordinary Criterion runs. All input generation and priming stay outside timing.

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

## Printing and scrolling follow-up, 2026-09-15

This follow-up starts with the already optimized production code at `e112c2c`.
The identical 36-case harness from `847ad1c` measures that baseline and the
production changes ending at `d2911c9` (`5cc935a` replaces printed cells directly;
`d2911c9` avoids building resource lists for rows without resources). Ghostty
production code remains unchanged; only its benchmark utility was extended.

Measurements use the same machine, compilers, release settings, and short
Criterion sampling configuration described above. All builds and tests finished
before serial timing. Values are medians in microseconds per complete workload.
The `feed` and stream byte/scalar counts are defined in the workload tables;
they are not the same amount of work as the original `print` case.

ASCII printing improves 3.38× and Chinese printing 1.87×. Parsed ASCII overwrites
improve 2.73×, plain ASCII streams 2.66×, and styled ASCII streams 2.18×. This
pass does not reach the proposed 5× printing target. Ghostty's printable-run
batching remains a substantial opportunity for parsed output. Cell size stays
56 bytes. The ASCII/combining direct scalar-scan controls are about 3% slower
in this run, roughly 0.04 microseconds per 4,096-cell scan; those regressions
are retained in the table rather than treated as improvements.

Sampling used optimized Rust binaries with debug information and frame pointers,
at nominal 1 kHz for eight seconds per workload. Timings above use the normal
release binaries without profiling. Shares below describe physical symbols;
inlined work contributes to its enclosing symbol, and shares are independently
normalized for each run.

- Before the change, cursor/style/link helpers account for about 32% of ASCII
  printing samples, and erase-before-replace accounts for another 10%.
- After replacement was consolidated, `sync_resource_row` accounted for 29% of
  ASCII stream samples. After skipping resource-list construction for rows
  without resources, its share falls to 6.4%.
- Final ASCII streaming spends about 28% in cell writing, 16% in the enclosing
  print operation, 11% accounting for row storage, and 8% allocating/initializing
  cell rows. These identify remaining costs beyond printable-run batching.

Two native reference cases need special interpretation. Combining overwrite
still hits the full 512-entry grapheme-map cliff. Parsed Chinese overwrite hits
another existing cliff: the native batch writer scans the remaining eligible
Unicode run, rejects an existing wide destination cell, prints one character,
and rescans the suffix. That is roughly half a million eligibility checks for
1,024 characters. A confirming profile attributes 96.4% of samples to
`Terminal.printSlice`. Fresh-row Chinese streaming batches successfully. The
Chinese `feed` result therefore does not describe general Chinese throughput.
Neither native production behavior was changed.

Final validation: all 293 `rustty-vt` tests, all 72 Rust/native benchmark smoke
cases, strict all-target Clippy, formatting, and the app check pass. Charset,
style, hyperlink, and grapheme differential suites pass 2,013 of 2,016 cases;
the three failures are the same previously documented alternate-screen ZWJ
mismatch in whole/scalar/chunked input variants. Independent review found no
correctness issues in either production change.

| Operation | Corpus | Rustty before µs | Rustty after µs | Ghostty µs | Speedup |
| --- | --- | ---: | ---: | ---: | ---: |
| print | ascii | 43.466 | 12.861 | 6.236 | 3.38× |
| print | chinese | 57.447 | 30.717 | 12.130 | 1.87× |
| print | combining | 50.903 | 34.039 | 404.776 | 1.50× |
| print | emoji | 47.003 | 37.587 | 15.084 | 1.25× |
| feed | ascii | 46.080 | 16.875 | 0.502 | 2.73× |
| feed | chinese | 61.892 | 35.659 | 441.271 | 1.74× |
| feed | combining | 56.481 | 38.841 | 436.534 | 1.45× |
| feed | emoji | 52.625 | 42.016 | 17.382 | 1.25× |
| stream | ascii | 465.277 | 175.223 | 5.749 | 2.66× |
| stream | chinese | 371.331 | 179.941 | 9.311 | 2.06× |
| stream | combining | 906.178 | 563.151 | 502.738 | 1.61× |
| stream | emoji | 864.453 | 633.851 | 743.788 | 1.36× |
| stream_styled | ascii | 545.556 | 249.981 | 7.908 | 2.18× |
| stream_styled | chinese | 420.376 | 220.568 | 35.590 | 1.91× |
| stream_styled | combining | 1032.979 | 739.317 | 541.605 | 1.40× |
| stream_styled | emoji | 1010.541 | 808.619 | 772.004 | 1.25× |
| read | ascii | 1.792 | 1.743 | 1.940 | 1.03× |
| read | chinese | 1.793 | 1.751 | 2.030 | 1.02× |
| read | combining | 2.541 | 2.508 | 5.932 | 1.01× |
| read | emoji | 2.529 | 2.488 | 2.568 | 1.02× |
| clone | ascii | 10.987 | 11.146 | 5.734 | 0.99× |
| clone | chinese | 12.086 | 11.916 | 5.737 | 1.01× |
| clone | combining | 13.198 | 12.888 | 17.418 | 1.02× |
| clone | emoji | 12.174 | 11.802 | 9.545 | 1.03× |
| reflow | ascii | 69.160 | 68.539 | 23.560 | 1.01× |
| reflow | chinese | 71.441 | 70.129 | 24.780 | 1.02× |
| reflow | combining | 64.546 | 64.676 | 51.886 | 1.00× |
| reflow | emoji | 57.025 | 57.145 | 41.325 | 1.00× |
| width | ascii | 0.479 | 0.480 | 0.361 | 1.00× |
| width | chinese | 0.481 | 0.480 | 0.343 | 1.00× |
| width | combining | 0.444 | 0.437 | 0.431 | 1.02× |
| width | emoji | 0.342 | 0.328 | 0.321 | 1.04× |
| scalar | ascii | 1.558 | 1.598 | 1.308 | 0.97× |
| scalar | chinese | 1.542 | 1.553 | 1.310 | 0.99× |
| scalar | combining | 1.569 | 1.609 | 1.304 | 0.97× |
| scalar | emoji | 1.547 | 1.535 | 1.304 | 1.01× |
