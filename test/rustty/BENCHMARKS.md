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
includes the public Rust text API's UTF-8 encode/decode work. Empty cells and
wide-cell continuations are scanned too. `clone` includes destruction in both
engines. Reflow fits within the active screen even for the Chinese corpus.

Allocator instrumentation is excluded from timing. The separate
`scalar_allocations` test checks that ordinary scalar printing and reads make
no allocations. Chinese text alone does not exercise the grapheme allocator;
the combining and emoji cases do.
