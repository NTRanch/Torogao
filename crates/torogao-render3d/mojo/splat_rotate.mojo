# Torogao render3d — Mojo GPU splat layer-rotation kernel
# (EXTENSION TRIAL, 2026-11-27).
#
# Trial scope: the layer-rotation step of the CPU splat pack
# (`rotate_cloud`, crates/torogao-render3d/src/splat.rs, built on
# src/splat_rotate.rs) — measured at 401.8 ms standalone on the
# full bonsai witness in the adoption lane's profiling pass, the
# hottest measurable pure-f64 step after projection itself. The
# kernel is judged exactly like the projection kernel: bit-exact
# against the Rust reference on the full witness, or the trial
# stands as a record and the Rust path stays. Trial verdict
# (2026-11-27): BIT-EXACT on both estate GPUs, adopted; wired
# into the pack route by the follow-on lane the same day
# (src/splat_mojo.rs runs it ahead of the projection kernel
# whenever TOROGAO_MOJO_ROTATE_BIN names a binary).
#
# FP-MODE INVARIANT (gated): this file must be built with
# `--fp-mode contract=off` (see build.sh). contract=fast was
# measured REJECTED on the projection kernel (13,204 / 20,000
# debug records differing, worst relative depth deviation
# 2.217e-16); FMA contraction changes low-order bits and this
# estate's splat gates are record-for-record.
#
# File contract (little-endian throughout):
#   Header (776 bytes): u64 splat count N, then 96 f64 uniforms:
#     [0..9)   layer rotation matrix, row-major (Rotation.matrix)
#     [9..13)  layer rotation quaternion, (w, x, y, z)
#              (Rotation.quat)
#     [13..96) SH band matrices, ShRotation::matrices_flat()
#              layout (degree 1: 3x3 at 0, degree 2: 5x5 at 9,
#              degree 3: 7x7 at 34; each [m][n] row-major in the
#              indexing rotate_channel applies:
#              out[m] = sum_n D[m][n] * c[n])
#   Records: N x 256-byte GpuSplatRecord records (the frame
#   record layout of src/splat_gpu.rs): f32 pos[3] + activated
#   opacity, dc[3] + pad, activated scale[3] + pad, quaternion
#   (w, x, y, z), SH rest coefficients channel-major
#   ([[f32; 15]; 3]), tail padding. The kernel rotates pos, the
#   quaternion and the rest coefficients; every other field is
#   copied through bit-identically (rotate_cloud likewise
#   leaves dc, opacity and scales untouched).
#
# Determinism: every arithmetic expression transcribes the Rust
# source's association order exactly (Rotation::apply rows,
# quat_mul as written in splat_rotate.rs — note it does NOT
# renormalise the product — and rotate_channel's band sums
# folding in ascending n). All math is f64; the only f32
# conversions are the input loads (exact) and the output stores
# (the same round-to-nearest-even casts the Rust code performs).
#
# Bounds: N validated against the input file length before any
# allocation; grid sized from N; one thread per splat; the only
# loops are the fixed band sums (3/5/7 terms) and the fixed
# 32-word record copy.

from max.gpu import thread_idx, block_idx, block_dim
from max.gpu.host import DeviceContext
from std.sys import argv
from std.time import perf_counter_ns

comptime HEADER_BYTES = 776
comptime RECORD_BYTES = 256
comptime UNIFORM_COUNT = 96


def parse_nonnegative_int(s: String) raises -> Int:
    # Bounded argv parse: at most 9 digits, digits only.
    if s.byte_length() < 1 or s.byte_length() > 9:
        raise Error("integer argument out of bounds")
    var v = 0
    for byte in s.as_bytes():
        var d = Int(byte) - 48
        if d < 0 or d > 9:
            raise Error("integer argument is not decimal")
        v = v * 10 + d
    return v


def read_u64[O: Origin](p: Pointer[UInt8, O], off: Int) -> UInt64:
    var v = UInt64(0)
    for k in range(8):
        v |= UInt64(p.unsafe_offset(off + k).unsafe_load()) << UInt64(8 * k)
    return v


def read_f64[O: Origin](p: Pointer[UInt8, O], off: Int) -> Float64:
    var buf = List[UInt8](length=8, fill=0)
    for k in range(8):
        buf[k] = p.unsafe_offset(off + k).unsafe_load()
    return buf.unsafe_ptr().unsafe_bitcast[Float64]().unsafe_load()


def load_f32[O: Origin](p: Pointer[UInt8, O], off: Int) -> Float32:
    return p.unsafe_offset(off).unsafe_bitcast[Float32]().unsafe_load()


def store_f32(p: Pointer[UInt8, MutAnyOrigin], off: Int, v: Float32):
    p.unsafe_offset(off).unsafe_bitcast[Float32]().unsafe_store(val=v)


