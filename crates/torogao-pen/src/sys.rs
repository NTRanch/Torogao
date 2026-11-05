//! The crate's only unsafe surface: Linux evdev/uinput
//! ioctls and file opens, ported from the D-970 spike
//! (SPEC SS7), which proved them against the real kernel.
//! Every wrapper here is safe; the single unsafe helper
//! carries its contract in its docs.

use std::ffi::c_void;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;

// ioctl request construction (asm-generic layout).
const IOC_NRSHIFT: u32 = 0;
const IOC_TYPESHIFT: u32 = 8;
const IOC_SIZESHIFT: u32 = 16;
const IOC_DIRSHIFT: u32 = 30;
const IOC_READ: u32 = 2;
const IOC_WRITE: u32 = 1;

const fn ioc(dir: u32, ty: u8, nr: u32, size: usize) -> libc::c_ulong {
    let value = (dir << IOC_DIRSHIFT)
        | ((ty as u32) << IOC_TYPESHIFT)
        | (nr << IOC_NRSHIFT)
        | ((size as u32) << IOC_SIZESHIFT);
    value as libc::c_ulong
}

const fn ior(ty: u8, nr: u32, size: usize) -> libc::c_ulong {
    ioc(IOC_READ, ty, nr, size)
}

const fn iow(ty: u8, nr: u32, size: usize) -> libc::c_ulong {
    ioc(IOC_WRITE, ty, nr, size)
}

/// EVIOCGNAME(len)
fn eviocgname(len: usize) -> libc::c_ulong {
    ior(b'E', 0x06, len)
}

/// EVIOCGBIT(ev, len)
fn eviocgbit(ev: u32, len: usize) -> libc::c_ulong {
    ior(b'E', 0x20 + ev, len)
}

/// EVIOCGABS(abs)
fn eviocgabs(abs: u32) -> libc::c_ulong {
    ior(b'E', 0x40 + abs, std::mem::size_of::<InputAbsInfo>())
}

/// UI_SET_EVBIT (100) / UI_SET_KEYBIT (101) /
/// UI_SET_ABSBIT (103) — the kernel's numbering has a
/// gap at 102 (RELBIT), which the D-970 spike's feeder
/// documents by using the raw numbers.
pub fn ui_set_bit(nr: u32) -> libc::c_ulong {
    iow(b'U', nr, std::mem::size_of::<libc::c_int>())
}

/// UI_DEV_CREATE / UI_DEV_DESTROY: plain `_IO` requests
/// (direction bits zero) — the spike's constants; an
/// `_IOW` encoding here is a different request and the
/// kernel answers EINVAL.
pub const UI_DEV_CREATE: libc::c_ulong = ioc(0, b'U', 1, 0);
pub const UI_DEV_DESTROY: libc::c_ulong = ioc(0, b'U', 2, 0);

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct InputAbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

/// The kernel's `struct input_event` on 64-bit Linux:
/// a 16-byte timeval, then type/code/value.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct InputEvent {
    /// Seconds since the epoch (kernel clock).
    pub tv_sec: i64,
    /// Microseconds within the second.
    pub tv_usec: i64,
    /// Event type (EV_SYN / EV_KEY / EV_ABS).
    pub type_: u16,
    /// Event code (ABS_X, BTN_TOUCH, ...).
    pub code: u16,
    /// Event value.
    pub value: i32,
}

/// One axis's reported range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsInfo {
    /// Current value at query time.
    pub value: i32,
    /// Axis minimum.
    pub minimum: i32,
    /// Axis maximum.
    pub maximum: i32,
    /// Axis resolution (units per mm, 0 when unstated).
    pub resolution: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct UInputUserDev {
    /// The device name (80 bytes, NUL-padded).
    pub name: [u8; 80],
    /// bustype / vendor / product / version.
    pub id: [u16; 4],
    /// ff_effects_max (unused: 0).
    pub ff_effects_max: u32,
    /// Per-axis maxima (ABS_CNT = 64 slots).
    pub absmax: [i32; 64],
    /// Per-axis minima.
    pub absmin: [i32; 64],
    /// Per-axis fuzz.
    pub absfuzz: [i32; 64],
    /// Per-axis flat.
    pub absflat: [i32; 64],
}

