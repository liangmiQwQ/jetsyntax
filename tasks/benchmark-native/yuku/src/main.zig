const std = @import("std");
const parser = @import("yuku_parser");

const Stats = struct {
    median_ms: f64,
    minimum_ms: f64,
    p99_ms: f64,
};

fn nanosecondsToMilliseconds(nanoseconds: u64) f64 {
    return @as(f64, @floatFromInt(nanoseconds)) / std.time.ns_per_ms;
}

fn benchmark(
    io: std.Io,
    allocator: std.mem.Allocator,
    warmups: usize,
    samples: usize,
    source: []const u8,
    path: []const u8,
) !Stats {
    const options: parser.Options = .{
        .lang = .fromPath(path),
        .source_type = .fromPath(path),
    };

    for (0..warmups) |_| {
        var tree = try parser.parse(std.heap.smp_allocator, source, options);
        std.mem.doNotOptimizeAway(tree.nodes.len);
        tree.deinit();
    }

    const timings = try allocator.alloc(u64, samples);
    defer allocator.free(timings);
    for (timings) |*timing| {
        const start = try std.Io.Clock.now(.awake, io);
        var tree = try parser.parse(std.heap.smp_allocator, source, options);
        const end = try std.Io.Clock.now(.awake, io);

        std.mem.doNotOptimizeAway(tree.nodes.len);
        tree.deinit();
        timing.* = @intCast(start.durationTo(end).toNanoseconds());
    }

    std.mem.sort(u64, timings, {}, std.sort.asc(u64));
    return .{
        .median_ms = nanosecondsToMilliseconds(timings[samples / 2]),
        .minimum_ms = nanosecondsToMilliseconds(timings[0]),
        .p99_ms = nanosecondsToMilliseconds(timings[(samples * 99 / 100)]),
    };
}

pub fn main(init: std.process.Init) !void {
    const io = init.io;
    const allocator = init.gpa;
    var arguments = init.minimal.args.iterate();
    _ = arguments.skip();
    const warmups = try std.fmt.parseInt(usize, arguments.next() orelse return error.MissingWarmups, 10);
    const samples = try std.fmt.parseInt(usize, arguments.next() orelse return error.MissingSamples, 10);
    if (samples == 0) return error.MissingSamples;

    var json: std.ArrayList(u8) = .empty;
    defer json.deinit(allocator);
    const header = try std.fmt.allocPrint(
        allocator,
        "{{\"methodology\":{{\"warmups\":{d},\"samples\":{d}}},\"results\":[",
        .{ warmups, samples },
    );
    defer allocator.free(header);
    try json.appendSlice(allocator, header);

    var first = true;
    while (arguments.next()) |path| {
        const source = try std.Io.Dir.cwd().readFileAlloc(io, path, allocator, .unlimited);
        defer allocator.free(source);
        const stats = try benchmark(io, allocator, warmups, samples, source, path);

        if (!first) try json.append(allocator, ',');
        first = false;
        const result = try std.fmt.allocPrint(
            allocator,
            "{{\"fixture\":\"{s}\",\"bytes\":{d},\"parser\":\"Yuku\",\"medianMs\":{d},\"minimumMs\":{d},\"p99Ms\":{d}}}",
            .{
                std.fs.path.basename(path),
                source.len,
                stats.median_ms,
                stats.minimum_ms,
                stats.p99_ms,
            },
        );
        defer allocator.free(result);
        try json.appendSlice(allocator, result);
    }

    try json.appendSlice(allocator, "]}\n");
    try std.Io.File.stdout().writeStreamingAll(io, json.items);
}
