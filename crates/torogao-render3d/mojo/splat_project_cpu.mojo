# splat_project_cpu.mojo — CPU variant of splat_project.mojo (the
# Mojo/PTX splat-projection prototype), written for the
# render3d-mobile-compute lane (2026-11-27): the Owner asked
# whether the estate's Mojo toolchain can carry the projection
# to phone-class hardware. PTX cannot (NVIDIA-only), but Mojo
# also compiles CPU binaries — this file tests that route,
# including cross-compilation to aarch64.
#
# The file contract, CLI, and output format are IDENTICAL to
# splat_project.mojo (the GPU program): inputs.bin -> kernel-
# out.bin, record-for-record, gated by the same Rust harness
# (splat_mojo_bench). The projection body is the GPU kernel's
# body with the thread indexing replaced by a plain loop; the
# math is not re-derived — every expression keeps the GPU
# kernel's (and so the Rust pack's) association order, so the
# record-for-record gate against the CPU reference judges this
# file exactly as it judges the kernel.
#
# Scope: identity layer rotation, unlit — the same scope as
# the GPU prototype (the bench's config A).
#
# FP discipline: the gated invariant of the GPU build is
# `--fp-mode contract=off` (the default `contract=fast` build
# is a recorded REJECT: FMA contraction breaks bit-exactness
# at 1 ulp). CPU builds pin the same flag; the gate on each
# target (x86_64 host, aarch64 under qemu) is the evidence.
#
# Single-threaded by design: the Rust pack's projection is a
# single-threaded loop, and the honest first comparison is
# thread-for-thread. Per-splat independence makes a parallel
# variant bit-safe (each output record has exactly one
# writer); that is a named follow-on, not this file.

from std.sys import argv
from std.time import perf_counter_ns
from std.math import sqrt

# SH basis constants — same literals as splat_project.mojo.
comptime SH_C0 = 0.28209479177387814
comptime SH_C1 = 0.4886025119029199
comptime SH_C2_0 = 1.0925484305920792
comptime SH_C2_1 = -1.0925484305920792
comptime SH_C2_2 = 0.31539156525252
comptime SH_C2_3 = -1.0925484305920792
comptime SH_C2_4 = 0.5462742152960396
comptime SH_C3_0 = -0.5900435899266435
comptime SH_C3_1 = 2.890611442640554
comptime SH_C3_2 = -0.4570457994644658
comptime SH_C3_3 = 0.3731763325901154
comptime SH_C3_4 = -0.4570457994644658
comptime SH_C3_5 = 1.445305721320277
comptime SH_C3_6 = -0.5900435899266435

comptime COV_DILATION_PX2 = 0.3
comptime QUAD_SIGMAS = 3.0

comptime HEADER_BYTES = 368
comptime RECORD_BYTES = 256
comptime OUT_BYTES = 96
comptime UNIFORM_COUNT = 44


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


def read_f32[O: Origin](p: Pointer[UInt8, O], off: Int) -> Float32:
    var buf = List[UInt8](length=4, fill=0)
    for k in range(4):
        buf[k] = p.unsafe_offset(off + k).unsafe_load()
    return buf.unsafe_ptr().unsafe_bitcast[Float32]().unsafe_load()


def load_f32[O: Origin](p: Pointer[UInt8, O], off: Int) -> Float32:
    return p.unsafe_offset(off).unsafe_bitcast[Float32]().unsafe_load()


def store_f64[O: MutOrigin](p: Pointer[UInt8, O], off: Int, v: Float64):
    p.unsafe_offset(off).unsafe_bitcast[Float64]().unsafe_store(val=v)


def store_f32[O: MutOrigin](p: Pointer[UInt8, O], off: Int, v: Float32):
    p.unsafe_offset(off).unsafe_bitcast[Float32]().unsafe_store(val=v)


def store_u32[O: MutOrigin](p: Pointer[UInt8, O], off: Int, v: UInt32):
    p.unsafe_offset(off).unsafe_bitcast[UInt32]().unsafe_store(val=v)


def apply_mat(
    uni: List[Float64],
    base: Int,
    v0: Float64,
    v1: Float64,
    v2: Float64,
    v3: Float64,
) -> Tuple[Float64, Float64, Float64, Float64]:
    # Column-major Mat4 times vec4, the crate math.rs `apply`
    # association: ((p0 + p1) + p2) + p3 per row.
    return (
        uni[base + 0] * v0 + uni[base + 4] * v1 + uni[base + 8] * v2 + uni[base + 12] * v3,
        uni[base + 1] * v0 + uni[base + 5] * v1 + uni[base + 9] * v2 + uni[base + 13] * v3,
        uni[base + 2] * v0 + uni[base + 6] * v1 + uni[base + 10] * v2 + uni[base + 14] * v3,
        uni[base + 3] * v0 + uni[base + 7] * v1 + uni[base + 11] * v2 + uni[base + 15] * v3,
    )


