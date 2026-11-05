//! The uinput synthetic pen and the scripted battery
//! frames (SPEC SS7.3, ported from the D-970 spike): how
//! the production shim is verified on a machine with no
//! pen hardware.

use std::io;
use std::os::fd::OwnedFd;
use std::time::Duration;

use crate::sys;

/// The battery pen's device name (scan matches on it).
pub const BATTERY_PEN_NAME: &str = "torogao-pen-battery";
/// Battery pen axis maxima: X and Y in device units.
pub const BATTERY_AXIS_MAX: i32 = 1023;
/// Battery pen pressure maximum (raw device units).
pub const BATTERY_PRESSURE_MAX: i32 = 2047;

/// Event constants the feeder uses (linux/input-event-codes.h).
pub const EV_SYN: u16 = 0;
/// Key event class.
pub const EV_KEY: u16 = 1;
/// Absolute-axis event class.
pub const EV_ABS: u16 = 3;
/// Proximity of the pen tool.
pub const BTN_TOOL_PEN: u16 = 0x140;
/// Pen touching the surface.
pub const BTN_TOUCH: u16 = 0x14a;
/// X axis.
pub const ABS_X: u16 = 0;
/// Y axis.
pub const ABS_Y: u16 = 1;
/// Pressure axis.
pub const ABS_PRESSURE: u16 = 0x18;

/// One scripted frame: events delivered together, then a
/// SYN, then a pause.
#[derive(Debug, Clone)]
pub struct Frame {
    /// (type, code, value) triples, in order.
    pub events: Vec<(u16, u16, i32)>,
    /// Pause after this frame, in milliseconds.
    pub delay_ms: u64,
}

fn frame(events: Vec<(u16, u16, i32)>, delay_ms: u64) -> Frame {
    Frame { events, delay_ms }
}

/// The SS7.3 script: a proximity in/out, a 300-step
/// pressure ramp at constant Y (raw pressure 64 + 6k,
/// X 100 + 3k), and a two-point stroke — the same numbers
/// the spike proved, unchanged.
pub fn scripted_frames() -> Vec<Frame> {
    let mut frames: Vec<Frame> = Vec::new();
    // Proximity in, then out — no touch.
    frames.push(frame(
        vec![
            (EV_KEY, BTN_TOOL_PEN, 1),
            (EV_ABS, ABS_X, 100),
            (EV_ABS, ABS_Y, 100),
        ],
        8,
    ));
    frames.push(frame(vec![(EV_KEY, BTN_TOOL_PEN, 0)], 8));
    // Stroke 1: the pressure ramp (300 touch frames).
    frames.push(frame(
        vec![
            (EV_KEY, BTN_TOOL_PEN, 1),
            (EV_KEY, BTN_TOUCH, 1),
            (EV_ABS, ABS_X, 100),
            (EV_ABS, ABS_Y, 512),
            (EV_ABS, ABS_PRESSURE, 64),
        ],
        4,
    ));
    for step in 1..300i32 {
        frames.push(frame(
            vec![
                (EV_ABS, ABS_X, 100 + 3 * step),
                (EV_ABS, ABS_Y, 512),
                (EV_ABS, ABS_PRESSURE, 64 + 6 * step),
            ],
            4,
        ));
    }
    frames.push(frame(
        vec![(EV_KEY, BTN_TOUCH, 0), (EV_KEY, BTN_TOOL_PEN, 0)],
        8,
    ));
    // Stroke 2: two placements at constant pressure.
    frames.push(frame(
        vec![
            (EV_KEY, BTN_TOOL_PEN, 1),
            (EV_KEY, BTN_TOUCH, 1),
            (EV_ABS, ABS_X, 200),
            (EV_ABS, ABS_Y, 200),
            (EV_ABS, ABS_PRESSURE, 1000),
        ],
        4,
    ));
    frames.push(frame(
        vec![
            (EV_ABS, ABS_X, 700),
            (EV_ABS, ABS_Y, 700),
            (EV_ABS, ABS_PRESSURE, 1000),
        ],
        4,
    ));
    frames.push(frame(
        vec![(EV_KEY, BTN_TOUCH, 0), (EV_KEY, BTN_TOOL_PEN, 0)],
        8,
    ));
    frames
}

/// A created uinput pen device, destroyed on drop.
pub struct SyntheticPen {
    fd: OwnedFd,
    created: bool,
}

impl SyntheticPen {
    /// Create a virtual pen with the battery's name and
    /// ranges under /dev/uinput.
    pub fn create_battery_pen() -> io::Result<SyntheticPen> {
        Self::create(
            BATTERY_PEN_NAME,
            BATTERY_AXIS_MAX,
            BATTERY_AXIS_MAX,
            BATTERY_PRESSURE_MAX,
        )
    }

    /// Create a virtual pen with the given name and axis
    /// maxima.
    pub fn create(
        name: &str,
        x_max: i32,
        y_max: i32,
        pressure_max: i32,
    ) -> io::Result<SyntheticPen> {
        let fd = sys::open_uinput()?;
        // SAFETY: fd is a fresh /dev/uinput descriptor,
        // not yet created as a device.
        unsafe { sys::create_pen_device(&fd, name, x_max, y_max, pressure_max)? };
        Ok(SyntheticPen { fd, created: true })
    }

    /// Emit one event (followed by nothing — call `syn`
    /// to complete the frame).
    pub fn emit(&mut self, type_: u16, code: u16, value: i32) -> io::Result<()> {
        let event = sys::InputEvent {
            tv_sec: 0,
            tv_usec: 0,
            type_,
            code,
            value,
        };
        // SAFETY: InputEvent is repr(C) and POD; its bytes
        // are exactly the kernel's input_event image.
        let bytes = unsafe {
            std::slice::from_raw_parts(
                (&raw const event).cast::<u8>(),
                std::mem::size_of::<sys::InputEvent>(),
            )
        };
        sys::write_all_fd(&self.fd, bytes)
    }

    /// Emit a SYN_REPORT, completing the current frame.
    pub fn syn(&mut self) -> io::Result<()> {
        self.emit(EV_SYN, 0, 0)
    }

    /// Feed one scripted frame: its events, a SYN, then
    /// the frame's pause.
    pub fn feed_frame(&mut self, frame: &Frame) -> io::Result<()> {
        for (type_, code, value) in &frame.events {
            self.emit(*type_, *code, *value)?;
        }
        self.syn()?;
        std::thread::sleep(Duration::from_millis(frame.delay_ms));
        Ok(())
    }

    /// Feed the whole scripted battery.
    pub fn feed_script(&mut self) -> io::Result<()> {
        for frame in scripted_frames() {
            self.feed_frame(&frame)?;
        }
        Ok(())
    }
}

impl Drop for SyntheticPen {
    fn drop(&mut self) {
        if self.created {
            // SAFETY: this device was created by us and is
            // destroyed exactly once, here.
            let _ = unsafe { sys::destroy_device(&self.fd) };
        }
    }
}
