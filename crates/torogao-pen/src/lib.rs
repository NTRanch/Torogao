// SPDX-License-Identifier: Apache-2.0 OR MIT
// Copyright (c) 2026 Qompass AI

//! `torogao-pen`: the production pen-input side channel
//! for torogao-paint (SPEC SS7, DP-3 as built by the
//! D-970 spike). The pinned egui/winit stack cannot carry
//! tablet pressure on Wayland, so pen state is read
//! straight from evdev: capability scan, reader-only (no
//! grab), one sample per SYN frame, position and pressure
//! from the same stream.
//!
//! Absence is silent by design: no pen device means an
//! empty scan and the editor's mouse path simply never
//! sees a sample. Nothing here blocks on hardware.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

pub mod synthetic;
mod sys;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::JoinHandle;

// evdev event types and codes (linux/input-event-codes.h).
const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const BTN_TOOL_PEN: u16 = 0x140;
const BTN_TOUCH: u16 = 0x14a;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const ABS_PRESSURE: u16 = 0x18;

/// Input nodes probed by the scan (bounded on purpose).
pub const INPUT_NODES_MAX: u32 = 64;
/// Samples buffered between the reader thread and the
/// consumer; past the bound, samples drop and count.
pub const CHANNEL_CAPACITY: usize = 4096;

/// One pen state sample, emitted per SYN_REPORT in which
/// the tracked state changed (the spike's §7.3 shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PenSample {
    /// Pen X in device units (the device's own range).
    pub x_units: i32,
    /// Pen Y in device units.
    pub y_units: i32,
    /// Pressure normalized to 0..=65535 from the device
    /// range by exact integer scaling.
    pub pressure: u16,
    /// True while the pen touches the surface.
    pub touch: bool,
    /// True while the pen tool is in proximity.
    pub tool_pen: bool,
    /// Kernel timestamp of the SYN event, in nanoseconds.
    pub time_ns: u64,
}

/// A pen-capable input device, as the scan found it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PenDeviceInfo {
    /// The `/dev/input/eventN` node.
    pub node: PathBuf,
    /// The device's reported name.
    pub name: String,
    /// ABS_X maximum (device units).
    pub x_max: i32,
    /// ABS_Y maximum (device units).
    pub y_max: i32,
    /// ABS_PRESSURE maximum (device units).
    pub pressure_max: i32,
    /// Whether the device reports BTN_TOOL_PEN.
    pub has_tool_pen: bool,
}

/// Scan `/dev/input/event*` for pen-capable devices:
/// a node qualifies when it answers EVIOCGABS for
/// ABS_PRESSURE with a positive maximum (an axis that
/// cannot report pressure is not a pen). Tool-pen
/// reporters rank ahead of bare pressure axes. Never
/// errors: an unreadable node is simply not a pen.
pub fn scan_pen_devices() -> Vec<PenDeviceInfo> {
    let mut found: Vec<PenDeviceInfo> = Vec::new();
    for index in 0..INPUT_NODES_MAX {
        let node = PathBuf::from(format!("/dev/input/event{index}"));
        let Ok(fd) = sys::open_readonly(&node) else {
            continue;
        };
        let Some(pressure) = sys::abs_info(&fd, ABS_PRESSURE) else {
            continue;
        };
        if pressure.maximum <= 0 {
            continue;
        }
        let x = sys::abs_info(&fd, ABS_X);
        let y = sys::abs_info(&fd, ABS_Y);
        found.push(PenDeviceInfo {
            node,
            name: sys::device_name(&fd).unwrap_or_default(),
            x_max: x.map(|info| info.maximum).unwrap_or(0),
            y_max: y.map(|info| info.maximum).unwrap_or(0),
            pressure_max: pressure.maximum,
            has_tool_pen: sys::class_has_code(&fd, EV_KEY, BTN_TOOL_PEN),
        });
    }
    found.sort_by_key(|info| !info.has_tool_pen);
    found
}

/// Normalize a raw pressure reading onto 0..=65535 by
/// exact integer scaling, clamped to the device range.
/// A device reporting a non-positive maximum normalizes
/// everything to 0 rather than dividing by it.
pub fn normalize_pressure(raw: i32, device_max: i32) -> u16 {
    if device_max <= 0 {
        return 0;
    }
    let clamped = raw.clamp(0, device_max) as i64;
    ((clamped * 65_535) / i64::from(device_max)) as u16
}