def project_one[O: Origin, O2: MutOrigin](
    recs: Pointer[UInt8, O],
    uni: List[Float64],
    outb: Pointer[UInt8, O2],
    i: Int,
    alpha_cull: Float32,
):
    # The GPU kernel's body, verbatim math, one splat.
    var base = i * RECORD_BYTES
    var obase = i * OUT_BYTES

    # Culled-by-default record; survivors overwrite the flag last.
    store_u32(outb, obase + 88, 1)

    var opacity = load_f32(recs, base + 12)
    if opacity < alpha_cull:
        return

    var tr_scale = uni[38]
    var centre0 = Float64(load_f32(recs, base + 0)) * tr_scale + uni[35]
    var centre1 = Float64(load_f32(recs, base + 4)) * tr_scale + uni[36]
    var centre2 = Float64(load_f32(recs, base + 8)) * tr_scale + uni[37]

    var t = apply_mat(uni, 0, centre0, centre1, centre2, 1.0)
    var depth = -t[2]
    if depth < uni[41]:
        return

    # Covariance from the splat's quaternion + activated scales.
    var rw = Float64(load_f32(recs, base + 48))
    var rx = Float64(load_f32(recs, base + 52))
    var ry = Float64(load_f32(recs, base + 56))
    var rz = Float64(load_f32(recs, base + 60))
    var qnorm = sqrt(rw * rw + rx * rx + ry * ry + rz * rz)
    var qw = Float64(1.0)
    var qx = Float64(0.0)
    var qy = Float64(0.0)
    var qz = Float64(0.0)
    if qnorm > 0.0:
        qw = rw / qnorm
        qx = rx / qnorm
        qy = ry / qnorm
        qz = rz / qnorm
    var r00 = 1.0 - 2.0 * (qy * qy + qz * qz)
    var r01 = 2.0 * (qx * qy - qw * qz)
    var r02 = 2.0 * (qx * qz + qw * qy)
    var r10 = 2.0 * (qx * qy + qw * qz)
    var r11 = 1.0 - 2.0 * (qx * qx + qz * qz)
    var r12 = 2.0 * (qy * qz - qw * qx)
    var r20 = 2.0 * (qx * qz - qw * qy)
    var r21 = 2.0 * (qy * qz + qw * qx)
    var r22 = 1.0 - 2.0 * (qx * qx + qy * qy)
    var sc0 = Float64(load_f32(recs, base + 32))
    var sc1 = Float64(load_f32(recs, base + 36))
    var sc2 = Float64(load_f32(recs, base + 40))
    var s20 = sc0 * sc0
    var s21 = sc1 * sc1
    var s22 = sc2 * sc2

    var ca00 = 0.0 + r00 * s20 * r00 + r01 * s21 * r01 + r02 * s22 * r02
    var ca01 = 0.0 + r00 * s20 * r10 + r01 * s21 * r11 + r02 * s22 * r12
    var ca02 = 0.0 + r00 * s20 * r20 + r01 * s21 * r21 + r02 * s22 * r22
    var ca10 = 0.0 + r10 * s20 * r00 + r11 * s21 * r01 + r12 * s22 * r02
    var ca11 = 0.0 + r10 * s20 * r10 + r11 * s21 * r11 + r12 * s22 * r12
    var ca12 = 0.0 + r10 * s20 * r20 + r11 * s21 * r21 + r12 * s22 * r22
    var ca20 = 0.0 + r20 * s20 * r00 + r21 * s21 * r01 + r22 * s22 * r02
    var ca21 = 0.0 + r20 * s20 * r10 + r21 * s21 * r11 + r22 * s22 * r12
    var ca22 = 0.0 + r20 * s20 * r20 + r21 * s21 * r21 + r22 * s22 * r22

    # View rotation w3[i][j] = view.m[j*4+i].
    var w00 = uni[0]
    var w01 = uni[4]
    var w02 = uni[8]
    var w10 = uni[1]
    var w11 = uni[5]
    var w12 = uni[9]
    var w20 = uni[2]
    var w21 = uni[6]
    var w22 = uni[10]

    var s2t = tr_scale * tr_scale
    var cv00 = s2t * (
        0.0
        + (0.0 + w00 * ca00 * w00 + w00 * ca01 * w01 + w00 * ca02 * w02)
        + (0.0 + w01 * ca10 * w00 + w01 * ca11 * w01 + w01 * ca12 * w02)
        + (0.0 + w02 * ca20 * w00 + w02 * ca21 * w01 + w02 * ca22 * w02)
    )
    var cv01 = s2t * (
        0.0
        + (0.0 + w00 * ca00 * w10 + w00 * ca01 * w11 + w00 * ca02 * w12)
        + (0.0 + w01 * ca10 * w10 + w01 * ca11 * w11 + w01 * ca12 * w12)
        + (0.0 + w02 * ca20 * w10 + w02 * ca21 * w11 + w02 * ca22 * w12)
    )
    var cv02 = s2t * (
        0.0
        + (0.0 + w00 * ca00 * w20 + w00 * ca01 * w21 + w00 * ca02 * w22)
        + (0.0 + w01 * ca10 * w20 + w01 * ca11 * w21 + w01 * ca12 * w22)
        + (0.0 + w02 * ca20 * w20 + w02 * ca21 * w21 + w02 * ca22 * w22)
    )
    var cv10 = s2t * (
        0.0
        + (0.0 + w10 * ca00 * w00 + w10 * ca01 * w01 + w10 * ca02 * w02)
        + (0.0 + w11 * ca10 * w00 + w11 * ca11 * w01 + w11 * ca12 * w02)
        + (0.0 + w12 * ca20 * w00 + w12 * ca21 * w01 + w12 * ca22 * w02)
    )
    var cv11 = s2t * (
        0.0
        + (0.0 + w10 * ca00 * w10 + w10 * ca01 * w11 + w10 * ca02 * w12)
        + (0.0 + w11 * ca10 * w10 + w11 * ca11 * w11 + w11 * ca12 * w12)
        + (0.0 + w12 * ca20 * w10 + w12 * ca21 * w11 + w12 * ca22 * w12)
    )
    var cv12 = s2t * (
        0.0
        + (0.0 + w10 * ca00 * w20 + w10 * ca01 * w21 + w10 * ca02 * w22)
        + (0.0 + w11 * ca10 * w20 + w11 * ca11 * w21 + w11 * ca12 * w22)
        + (0.0 + w12 * ca20 * w20 + w12 * ca21 * w21 + w12 * ca22 * w22)
    )
    var cv20 = s2t * (
        0.0
        + (0.0 + w20 * ca00 * w00 + w20 * ca01 * w01 + w20 * ca02 * w02)
        + (0.0 + w21 * ca10 * w00 + w21 * ca11 * w01 + w21 * ca12 * w02)
        + (0.0 + w22 * ca20 * w00 + w22 * ca21 * w01 + w22 * ca22 * w02)
    )
    var cv21 = s2t * (
        0.0
        + (0.0 + w20 * ca00 * w10 + w20 * ca01 * w11 + w20 * ca02 * w12)
        + (0.0 + w21 * ca10 * w10 + w21 * ca11 * w11 + w21 * ca12 * w12)
        + (0.0 + w22 * ca20 * w10 + w22 * ca21 * w11 + w22 * ca22 * w12)
    )
    var cv22 = s2t * (
        0.0
        + (0.0 + w20 * ca00 * w20 + w20 * ca01 * w21 + w20 * ca02 * w22)
        + (0.0 + w21 * ca10 * w20 + w21 * ca11 * w21 + w21 * ca12 * w22)
        + (0.0 + w22 * ca20 * w20 + w22 * ca21 * w21 + w22 * ca22 * w22)
    )

    # Perspective Jacobian at the centre.
    var tx = t[0]
    var ty = t[1]
    var tz = t[2]
    var tanx = uni[39]
    var tany = uni[40]
    var j00 = 1.0 / (tanx * -tz)
    var j02 = tx / (tanx * tz * tz)
    var j11 = 1.0 / (tany * tz)
    var j12 = -ty / (tany * tz * tz)

    var cn00 = (
        0.0
        + (0.0 + j00 * cv00 * j00 + j00 * cv01 * 0.0 + j00 * cv02 * j02)
        + (0.0 + 0.0 * cv10 * j00 + 0.0 * cv11 * 0.0 + 0.0 * cv12 * j02)
        + (0.0 + j02 * cv20 * j00 + j02 * cv21 * 0.0 + j02 * cv22 * j02)
    )
    var cn01 = (
        0.0
        + (0.0 + j00 * cv00 * 0.0 + j00 * cv01 * j11 + j00 * cv02 * j12)
        + (0.0 + 0.0 * cv10 * 0.0 + 0.0 * cv11 * j11 + 0.0 * cv12 * j12)
        + (0.0 + j02 * cv20 * 0.0 + j02 * cv21 * j11 + j02 * cv22 * j12)
    )
    var cn10 = (
        0.0
        + (0.0 + 0.0 * cv00 * j00 + 0.0 * cv01 * 0.0 + 0.0 * cv02 * j02)
        + (0.0 + j11 * cv10 * j00 + j11 * cv11 * 0.0 + j11 * cv12 * j02)
        + (0.0 + j12 * cv20 * j00 + j12 * cv21 * 0.0 + j12 * cv22 * j02)
    )
    var cn11 = (
        0.0
        + (0.0 + 0.0 * cv00 * 0.0 + 0.0 * cv01 * j11 + 0.0 * cv02 * j12)
        + (0.0 + j11 * cv10 * 0.0 + j11 * cv11 * j11 + j11 * cv12 * j12)
        + (0.0 + j12 * cv20 * 0.0 + j12 * cv21 * j11 + j12 * cv22 * j12)
    )

    var wf = uni[42]
    var hf = uni[43]
    var d0 = wf / 2.0
    var d1 = hf / 2.0
    var cp00 = cn00 * d0 * d0 + COV_DILATION_PX2
    var cp01 = cn01 * d0 * d1
    var cp11 = cn11 * d1 * d1 + COV_DILATION_PX2
    var cp10 = cp01
    var det = cp00 * cp11 - cp01 * cp10
    if det <= 0.0:
        return
    var conic0 = Float32(cp11 / det)
    var conic1 = Float32(-cp01 / det)
    var conic2 = Float32(cp00 / det)

    # Eigen-decomposition of the symmetric 2x2 (quad extent).
    var mid = (cp00 + cp11) / 2.0
    var half_diff = (cp00 - cp11) / 2.0
    var disc = sqrt(half_diff * half_diff + cp01 * cp01)
    var l1 = mid + disc
    var l2 = mid - disc
    if l2 < 0.0:
        l2 = 0.0
    var ax0 = Float64(1.0)
    var ax1 = Float64(0.0)
    if cp01 > 1e-12 or cp01 < -1e-12:
        var v0 = cp01
        var v1 = l1 - cp00
        var vn = sqrt(v0 * v0 + v1 * v1)
        ax0 = v0 / vn
        ax1 = v1 / vn
    elif cp00 >= cp11:
        ax0 = 1.0
        ax1 = 0.0
    else:
        ax0 = 0.0
        ax1 = 1.0
    var radii0 = QUAD_SIGMAS * sqrt(l1)
    var radii1 = QUAD_SIGMAS * sqrt(l2)

    var clip = apply_mat(uni, 16, centre0, centre1, centre2, 1.0)
    if clip[3] <= 0.0:
        return
    var ndc0 = clip[0] / clip[3]
    var ndc1 = clip[1] / clip[3]
    var centre_px0 = (ndc0 * 0.5 + 0.5) * wf
    var centre_px1 = (ndc1 * 0.5 + 0.5) * hf
    var z_ndc = Float32(clip[2] / clip[3])

    # SH colour for this camera (splat.rs `sh_colour`).
    var tc0 = uni[32] - centre0
    var tc1 = uni[33] - centre1
    var tc2 = uni[34] - centre2
    var len_ = sqrt(tc0 * tc0 + tc1 * tc1 + tc2 * tc2)
    if len_ <= 0.0:
        return
    var x = tc0 / len_
    var y = tc1 / len_
    var z = tc2 / len_
    var xx = x * x
    var yy = y * y
    var zz = z * z
    var b10 = -SH_C1 * y
    var b11 = SH_C1 * z
    var b12 = -SH_C1 * x
    var b20 = SH_C2_0 * x * y
    var b21 = SH_C2_1 * y * z
    var b22 = SH_C2_2 * (2.0 * zz - xx - yy)
    var b23 = SH_C2_3 * x * z
    var b24 = SH_C2_4 * (xx - yy)
    var b30 = SH_C3_0 * y * (3.0 * xx - yy)
    var b31 = SH_C3_1 * x * y * z
    var b32 = SH_C3_2 * y * (4.0 * zz - xx - yy)
    var b33 = SH_C3_3 * z * (2.0 * zz - 3.0 * xx - 3.0 * yy)
    var b34 = SH_C3_4 * x * (4.0 * zz - xx - yy)
    var b35 = SH_C3_5 * z * (xx - yy)
    var b36 = SH_C3_6 * x * (xx - 3.0 * yy)

    var col0 = Float32(0.0)
    var col1 = Float32(0.0)
    var col2 = Float32(0.0)
    for ch in range(3):
        var rbase = base + 64 + ch * 60
        var v = 0.5 + SH_C0 * Float64(load_f32(recs, base + 16 + ch * 4))
        v += b10 * Float64(load_f32(recs, rbase + 0))
        v += b11 * Float64(load_f32(recs, rbase + 4))
        v += b12 * Float64(load_f32(recs, rbase + 8))
        v += b20 * Float64(load_f32(recs, rbase + 12))
        v += b21 * Float64(load_f32(recs, rbase + 16))
        v += b22 * Float64(load_f32(recs, rbase + 20))
        v += b23 * Float64(load_f32(recs, rbase + 24))
        v += b24 * Float64(load_f32(recs, rbase + 28))
        v += b30 * Float64(load_f32(recs, rbase + 32))
        v += b31 * Float64(load_f32(recs, rbase + 36))
        v += b32 * Float64(load_f32(recs, rbase + 40))
        v += b33 * Float64(load_f32(recs, rbase + 44))
        v += b34 * Float64(load_f32(recs, rbase + 48))
        v += b35 * Float64(load_f32(recs, rbase + 52))
        v += b36 * Float64(load_f32(recs, rbase + 56))
        if v < 0.0:
            v = 0.0
        if ch == 0:
            col0 = Float32(v)
        elif ch == 1:
            col1 = Float32(v)
        else:
            col2 = Float32(v)

    store_f64(outb, obase + 0, centre_px0)
    store_f64(outb, obase + 8, centre_px1)
    store_f64(outb, obase + 16, radii0)
    store_f64(outb, obase + 24, radii1)
    store_f64(outb, obase + 32, ax0)
    store_f64(outb, obase + 40, ax1)
    store_f64(outb, obase + 48, depth)
    store_f32(outb, obase + 56, conic0)
    store_f32(outb, obase + 60, conic1)
    store_f32(outb, obase + 64, conic2)
    store_f32(outb, obase + 68, opacity)
    store_f32(outb, obase + 72, col0)
    store_f32(outb, obase + 76, col1)
    store_f32(outb, obase + 80, col2)
    store_f32(outb, obase + 84, z_ndc)
    store_u32(outb, obase + 88, 0)


