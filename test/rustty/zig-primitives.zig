//! Portable unit-level benchmarks. Only libghostty-vt and std are imported;
//! there is no renderer, application runtime, PTY, or platform instrumentation.
//! Input generation and UTF-8 decoding are deliberately outside the timer.
const std = @import("std");
const vt = @import("ghostty-vt");

pub const std_options: std.Options = .{ .log_level = .err };

const Operation = enum { width, print, scalar, read, clone, reflow };
const cols = 128;
const rows = 32;

fn decode(alloc: std.mem.Allocator, bytes: []const u8) ![]u21 {
    const view = try std.unicode.Utf8View.init(bytes);
    var result: std.ArrayList(u21) = .empty;
    errdefer result.deinit(alloc);
    var it = view.iterator();
    while (it.nextCodepoint()) |cp| {
        // These workloads exercise text primitives, not terminal controls.
        if (cp < 0x20 or (cp >= 0x7f and cp < 0xa0)) return error.InvalidCorpus;
        try result.append(alloc, cp);
    }
    if (result.items.len == 0) return error.InvalidCorpus;
    return result.toOwnedSlice(alloc);
}

fn fill(terminal: *vt.Terminal, cps: []const u21) !void {
    terminal.setCursorPos(1, 1);
    for (cps) |cp| try terminal.print(cp);
}

fn read(screen: *const vt.Screen, comptime full_text: bool) u64 {
    var sum: u64 = 0;
    var it = screen.pages.rowIterator(.right_down, .{ .active = .{} }, null);
    while (it.next()) |pin| {
        for (pin.cells(.all)) |*cell| {
            sum +%= cell.codepoint();
            if (full_text and cell.hasGrapheme()) {
                for (pin.grapheme(cell).?) |cp| sum +%= cp;
            }
        }
    }
    return sum;
}

fn step(comptime op: Operation, terminal: *vt.Terminal, cps: []const u21) !u64 {
    std.mem.doNotOptimizeAway(terminal);
    std.mem.doNotOptimizeAway(cps);
    switch (op) {
        .width => {
            var sum: u64 = 0;
            for (cps) |cp| sum +%= vt.unicode.codepointWidth(cp);
            return sum;
        },
        .print => try fill(terminal, cps),
        .scalar => return read(terminal.screens.active, false),
        .read => return read(terminal.screens.active, true),
        .clone => {
            var copy = try terminal.screens.active.clone(
                terminal.io(),
                terminal.gpa(),
                .{ .viewport = .{} },
                null,
            );
            std.mem.doNotOptimizeAway(&copy);
            copy.deinit();
        },
        .reflow => {
            try terminal.resize(terminal.gpa(), .{ .cols = cols / 2, .rows = rows });
            try terminal.resize(terminal.gpa(), .{ .cols = cols, .rows = rows });
        },
    }
    std.mem.doNotOptimizeAway(terminal);
    return 0;
}

pub fn main(init: std.process.Init) !void {
    const alloc = init.gpa;
    const args = try init.minimal.args.toSlice(init.arena.allocator());
    if (args.len != 4) return error.ExpectedOperationDatafileIterations;
    const op = std.meta.stringToEnum(Operation, args[1]) orelse return error.InvalidOperation;
    const iterations = try std.fmt.parseInt(u64, args[3], 10);
    if (iterations == 0) return error.InvalidIterations;
    const bytes = try std.Io.Dir.cwd().readFileAlloc(init.io, args[2], alloc, .limited(16 * 1024 * 1024));
    defer alloc.free(bytes);
    const cps = try decode(alloc, bytes);
    defer alloc.free(cps);
    var terminal = try vt.Terminal.init(init.io, alloc, .{
        .cols = cols,
        .rows = rows,
        .max_scrollback_bytes = 0,
        .default_modes = .{ .grapheme_cluster = true },
    });
    defer terminal.deinit(alloc);
    try fill(&terminal, cps);
    const expected = switch (op) {
        .width => try step(.width, &terminal, cps),
        .scalar => read(terminal.screens.active, false),
        else => read(terminal.screens.active, true),
    };

    var checksum: u64 = 0;
    const elapsed = switch (op) {
        inline else => |operation| measured: {
            const start = std.Io.Timestamp.now(init.io, .awake);
            for (0..iterations) |_| {
                const value = try step(operation, &terminal, cps);
                std.mem.doNotOptimizeAway(value);
                checksum = value;
            }
            break :measured start.durationTo(.now(init.io, .awake)).nanoseconds;
        },
    };
    switch (op) {
        .print, .clone, .reflow => checksum = read(terminal.screens.active, true),
        else => {},
    }
    if (checksum != expected) return error.ChecksumMismatch;

    var buffer: [4096]u8 = undefined;
    var stdout = std.Io.File.stdout().writerStreaming(init.io, &buffer);
    try std.json.Stringify.value(.{
        .engine = "ghostty",
        .operation = @tagName(op),
        .iterations = iterations,
        .elapsed_ns = elapsed,
        .units_per_iteration = switch (op) {
            .width, .print => cps.len,
            .scalar, .read, .clone => cols * rows,
            .reflow => 1,
        },
        .cell_bytes = @sizeOf(vt.Cell),
        .checksum = checksum,
    }, .{}, &stdout.interface);
    try stdout.interface.writeByte('\n');
    try stdout.interface.flush();
}

test "primitive workloads retain scalar and grapheme contents" {
    const alloc = std.testing.allocator;
    const cps = try decode(alloc, "a\u{0301}天地👩\u{200d}💻");
    defer alloc.free(cps);
    var terminal = try vt.Terminal.init(std.testing.io, alloc, .{
        .cols = cols,
        .rows = rows,
        .max_scrollback_bytes = 0,
        .default_modes = .{ .grapheme_cluster = true },
    });
    defer terminal.deinit(alloc);
    try fill(&terminal, cps);
    var expected: u64 = 0;
    for (cps) |cp| expected += cp;
    try std.testing.expectEqual(expected, read(terminal.screens.active, true));
    const pin = terminal.screens.active.pages.getTopLeft(.active);
    const cells = pin.cells(.all);
    try std.testing.expectEqualSlices(u21, &.{0x0301}, pin.grapheme(&cells[0]).?);
    try std.testing.expectEqualSlices(u21, &.{ 0x200d, 0x1f4bb }, pin.grapheme(&cells[5]).?);
    try std.testing.expectEqual('a' + 0x5929 + 0x5730 + 0x1f469, try step(.scalar, &terminal, cps));
    var copy = try terminal.screens.active.clone(std.testing.io, alloc, .{ .viewport = .{} }, null);
    defer copy.deinit();
    try std.testing.expectEqual(expected, read(&copy, true));
    inline for ([_]Operation{ .print, .clone, .reflow }) |op| {
        _ = try step(op, &terminal, cps);
        try std.testing.expectEqual(expected, read(terminal.screens.active, true));
    }
    try std.testing.expectEqual(expected, try step(.read, &terminal, cps));
    try std.testing.expectEqual(9, try step(.width, &terminal, cps));
    try std.testing.expectError(error.InvalidCorpus, decode(alloc, "\n"));
    try std.testing.expectError(error.InvalidCorpus, decode(alloc, ""));
    try std.testing.expectError(error.InvalidUtf8, decode(alloc, "\xff"));
}