impl UInputUserDev {
    /// The legacy setup record for a pen, with the
    /// spike's proven id values (BUS_USB, arbitrary
    /// vendor/product — the device is local and
    /// transient).
    pub fn pen(name: &str, x_max: i32, y_max: i32, pressure_max: i32) -> UInputUserDev {
        let mut dev = UInputUserDev {
            name: [0; 80],
            id: [0x03, 0x28bd, 0x9700, 1],
            ff_effects_max: 0,
            absmax: [0; 64],
            absmin: [0; 64],
            absfuzz: [0; 64],
            absflat: [0; 64],
        };
        let take = name.len().min(79);
        dev.name[..take].copy_from_slice(&name.as_bytes()[..take]);
        dev.absmax[0] = x_max;
        dev.absmax[1] = y_max;
        dev.absmax[24] = pressure_max;
        dev
    }
}

/// Run one ioctl with a pointer argument.
///
/// # Safety
/// The caller guarantees `arg` points at a value of the
/// type the kernel expects for `request` on `fd`, valid
/// for the call's duration. Every call site in this crate
/// passes a pointer to a live local of exactly the
/// kernel's struct for that request.
unsafe fn ioctl_ptr<T>(fd: RawFd, request: libc::c_ulong, arg: *mut T) -> io::Result<()> {
    // SAFETY: per this function's contract, discharged at
    // each call site below.
    let rc = unsafe { libc::ioctl(fd, request, arg as *mut c_void) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Run one ioctl with an integer argument (bit-setting).
///
/// # Safety
/// The caller guarantees `fd` is an open uinput descriptor
/// and `nr` selects a UI_SET_*BIT request.
unsafe fn ioctl_bit(fd: RawFd, nr: u32, bit: i32) -> io::Result<()> {
    // SAFETY: per this function's contract; call sites pass
    // a uinput fd and a UI_SET_*BIT number.
    let rc = unsafe { libc::ioctl(fd, ui_set_bit(nr), bit as libc::c_int) };
    if rc < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Open a device node read-only, non-blocking, close-on-exec.
pub fn open_readonly(path: &Path) -> io::Result<OwnedFd> {
    open_with_flags(path, libc::O_RDONLY)
}

/// Open /dev/uinput read-write, non-blocking, close-on-exec.
pub fn open_uinput() -> io::Result<OwnedFd> {
    open_with_flags(Path::new("/dev/uinput"), libc::O_RDWR)
}

fn open_with_flags(path: &Path, access: libc::c_int) -> io::Result<OwnedFd> {
    let c_path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path contains a NUL"))?;
    // SAFETY: c_path is a live NUL-terminated string for
    // the call; the returned descriptor is owned by us.
    let fd = unsafe { libc::open(c_path.as_ptr(), access | libc::O_NONBLOCK | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fd was just opened by us and not yet shared.
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// The device's EVIOCGNAME, when it answers.
pub fn device_name(fd: &OwnedFd) -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: buf is a live 256-byte array; EVIOCGNAME(256)
    // writes at most 256 bytes into it.
    let ok = unsafe { ioctl_ptr(fd.as_raw_fd(), eviocgname(buf.len()), buf.as_mut_ptr()) };
    ok.ok()?;
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    Some(String::from_utf8_lossy(&buf[..end]).into_owned())
}

/// One axis's range, when the device has that axis.
pub fn abs_info(fd: &OwnedFd, code: u16) -> Option<AbsInfo> {
    let mut info = InputAbsInfo::default();
    // SAFETY: info is a live InputAbsInfo; EVIOCGABS fills
    // exactly that struct for the axis.
    let ok = unsafe { ioctl_ptr(fd.as_raw_fd(), eviocgabs(u32::from(code)), &mut info) };
    ok.ok()?;
    Some(AbsInfo {
        value: info.value,
        minimum: info.minimum,
        maximum: info.maximum,
        resolution: info.resolution,
    })
}

/// Whether the device reports any code in an event class
/// (EVIOCGBIT over the class's bit array).
pub fn class_has_code(fd: &OwnedFd, class: u16, code: u16) -> bool {
    let mut bits = [0u8; 128];
    let request = eviocgbit(u32::from(class), bits.len());
    // SAFETY: bits is a live 128-byte array; EVIOCGBIT
    // writes at most bits.len() bytes.
    let ok = unsafe { ioctl_ptr(fd.as_raw_fd(), request, bits.as_mut_ptr()) };
    if ok.is_err() {
        return false;
    }
    let byte = usize::from(code / 8);
    byte < bits.len() && (bits[byte] >> (code % 8)) & 1 == 1
}

/// Read available bytes from a non-blocking descriptor.
/// WouldBlock maps to an empty read, not an error.
pub fn read_some(fd: &OwnedFd, buf: &mut [u8]) -> io::Result<usize> {
    // SAFETY: buf is a live mutable slice; read fills at
    // most buf.len() bytes and the fd is ours.
    let n = unsafe { libc::read(fd.as_raw_fd(), buf.as_mut_ptr() as *mut c_void, buf.len()) };
    if n < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::WouldBlock {
            return Ok(0);
        }
        return Err(err);
    }
    Ok(n as usize)
}

/// Wait up to `timeout_ms` for the descriptor to be
/// readable; true when it is.
pub fn poll_readable(fd: &OwnedFd, timeout_ms: i32) -> io::Result<bool> {
    let mut pfd = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: pfd is a live pollfd for the call.
    let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(rc > 0 && (pfd.revents & libc::POLLIN) != 0)
}

/// Write a whole buffer through a descriptor (uinput
/// event feeding).
pub fn write_all_fd(fd: &OwnedFd, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        // SAFETY: bytes is a live slice; write consumes at
        // most bytes.len() from it and the fd is ours.
        let n =
            unsafe { libc::write(fd.as_raw_fd(), bytes.as_ptr() as *const c_void, bytes.len()) };
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        bytes = &bytes[n as usize..];
    }
    Ok(())
}

/// Configure a uinput fd as a pen and create the
/// device, in the D-970 spike's proven shape: bit-setting
/// ioctls (EVBIT 100, KEYBIT 101, ABSBIT 103), then the
/// legacy `uinput_user_dev` record written to the fd,
/// then UI_DEV_CREATE.
///
/// # Safety
/// `fd` must be an open /dev/uinput descriptor not yet
/// created as a device. The caller owns its lifecycle
/// (destroy via `destroy_device` before close).
pub unsafe fn create_pen_device(
    fd: &OwnedFd,
    name: &str,
    x_max: i32,
    y_max: i32,
    pressure_max: i32,
) -> io::Result<()> {
    let raw = fd.as_raw_fd();
    // SAFETY: fd is a uinput descriptor per the contract;
    // each bit-setting call passes a valid bit number.
    unsafe {
        for ev in [0i32, 1, 3] {
            ioctl_bit(raw, 100, ev)?;
        }
        for key in [320i32, 330] {
            ioctl_bit(raw, 101, key)?;
        }
        for axis in [0i32, 1, 24] {
            ioctl_bit(raw, 103, axis)?;
        }
    }
    let dev = UInputUserDev::pen(name, x_max, y_max, pressure_max);
    // SAFETY: UInputUserDev is repr(C) and POD; its bytes
    // are exactly the kernel's uinput_user_dev image,
    // which the legacy create path reads from the fd.
    let bytes = unsafe {
        std::slice::from_raw_parts(
            (&raw const dev).cast::<u8>(),
            std::mem::size_of::<UInputUserDev>(),
        )
    };
    write_all_fd(fd, bytes)?;
    // SAFETY: same contract as above.
    unsafe { ioctl_ptr(raw, UI_DEV_CREATE, std::ptr::null_mut::<u8>()) }
}

/// Destroy a created uinput device.
///
/// # Safety
/// `fd` must be a uinput descriptor whose device was
/// created and not yet destroyed.
pub unsafe fn destroy_device(fd: &OwnedFd) -> io::Result<()> {
    // SAFETY: per the contract above.
    unsafe { ioctl_ptr(fd.as_raw_fd(), UI_DEV_DESTROY, std::ptr::null_mut::<u8>()) }
}
