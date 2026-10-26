//! Ambient drivers (drivers.d port): breath, blink,
//! saccade, and sway as deterministic functions of the
//! step index, the step rate, and a recorded seed.

use serde_json::Value;
use torogao_engine::Puppet;

/// A parsed driver set.
#[derive(Debug, Clone, Copy)]
pub struct DriverConfig {
    /// Breath driver enabled.
    pub breath: bool,
    /// Blink driver enabled.
    pub blink: bool,
    /// Saccade driver enabled.
    pub saccade: bool,
    /// Sway driver enabled.
    pub sway: bool,
    /// The schedule seed.
    pub seed: u64,
}

impl Default for DriverConfig {
    fn default() -> DriverConfig {
        DriverConfig {
            breath: true,
            blink: true,
            saccade: true,
            sway: true,
            seed: 1,
        }
    }
}

/// Parses a driver-set document; on failure returns the
/// reference's error string.
pub fn parse_driver_config(doc: &Value) -> Result<DriverConfig, String> {
    let mut cfg = DriverConfig::default();
    if !doc.is_object() {
        return Err("driver set must be a JSON object".to_string());
    }
    if let Some(seed) = doc.get("seed") {
        if let Some(value) = seed.as_i64() {
            cfg.seed = value as u64;
        } else if let Some(value) = seed.as_u64() {
            cfg.seed = value;
        } else {
            return Err("driver set 'seed' must be an integer".to_string());
        }
    }
    if let Some(list) = doc.get("drivers") {
        let Some(names) = list.as_array() else {
            return Err("driver set 'drivers' must be an array of names".to_string());
        };
        cfg.breath = false;
        cfg.blink = false;
        cfg.saccade = false;
        cfg.sway = false;
        for entry in names {
            let Some(name) = entry.as_str() else {
                return Err("driver names must be strings".to_string());
            };
            match name {
                "breath" => cfg.breath = true,
                "blink" => cfg.blink = true,
                "saccade" => cfg.saccade = true,
                "sway" => cfg.sway = true,
                other => {
                    return Err(format!(
                        "unknown ambient driver '{other}' (known: breath, blink, saccade, sway)"
                    ));
                }
            }
        }
    }
    Ok(cfg)
}

/// One splitmix64 step (the reference's seeded schedule
/// generator).
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The driver names active in a configuration, in the
/// reference's fixed order (for reports).
pub fn active_driver_names(cfg: &DriverConfig) -> Vec<&'static str> {
    let mut names = Vec::new();
    if cfg.breath {
        names.push("breath");
    }
    if cfg.blink {
        names.push("blink");
    }
    if cfg.saccade {
        names.push("saccade");
    }
    if cfg.sway {
        names.push("sway");
    }
    names
}

/// Computes driver input values for `steps` steps at
/// `fps`: (parameter name, per-step values) pairs covering
/// only parameters the puppet actually has.
pub fn driver_inputs(
    puppet: &Puppet,
    cfg: &DriverConfig,
    steps: usize,
    fps: i32,
) -> Vec<(String, Vec<f64>)> {
    let mut out: Vec<(String, Vec<f64>)> = Vec::new();
    let range_of = |name: &str| -> Option<(f64, f64, f64)> {
        puppet.params.iter().find(|p| p.name == name).map(|p| {
            (
                f64::from(p.min.x),
                f64::from(p.max.x),
                f64::from(p.defaults.x),
            )
        })
    };

    if cfg.breath {
        if let Some((lo_r, hi_r, _)) = range_of("ParamBreath") {
            let lo = lo_r + 0.15 * (hi_r - lo_r);
            let hi = lo_r + 0.85 * (hi_r - lo_r);
            let values: Vec<f64> = (0..steps)
                .map(|i| {
                    let phase = (i as f64 / f64::from(fps)) / 4.0;
                    let s = 0.5 + 0.5 * (2.0 * std::f64::consts::PI * phase).sin();
                    lo + s * (hi - lo)
                })
                .collect();
            out.push(("ParamBreath".to_string(), values));
        }
    }

    if cfg.blink {
        let period = 4 * fps;
        let closed = 3;
        for eye in ["ParamEyeLOpen", "ParamEyeROpen"] {
            if let Some((shut, open, _)) = range_of(eye) {
                let values: Vec<f64> = (0..steps)
                    .map(|i| {
                        if (i as i32 % period) < closed {
                            shut
                        } else {
                            open
                        }
                    })
                    .collect();
                out.push((eye.to_string(), values));
            }
        }
    }

    if cfg.saccade {
        for axis in ["ParamEyeBallX", "ParamEyeBallY"] {
            if let Some((lo, hi, default)) = range_of(axis) {
                let mut state = cfg.seed
                    ^ if axis == "ParamEyeBallX" {
                        0xA24B_AED4_963E_E407
                    } else {
                        0x9FB2_1C65_1E98_DF25
                    };
                let mut target = default;
                let mut next_jump: i32 = 0;
                let mut values = Vec::with_capacity(steps);
                for i in 0..steps {
                    if i as i32 >= next_jump {
                        let u = splitmix64(&mut state);
                        let frac = (u >> 11) as f64 / (1u64 << 53) as f64;
                        target = lo + frac * (hi - lo);
                        let w = splitmix64(&mut state);
                        let wait_seconds = 1 + (w % 3) as i32;
                        next_jump = i as i32 + wait_seconds * fps;
                    }
                    values.push(target);
                }
                out.push((axis.to_string(), values));
            }
        }
    }

    if cfg.sway {
        if let Some((lo, hi, center)) = range_of("ParamAngleZ") {
            let amp = 0.08 * (hi - lo);
            let values: Vec<f64> = (0..steps)
                .map(|i| {
                    let phase = (i as f64 / f64::from(fps)) / 7.0;
                    center + amp * (2.0 * std::f64::consts::PI * phase).sin()
                })
                .collect();
            out.push(("ParamAngleZ".to_string(), values));
        }
    }

    out
}