def project_all[O: Origin, O2: MutOrigin](
    recs: Pointer[UInt8, O],
    uni: List[Float64],
    outb: Pointer[UInt8, O2],
    count: Int,
    alpha_cull: Float32,
):
    for i in range(count):
        project_one(recs, uni, outb, i, alpha_cull)


def main() raises:
    var args = argv()
    if len(args) != 5:
        raise Error("usage: splat_project_cpu IN OUT REPS DEVICE_ID (id ignored)")
    var in_path = String(args[1])
    var out_path = String(args[2])
    var reps = parse_nonnegative_int(String(args[3]))
    _ = parse_nonnegative_int(String(args[4]))
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
    var out_bytes = count * OUT_BYTES
    if len(bytes_in) != HEADER_BYTES + in_bytes:
        raise Error("input length does not match the header count")

    var alpha_cull = read_f32(bytes_in.unsafe_ptr(), 360)

    var uni = List[Float64](length=UNIFORM_COUNT, fill=0.0)
    for k in range(UNIFORM_COUNT):
        uni[k] = read_f64(bytes_in.unsafe_ptr(), 8 + k * 8)

    print("device: Mojo CPU (single-thread)")
    var recs = bytes_in.unsafe_ptr().unsafe_offset(HEADER_BYTES)
    var out_list = List[UInt8](length=out_bytes, fill=0)
    var outb = out_list.unsafe_ptr()

    # Correctness run (its output is written), then timed reps.
    project_all(recs, uni, outb, count, alpha_cull)
    var times = List[Float64]()
    for _ in range(reps):
        var s = perf_counter_ns()
        project_all(recs, uni, outb, count, alpha_cull)
        var e = perf_counter_ns()
        times.append(Float64(e - s) / 1.0e6)
    for a in range(len(times)):
        for b in range(a + 1, len(times)):
            if times[b] < times[a]:
                var tmp = times[a]
                times[a] = times[b]
                times[b] = tmp
    print("KERNEL_MEDIAN_MS", times[len(times) // 2])
    print("KERNEL_MIN_MS", times[0])

    with open(out_path, "w") as f:
        f.write_bytes(out_list)
    print("SPLAT_PROJECT_DONE", count)
