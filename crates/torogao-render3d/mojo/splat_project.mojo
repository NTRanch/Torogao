# splat_project.mojo — Mojo/PTX prototype of torogao-render3d's splat
# projection (the projection half of the CPU splat pack), built for the
# Mojo+PTX evaluation lane (2026-11-27). NOT production code: a measured
# prototype gated against the CPU reference, record-for-record.
#
# Contract
# --------
# Input file (little-endian, written by the Rust harness
# `splat_mojo_bench` in the same crate):
#   [0..8)     u64  splat count N (1 <= N <= 2^31)
#   [8..360)   44 x f64 uniforms:
#                [0..16)  view matrix m (column-major, m[j*4+i])
#                [16..32) view-projection matrix m (same layout)
#                [32..35) camera position (world)
#                [35..38) layer translation
#                [38]     layer uniform scale
#                [39]     tan_half_x   [40] tan_half_y
#                [41]     near plane   [42] width px (as f64)
#                [43]     height px (as f64)
#   [360..364) u32  bit pattern of the f32 alpha-cull threshold
#   [368..)    N x 256-byte GpuSplatRecord (the crate's own resident
#              record layout: opacity and scales PRE-ACTIVATED on the
#              host by the pack's own functions, so this kernel needs
#              no transcendentals and every operation is IEEE-exact
#              +, -, *, /, sqrt on f64 — the same property the Vulkan
#              projection relies on for its bit-exact gate).
# Output file: N x 96-byte projected records in asset order:
#   f64 centre_px[2], radii[2], axis1[2], depth;
#   f32 conic[3], opacity, colour[3], z_ndc; u32 culled; u32 pad.
# Culled splats carry culled=1 (their remaining fields are not
# written and carry no meaning).
#
# Scope: identity layer rotation, unlit (the bench's config A — the
# twin-witness configuration). Layer rotation and Rung-4 light are
# host-side / separate paths in the CPU pack and are out of scope here.
#
# Determinism: every arithmetic expression below transcribes the
# Rust source's association order exactly (sums fold in index order,
# products associate as written). If the compiler contracts a
# multiply-add into an FMA, the record-for-record gate against the
# CPU reference will show it in the low bits — that is what the gate
# is for.
#
# Bounds: N validated against the input file length before any
# allocation; grid sized from N; one thread per splat; no loops over
# unbounded data (the only loops are the fixed 3x3 covariance sums
# and bounded host-side copies).

from max.gpu import thread_idx, block_idx, block_dim
from max.gpu.host import DeviceContext
from std.sys import argv
from std.time import perf_counter_ns
from std.math import sqrt

# SH basis constants, transcribed from crates/torogao-render3d/src/
# splat.rs (Inria 3DGS reference values, f64 literals).
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


def store_f64(p: Pointer[UInt8, MutAnyOrigin], off: Int, v: Float64):
    p.unsafe_offset(off).unsafe_bitcast[Float64]().unsafe_store(val=v)


def store_f32(p: Pointer[UInt8, MutAnyOrigin], off: Int, v: Float32):
    p.unsafe_offset(off).unsafe_bitcast[Float32]().unsafe_store(val=v)


def store_u32(p: Pointer[UInt8, MutAnyOrigin], off: Int, v: UInt32):
    p.unsafe_offset(off).unsafe_bitcast[UInt32]().unsafe_store(val=v)


def apply_mat[O: Origin](
    m: Pointer[Float64, O],
    base: Int,
    v0: Float64,
    v1: Float64,
    v2: Float64,
    v3: Float64,
) -> Tuple[Float64, Float64, Float64, Float64]:
    # Column-major Mat4 times vec4, the crate math.rs `apply`
    # association: ((p0 + p1) + p2) + p3 per row.
    return (
        m.unsafe_offset(base + 0).unsafe_load() * v0
        + m.unsafe_offset(base + 4).unsafe_load() * v1
        + m.unsafe_offset(base + 8).unsafe_load() * v2
        + m.unsafe_offset(base + 12).unsafe_load() * v3,
        m.unsafe_offset(base + 1).unsafe_load() * v0
        + m.unsafe_offset(base + 5).unsafe_load() * v1
        + m.unsafe_offset(base + 9).unsafe_load() * v2
        + m.unsafe_offset(base + 13).unsafe_load() * v3,
        m.unsafe_offset(base + 2).unsafe_load() * v0
        + m.unsafe_offset(base + 6).unsafe_load() * v1
        + m.unsafe_offset(base + 10).unsafe_load() * v2
        + m.unsafe_offset(base + 14).unsafe_load() * v3,
        m.unsafe_offset(base + 3).unsafe_load() * v0
        + m.unsafe_offset(base + 7).unsafe_load() * v1
        + m.unsafe_offset(base + 11).unsafe_load() * v2
        + m.unsafe_offset(base + 15).unsafe_load() * v3,
    )