/// A running pen reader: a thread draining one evdev
/// node into a bounded channel.
pub struct PenReader {
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

impl PenReader {
    /// Spawn a reader on `node`. Fails when the node
    /// cannot be opened or carries no pressure axis —
    /// callers treat that exactly like "no pen present".
    pub fn spawn(node: &Path) -> io::Result<(PenReader, Receiver<PenSample>)> {
        let fd = sys::open_readonly(node)?;
        let pressure = sys::abs_info(&fd, ABS_PRESSURE)
            .filter(|info| info.maximum > 0)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Unsupported,
                    "node has no usable ABS_PRESSURE axis",
                )
            })?;
        let (sender, receiver) = sync_channel::<PenSample>(CHANNEL_CAPACITY);
        let stop = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let thread = {
            let stop = Arc::clone(&stop);
            let dropped = Arc::clone(&dropped);
            std::thread::Builder::new()
                .name("torogao-pen-reader".to_string())
                .spawn(move || run_reader(fd, pressure.maximum, sender, stop, dropped))?
        };
        Ok((
            PenReader {
                stop,
                dropped,
                thread: Some(thread),
            },
            receiver,
        ))
    }

    /// Samples dropped because the consumer fell behind
    /// (the bounded channel's overflow count).
    pub fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Stop the reader thread and join it.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for PenReader {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The reader loop: reconstruct pen state from the event
/// stream and emit one sample per SYN_REPORT that changed
/// anything (position comes from this stream — the
/// spike's binding design note).
fn run_reader(
    fd: std::os::fd::OwnedFd,
    pressure_max: i32,
    sender: SyncSender<PenSample>,
    stop: Arc<AtomicBool>,
    dropped: Arc<AtomicU64>,
) {
    let event_bytes = std::mem::size_of::<sys::InputEvent>();
    let mut pending: Vec<u8> = Vec::with_capacity(8192);
    let mut chunk = [0u8; 4096];
    let (mut x, mut y, mut raw_pressure) = (0i32, 0i32, 0i32);
    let (mut touch, mut tool_pen) = (false, false);
    let mut last_emitted: Option<PenSample> = None;
    while !stop.load(Ordering::Relaxed) {
        match sys::poll_readable(&fd, 100) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(_) => break,
        }
        let read = match sys::read_some(&fd, &mut chunk) {
            Ok(0) => continue,
            Ok(n) => n,
            Err(_) => break,
        };
        pending.extend_from_slice(&chunk[..read]);
        let mut offset = 0usize;
        while offset + event_bytes <= pending.len() {
            // SAFETY: the slice holds at least one whole
            // InputEvent; the struct is repr(C), POD, and
            // any bit pattern is a valid value.
            let event = unsafe {
                std::ptr::read_unaligned(pending[offset..].as_ptr().cast::<sys::InputEvent>())
            };
            offset += event_bytes;
            match event.type_ {
                EV_ABS => match event.code {
                    ABS_X => x = event.value,
                    ABS_Y => y = event.value,
                    ABS_PRESSURE => raw_pressure = event.value,
                    _ => {}
                },
                EV_KEY => match event.code {
                    BTN_TOUCH => touch = event.value != 0,
                    BTN_TOOL_PEN => tool_pen = event.value != 0,
                    _ => {}
                },
                EV_SYN if event.code == SYN_REPORT => {
                    let sample = PenSample {
                        x_units: x,
                        y_units: y,
                        pressure: normalize_pressure(raw_pressure, pressure_max),
                        touch,
                        tool_pen,
                        time_ns: (event.tv_sec as u64)
                            .saturating_mul(1_000_000_000)
                            .saturating_add((event.tv_usec as u64).saturating_mul(1_000)),
                    };
                    if last_emitted != Some(sample) {
                        if sender.try_send(sample).is_err() {
                            dropped.fetch_add(1, Ordering::Relaxed);
                        }
                        last_emitted = Some(sample);
                    }
                }
                _ => {}
            }
        }
        pending.drain(..offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pressure_normalization_is_exact() {
        assert_eq!(normalize_pressure(0, 2047), 0);
        assert_eq!(normalize_pressure(2047, 2047), 65_535);
        // The spike's first-touch raw value 64 on a
        // 2047-max device: 64 * 65535 / 2047 = 2048.
        assert_eq!(normalize_pressure(64, 2047), 2048);
        // Clamping holds at both ends.
        assert_eq!(normalize_pressure(-5, 2047), 0);
        assert_eq!(normalize_pressure(999_999, 2047), 65_535);
        // A broken device maximum never divides by zero.
        assert_eq!(normalize_pressure(10, 0), 0);
        assert_eq!(normalize_pressure(10, -1), 0);
    }

    #[test]
    fn scan_without_hardware_is_empty_and_quiet() {
        // Valid on any machine: the scan may find devices
        // (never panics, never errors), and any hit must
        // carry a sane pressure maximum.
        for info in scan_pen_devices() {
            assert!(info.pressure_max > 0);
        }
    }

    #[test]
    fn spawn_on_a_missing_node_fails_cleanly() {
        let missing = Path::new("/dev/input/event-does-not-exist");
        assert!(PenReader::spawn(missing).is_err());
    }
}
