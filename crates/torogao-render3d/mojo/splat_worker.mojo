# splat_worker.mojo — torogao-render3d's persistent Mojo kernel
# worker (invocation-overhead cure, 2026-11-27).
#
# Why this exists: the one-shot kernel programs
# (splat_project.mojo / splat_rotate.mojo) pay the MAX runtime
# start + DeviceContext creation on EVERY invocation — measured
# at ~2.1-3.2 s of process wall against 5-180 ms of transfers +
# kernel on the full 1,157,141-splat witness (adoption gates and
# the 2026-11-27 size-scaling probe: a 1,000-splat invocation
# walls the same ~3 s as a full-witness one). The pack route
# pays that per layer, and the rotated route pays it twice.
# This worker pays it ONCE: it creates one DeviceContext, then
# serves pack requests over its stdin/stdout until EOF. The
# kernels it runs are the very functions the one-shot programs
# run — imported from splat_project.mojo / splat_rotate.mojo,
# not transcribed — so the bit-exactness gates carry over: the
# file round-trip the one-shots make was a byte vessel, and the
# fused path's only change is that the rotated records stay in
# a device buffer between the two kernels instead of crossing
# the host twice.
#
# Build only via mojo/build.sh (the --fp-mode contract=off pin
# is load-bearing, exactly as for the one-shot kernels).
#
# Protocol (raw fds 0/1, little-endian; the Rust host in
# src/splat_mojo.rs is the only client):
#   Request:  [u64 payload_len][u8 kind][payload]
#     kind 1 (PROJECT): payload is byte-identical to the
#       input file splat_project.mojo reads (480-byte header +
#       N x 256-byte records).
#     kind 2 (ROTATE_PROJECT): payload is the 776-byte
#       splat_rotate header, then the 480-byte splat_project
#       header, then the N x 256-byte records (both headers'
#       counts must agree). The rotation kernel's output feeds
#       the projection kernel device-side.
#   Response: [u64 payload_len][u8 status][payload]
#     status 0: payload is byte-identical to the output file
#       the one-shot projection program writes (N x 96 bytes).
#     status 1: the request was rejected (validation); the
#       payload is empty and the worker keeps serving — the
#       host falls back for that request.
#   EOF on stdin at a frame boundary ends the worker cleanly
#   (exit 0): the worker dies with its host process, never
#   outlives it. EOF mid-frame or an implausible frame length
#   is fatal (nonzero exit); the host detects the death and
#   falls back. Nothing but protocol frames is ever written
#   to stdout; the one diagnostic line goes to stderr.
#
# Bounds: payload_len capped at 2^33; counts validated exactly
# as the one-shot programs validate them (1..2^31, payload
# length must match the headers' count exactly) before any
# kernel launch; device/host buffers grow to the largest
# request seen and are reused.

from max.gpu.host import DeviceContext
from std.ffi import external_call
from std.sys import argv

from splat_project import (
    HEADER_BYTES as PROJ_HEADER_BYTES,
    OUT_BYTES,
    RECORD_BYTES,
    UNIFORM_COUNT as PROJ_UNIFORM_COUNT,
    project_kernel,
    read_f32,
    read_f64,
    read_u64,
)
from splat_rotate import (
    HEADER_BYTES as ROTATE_HEADER_BYTES,
    UNIFORM_COUNT as ROTATE_UNIFORM_COUNT,
    rotate_kernel,
)

comptime KIND_PROJECT = 1
comptime KIND_ROTATE_PROJECT = 2
comptime MAX_PAYLOAD = 8589934592  # 2^33


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


def read_exact[O: Origin](fd: Int32, p: Pointer[UInt8, O], n: Int) -> Bool:
    """Fill exactly n bytes from fd; False on EOF/error first."""
    var off = 0
    while off < n:
        var r = external_call["read", Int](fd, p.unsafe_offset(off), n - off)
        if r <= 0:
            return False
        off += r
    return True


def write_all[O: Origin](fd: Int32, p: Pointer[UInt8, O], n: Int) -> Bool:
    """Write exactly n bytes to fd; False on error."""
    var off = 0
    while off < n:
        var w = external_call["write", Int](fd, p.unsafe_offset(off), n - off)
        if w <= 0:
            return False
        off += w
    return True


def bulk_copy[O1: Origin, O2: Origin](
    dst: Pointer[UInt8, O1], src: Pointer[UInt8, O2], n: Int
):
    external_call["memcpy", NoneType](dst, src, n)


def respond[O: Origin](status: UInt8, payload: Pointer[UInt8, O], n: Int) -> Bool:
    var hdr = List[UInt8](length=9, fill=0)
    var len64 = UInt64(n)
    for k in range(8):
        hdr[k] = UInt8((len64 >> UInt64(8 * k)) & 0xFF)
    hdr[8] = status
    if not write_all(Int32(1), hdr.unsafe_ptr(), 9):
        return False
    if n > 0:
        return write_all(Int32(1), payload, n)
    return True