def project_kernel(
    recs: Pointer[UInt8, MutAnyOrigin],
    uni: Pointer[Float64, MutAnyOrigin],
    outb: Pointer[UInt8, MutAnyOrigin],
    count: Int64,
    alpha_cull: Float32,
):
    var idx = Int64(block_idx.x) * Int64(block_dim.x) + Int64(thread_idx.x)
    if idx >= count:
        return
    var i = Int(idx)
    var base = i * RECORD_BYTES
    var obase = i * OUT_BYTES

    # Culled-by-default record; survivors overwrite the flag last.
    store_u32(outb, obase + 88, 1)

    var opacity = load_f32(recs, base + 12)
    if opacity < alpha_cull:
        return

    var tr_scale = uni.unsafe_offset(38).unsafe_load()
    var centre0 = (
        Float64(load_f32(recs, base + 0)) * tr_scale
        + uni.unsafe_offset(35).unsafe_load()
    )
    var centre1 = (
        Float64(load_f32(recs, base + 4)) * tr_scale
        + uni.unsafe_offset(36).unsafe_load()
    )
    var centre2 = (
        Float64(load_f32(recs, base + 8)) * tr_scale
        + uni.unsafe_offset(37).unsafe_load()
    )

    var t = apply_mat(uni, 0, centre0, centre1, centre2, 1.0)
    var depth = -t[2]
    if depth < uni.unsafe_offset(41).unsafe_load():
        return

    # Covariance from the splat's quaternion + activated scales
    # (splat.rs `covariance`, f64 throughout).
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

    # cov_asset[i][j] = sum_k r[i][k] * s2[k] * r[j][k], folded from 0.
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
    var w00 = uni.unsafe_offset(0).unsafe_load()
    var w01 = uni.unsafe_offset(4).unsafe_load()
    var w02 = uni.unsafe_offset(8).unsafe_load()
    var w10 = uni.unsafe_offset(1).unsafe_load()
    var w11 = uni.unsafe_offset(5).unsafe_load()
    var w12 = uni.unsafe_offset(9).unsafe_load()
    var w20 = uni.unsafe_offset(2).unsafe_load()
    var w21 = uni.unsafe_offset(6).unsafe_load()
    var w22 = uni.unsafe_offset(10).unsafe_load()

    # cov_view[i][j] = s2t * sum_a sum_b w3[i][a]*ca[a][b]*w3[j][b],
    # both sums folded from 0, in a/b index order.
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
    var tanx = uni.unsafe_offset(39).unsafe_load()
    var tany = uni.unsafe_offset(40).unsafe_load()
    var j00 = 1.0 / (tanx * -tz)
    var j02 = tx / (tanx * tz * tz)
    var j11 = 1.0 / (tany * tz)
    var j12 = -ty / (tany * tz * tz)

    # cov_ndc[i][k] = sum_a sum_b j[i][a]*cv[a][b]*j[k][b]; the j
    # matrix is [[j00,0,j02],[0,j11,j12]] — zero terms contribute
    # exact zeros and are folded in the same order as the source.
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

    var wf = uni.unsafe_offset(42).unsafe_load()
    var hf = uni.unsafe_offset(43).unsafe_load()
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
    var tc0 = uni.unsafe_offset(32).unsafe_load() - centre0
    var tc1 = uni.unsafe_offset(33).unsafe_load() - centre1
    var tc2 = uni.unsafe_offset(34).unsafe_load() - centre2
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


def main() raises:
    var args = argv()
    if len(args) != 5:
        raise Error("usage: splat_project IN OUT REPS DEVICE_ID")
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
    var out_bytes = count * OUT_BYTES
    if len(bytes_in) != HEADER_BYTES + in_bytes:
        raise Error("input length does not match the header count")

    var alpha_cull = read_f32(bytes_in.unsafe_ptr(), 360)

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
        var dev_out = ctx.enqueue_create_buffer[DType.uint8](out_bytes)

        var grid = (count + 255) // 256
        # Warmup (also the correctness run whose output is written).
        ctx.enqueue_function[project_kernel](
            dev_in,
            dev_uni,
            dev_out,
            Int64(count),
            alpha_cull,
            grid_dim=grid,
            block_dim=256,
        )
        ctx.synchronize()

        var times = List[Float64]()
        for _ in range(reps):
            var s = perf_counter_ns()
            ctx.enqueue_function[project_kernel](
                dev_in,
                dev_uni,
                dev_out,
                Int64(count),
                alpha_cull,
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

        var host_out = ctx.enqueue_create_host_buffer[DType.uint8](out_bytes)
        var t2 = perf_counter_ns()
        ctx.enqueue_copy(host_out, dev_out)
        ctx.synchronize()
        var t3 = perf_counter_ns()
        print("D2H_MS", Float64(t3 - t2) / 1.0e6)

        var out_list = List[UInt8](length=out_bytes, fill=0)
        var hout_p = host_out.unsafe_ptr()
        var olist_p = out_list.unsafe_ptr()
        var n8_out = out_bytes // 8
        for k in range(n8_out):
            olist_p.unsafe_offset(k * 8).unsafe_bitcast[UInt64]().unsafe_store(
                val=hout_p.unsafe_offset(k * 8).unsafe_bitcast[UInt64]().unsafe_load()
            )
        for k in range(n8_out * 8, out_bytes):
            olist_p.unsafe_offset(k).unsafe_store(
                val=hout_p.unsafe_offset(k).unsafe_load()
            )
        with open(out_path, "w") as f:
            f.write_bytes(out_list)
    print("SPLAT_PROJECT_DONE", count)