def rotate_kernel(
    recs: Pointer[UInt8, MutAnyOrigin],
    uni: Pointer[Float64, MutAnyOrigin],
    outb: Pointer[UInt8, MutAnyOrigin],
    count: Int64,
):
    var idx = Int64(block_idx.x) * Int64(block_dim.x) + Int64(thread_idx.x)
    if idx >= count:
        return
    var i = Int(idx)
    var base = i * RECORD_BYTES

    # Copy the whole record through first; the rotated fields are
    # overwritten below. Activated opacity, dc, activated scales,
    # pads and tail therefore stay bit-identical by construction.
    for w in range(RECORD_BYTES // 8):
        var word = (
            recs.unsafe_offset(base + w * 8)
            .unsafe_bitcast[UInt64]()
            .unsafe_load()
        )
        outb.unsafe_offset(base + w * 8).unsafe_bitcast[UInt64]().unsafe_store(
            val=word
        )

    # Position: p' = R * p, rows dotted in index order
    # (Rotation::apply).
    var px = Float64(load_f32(recs, base + 0))
    var py = Float64(load_f32(recs, base + 4))
    var pz = Float64(load_f32(recs, base + 8))
    var m0 = uni.unsafe_offset(0).unsafe_load()
    var m1 = uni.unsafe_offset(1).unsafe_load()
    var m2 = uni.unsafe_offset(2).unsafe_load()
    var m3 = uni.unsafe_offset(3).unsafe_load()
    var m4 = uni.unsafe_offset(4).unsafe_load()
    var m5 = uni.unsafe_offset(5).unsafe_load()
    var m6 = uni.unsafe_offset(6).unsafe_load()
    var m7 = uni.unsafe_offset(7).unsafe_load()
    var m8 = uni.unsafe_offset(8).unsafe_load()
    store_f32(outb, base + 0, Float32(m0 * px + m1 * py + m2 * pz))
    store_f32(outb, base + 4, Float32(m3 * px + m4 * py + m5 * pz))
    store_f32(outb, base + 8, Float32(m6 * px + m7 * py + m8 * pz))

    # Orientation: q' = quat_mul(layer, splat), both (w, x, y, z),
    # transcribed term-for-term from splat_rotate.rs. The product
    # is NOT renormalised — the Rust reference does not
    # renormalise it either.
    var aw = uni.unsafe_offset(9).unsafe_load()
    var ax = uni.unsafe_offset(10).unsafe_load()
    var ay = uni.unsafe_offset(11).unsafe_load()
    var az = uni.unsafe_offset(12).unsafe_load()
    var bw = Float64(load_f32(recs, base + 48))
    var bx = Float64(load_f32(recs, base + 52))
    var by = Float64(load_f32(recs, base + 56))
    var bz = Float64(load_f32(recs, base + 60))
    store_f32(outb, base + 48, Float32(aw * bw - ax * bx - ay * by - az * bz))
    store_f32(outb, base + 52, Float32(aw * bx + ax * bw + ay * bz - az * by))
    store_f32(outb, base + 56, Float32(aw * by - ax * bz + ay * bw + az * bx))
    store_f32(outb, base + 60, Float32(aw * bz + ax * by - ay * bx + az * bw))

    # SH rest coefficients, per channel (rotate_channel): three
    # band rotations whose matrices sit in the flat block at
    # offsets 0 (3x3), 9 (5x5) and 34 (7x7) within the band
    # region (uniform index 13 + offset), each [m][n] row-major:
    # out[m] = sum_n D[m][n] * c[n], n ascending. Inputs are
    # loaded straight from the record (the projection kernel's
    # idiom — no on-device collections).
    for ch in range(3):
        var cbase = base + 64 + ch * 60  # first rest coefficient byte
        # Degree 1: coefficients 0..3, width 3, flat offset 13.
        for i1 in range(3):
            var acc = Float64(0.0)
            for j in range(3):
                acc += uni.unsafe_offset(13 + i1 * 3 + j).unsafe_load() * (
                    Float64(load_f32(recs, cbase + j * 4))
                )
            store_f32(outb, cbase + i1 * 4, Float32(acc))
        # Degree 2: coefficients 3..8, width 5, flat offset 13 + 9.
        for i2 in range(5):
            var acc2 = Float64(0.0)
            for j2 in range(5):
                acc2 += uni.unsafe_offset(22 + i2 * 5 + j2).unsafe_load() * (
                    Float64(load_f32(recs, cbase + (3 + j2) * 4))
                )
            store_f32(outb, cbase + (3 + i2) * 4, Float32(acc2))
        # Degree 3: coefficients 8..15, width 7, flat offset 13 + 34.
        for i3 in range(7):
            var acc3 = Float64(0.0)
            for j3 in range(7):
                acc3 += uni.unsafe_offset(47 + i3 * 7 + j3).unsafe_load() * (
                    Float64(load_f32(recs, cbase + (8 + j3) * 4))
                )
            store_f32(outb, cbase + (8 + i3) * 4, Float32(acc3))


def main() raises:
    var args = argv()
    if len(args) != 5:
        raise Error("usage: splat_rotate IN OUT REPS DEVICE_ID")
    var in_path = String(args[1])
    var out_path = String(args[2])
    var reps = parse_nonnegative_int(String(args[3]))
    var device_id = parse_nonnegative_int(String(args[4]))
    if reps < 1 or reps > 1000:
        raise Error("REPS out of bounds (1..1000)")

    var bytes_in: List[UInt8]
    with open(in_path, "r") as f:
        bytes_in = f.read_bytes()
    if len(bytes_in) < HEADER_BYTES:
        raise Error("input shorter than the header")

    var count = Int(read_u64(bytes_in.unsafe_ptr(), 0))
    if count < 1 or count > 2147483647:
        raise Error("splat count out of bounds")
    var in_bytes = count * RECORD_BYTES
    if len(bytes_in) != HEADER_BYTES + in_bytes:
        raise Error("input length does not match the header count")

    with DeviceContext(device_id=device_id) as ctx:
        print("device:", ctx.name())
        var dev_in = ctx.enqueue_create_buffer[DType.uint8](in_bytes)
        var host_in = ctx.enqueue_create_host_buffer[DType.uint8](in_bytes)
        var hin_p = host_in.unsafe_ptr()
        var bin_p = bytes_in.unsafe_ptr().unsafe_offset(HEADER_BYTES)
        var n8_in = in_bytes // 8
        for k in range(n8_in):
            hin_p.unsafe_offset(k * 8).unsafe_bitcast[UInt64]().unsafe_store(
                val=bin_p.unsafe_offset(k * 8).unsafe_bitcast[UInt64]().unsafe_load()
            )
        for k in range(n8_in * 8, in_bytes):
            hin_p.unsafe_offset(k).unsafe_store(
                val=bin_p.unsafe_offset(k).unsafe_load()
            )
        var t0 = perf_counter_ns()
        ctx.enqueue_copy(dev_in, host_in)
        ctx.synchronize()
        var t1 = perf_counter_ns()
        print("H2D_MS", Float64(t1 - t0) / 1.0e6)

        var dev_uni = ctx.enqueue_create_buffer[DType.float64](UNIFORM_COUNT)
        with dev_uni.map_to_host() as mapped:
            for k in range(UNIFORM_COUNT):
                mapped[k] = read_f64(bytes_in.unsafe_ptr(), 8 + k * 8)
        var dev_out = ctx.enqueue_create_buffer[DType.uint8](in_bytes)

        var grid = (count + 255) // 256
        # Warmup (also the correctness run whose output is written).
        ctx.enqueue_function[rotate_kernel](
            dev_in,
            dev_uni,
            dev_out,
            Int64(count),
            grid_dim=grid,
            block_dim=256,
        )
        ctx.synchronize()

        var times = List[Float64]()
        for _ in range(reps):
            var s = perf_counter_ns()
            ctx.enqueue_function[rotate_kernel](
                dev_in,
                dev_uni,
                dev_out,
                Int64(count),
                grid_dim=grid,
                block_dim=256,
            )
            ctx.synchronize()
            var e = perf_counter_ns()
            times.append(Float64(e - s) / 1.0e6)
        # Median of the timed launches (bubble sort in place; the
        # list is bounded by the REPS check above).
        for a in range(len(times)):
            for b in range(a + 1, len(times)):
                if times[b] < times[a]:
                    var tmp = times[a]
                    times[a] = times[b]
                    times[b] = tmp
        print("KERNEL_MEDIAN_MS", times[len(times) // 2])
        print("KERNEL_MIN_MS", times[0])

        var host_out = ctx.enqueue_create_host_buffer[DType.uint8](in_bytes)
        var t2 = perf_counter_ns()
        ctx.enqueue_copy(host_out, dev_out)
        ctx.synchronize()
        var t3 = perf_counter_ns()
        print("D2H_MS", Float64(t3 - t2) / 1.0e6)

        var out_list = List[UInt8](length=in_bytes, fill=0)
        var hout_p = host_out.unsafe_ptr()
        var olist_p = out_list.unsafe_ptr()
        var n8_out = in_bytes // 8
        for k in range(n8_out):
            olist_p.unsafe_offset(k * 8).unsafe_bitcast[UInt64]().unsafe_store(
                val=hout_p.unsafe_offset(k * 8).unsafe_bitcast[UInt64]().unsafe_load()
            )
        for k in range(n8_out * 8, in_bytes):
            olist_p.unsafe_offset(k).unsafe_store(
                val=hout_p.unsafe_offset(k).unsafe_load()
            )
        with open(out_path, "w") as f:
            f.write_bytes(out_list)
    print("SPLAT_ROTATE_DONE", count)