def main() raises:
    var args = argv()
    if len(args) != 2:
        raise Error("usage: splat_worker DEVICE_ID")
    var device_id = parse_nonnegative_int(String(args[1]))

    with DeviceContext(device_id=device_id) as ctx:
        # Grow-to-fit buffers, reused across requests.
        var rec_cap = 1
        var out_cap = 1
        var dev_in = ctx.enqueue_create_buffer[DType.uint8](rec_cap)
        var dev_rot = ctx.enqueue_create_buffer[DType.uint8](rec_cap)
        var dev_out = ctx.enqueue_create_buffer[DType.uint8](out_cap)
        var dev_uni = ctx.enqueue_create_buffer[DType.float64](
            ROTATE_UNIFORM_COUNT
        )
        var host_in = ctx.enqueue_create_host_buffer[DType.uint8](rec_cap)
        var host_out = ctx.enqueue_create_host_buffer[DType.uint8](out_cap)
        var payload = List[UInt8]()
        var out_list = List[UInt8]()

        var ready = List[UInt8]()
        for b in "SPLAT_WORKER_READY\n".as_bytes():
            ready.append(b)
        _ = write_all(Int32(2), ready.unsafe_ptr(), len(ready))

        while True:
            # Frame prefix: 8-byte length, then 1-byte kind. A
            # clean EOF before the first length byte ends the
            # worker; anything partial is fatal.
            var len_buf = List[UInt8](length=8, fill=0)
            var first = external_call["read", Int](
                Int32(0), len_buf.unsafe_ptr(), 1
            )
            if first == 0:
                return  # host closed the pipe: clean shutdown
            if first < 0:
                raise Error("stdin read failed")
            if not read_exact(Int32(0), len_buf.unsafe_ptr().unsafe_offset(1), 7):
                raise Error("EOF inside a frame length")
            var payload_len = Int(read_u64(len_buf.unsafe_ptr(), 0))
            if payload_len < 1 or payload_len > MAX_PAYLOAD:
                raise Error("frame length out of bounds")
            var kind_buf = List[UInt8](length=1, fill=0)
            if not read_exact(Int32(0), kind_buf.unsafe_ptr(), 1):
                raise Error("EOF inside a frame kind")
            var kind = Int(kind_buf[0])
            if len(payload) < payload_len:
                payload = List[UInt8](length=payload_len, fill=0)
            if not read_exact(Int32(0), payload.unsafe_ptr(), payload_len):
                raise Error("EOF inside a frame payload")

            # Validate the request exactly as the one-shot
            # programs validate their input files. A rejection
            # answers status 1 and serving continues; the host
            # falls back for that request.
            var pp = payload.unsafe_ptr()
            var count = 0
            var records_off = 0
            var ok = True
            if kind == KIND_PROJECT:
                records_off = PROJ_HEADER_BYTES
                if payload_len < PROJ_HEADER_BYTES:
                    ok = False
                else:
                    count = Int(read_u64(pp, 0))
            elif kind == KIND_ROTATE_PROJECT:
                records_off = ROTATE_HEADER_BYTES + PROJ_HEADER_BYTES
                if payload_len < records_off:
                    ok = False
                else:
                    count = Int(read_u64(pp, 0))
                    var proj_count = Int(
                        read_u64(pp.unsafe_offset(ROTATE_HEADER_BYTES), 0)
                    )
                    if proj_count != count:
                        ok = False
            else:
                ok = False
            if ok:
                if count < 1 or count > 2147483647:
                    ok = False
                elif payload_len != records_off + count * RECORD_BYTES:
                    ok = False
            if not ok:
                if not respond(UInt8(1), payload.unsafe_ptr(), 0):
                    raise Error("stdout write failed")
                continue

            var in_bytes = count * RECORD_BYTES
            var out_bytes = count * OUT_BYTES
            if in_bytes > rec_cap:
                rec_cap = in_bytes
                dev_in = ctx.enqueue_create_buffer[DType.uint8](rec_cap)
                dev_rot = ctx.enqueue_create_buffer[DType.uint8](rec_cap)
                host_in = ctx.enqueue_create_host_buffer[DType.uint8](rec_cap)
            if out_bytes > out_cap:
                out_cap = out_bytes
                dev_out = ctx.enqueue_create_buffer[DType.uint8](out_cap)
                host_out = ctx.enqueue_create_host_buffer[DType.uint8](out_cap)

            bulk_copy(
                host_in.unsafe_ptr(),
                pp.unsafe_offset(records_off),
                in_bytes,
            )
            ctx.enqueue_copy(dev_in, host_in)

            var grid = (count + 255) // 256
            if kind == KIND_ROTATE_PROJECT:
                with dev_uni.map_to_host() as mapped:
                    for k in range(ROTATE_UNIFORM_COUNT):
                        mapped[k] = read_f64(pp, 8 + k * 8)
                ctx.enqueue_function[rotate_kernel](
                    dev_in,
                    dev_uni,
                    dev_rot,
                    Int64(count),
                    grid_dim=grid,
                    block_dim=256,
                )
                ctx.synchronize()

            var proj_hdr = pp.unsafe_offset(
                records_off - PROJ_HEADER_BYTES
            )
            var alpha_cull = read_f32(proj_hdr, 472)
            with dev_uni.map_to_host() as mapped:
                for k in range(PROJ_UNIFORM_COUNT):
                    mapped[k] = read_f64(proj_hdr, 8 + k * 8)
            if kind == KIND_ROTATE_PROJECT:
                ctx.enqueue_function[project_kernel](
                    dev_rot,
                    dev_uni,
                    dev_out,
                    Int64(count),
                    alpha_cull,
                    grid_dim=grid,
                    block_dim=256,
                )
            else:
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

            ctx.enqueue_copy(host_out, dev_out)
            ctx.synchronize()
            if len(out_list) < out_bytes:
                out_list = List[UInt8](length=out_bytes, fill=0)
            bulk_copy(out_list.unsafe_ptr(), host_out.unsafe_ptr(), out_bytes)
            if not respond(UInt8(0), out_list.unsafe_ptr(), out_bytes):
                raise Error("stdout write failed")
