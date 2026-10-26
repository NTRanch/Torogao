//! The fixed-step physics integrator: an RK4 system over a
//! flat f32 state, with the pendulum and spring-pendulum
//! drivers' derivative functions ported operation for
//! operation from the reference. Evaluation order here is
//! behavioral: the preview series must agree with the
//! reference bit for bit where the arithmetic allows.

use crate::math::Vec2;

/// One RK4 integration system, mirroring the reference
/// `PhysicsSystem`: named variables point into the flat
/// state vector, derivatives are staged into a scratch
/// vector, and a non-finite derivative reverts the step.
#[derive(Debug, Clone)]
pub struct PhysicsSystem {
    state: Vec<f32>,
    derivative: Vec<f32>,
    current: Vec<f32>,
    /// Simulation time.
    pub t: f32,
}

impl PhysicsSystem {
    /// A system over `variable_count` f32 state variables.
    pub fn new(variable_count: usize) -> PhysicsSystem {
        PhysicsSystem {
            state: vec![0.0; variable_count],
            derivative: vec![0.0; variable_count],
            current: vec![0.0; variable_count],
            t: 0.0,
        }
    }

    /// Reads state variable `index`.
    pub fn get(&self, index: usize) -> f32 {
        self.state[index]
    }

    /// Writes state variable `index`.
    pub fn set(&mut self, index: usize, value: f32) {
        self.state[index] = value;
    }

    /// Stages a derivative value.
    pub fn set_derivative(&mut self, index: usize, value: f32) {
        self.derivative[index] = value;
    }

    /// Advances the system by `h` with RK4, calling `eval`
    /// to restage derivatives at each sub-step (the reference
    /// structure: cur + h*k/2, cur + h*k/2, cur + h*k, then
    /// the weighted sum; non-finite derivative at any stage
    /// reverts the whole step).
    pub fn tick(&mut self, h: f32, eval: &mut dyn FnMut(&mut PhysicsSystem, f32)) {
        self.current.copy_from_slice(&self.state);
        let n = self.state.len();
        let mut k1 = vec![0.0f32; n];
        let mut k2 = vec![0.0f32; n];
        let mut k3 = vec![0.0f32; n];
        let mut k4 = vec![0.0f32; n];

        for stage in 0..4 {
            self.derivative.iter_mut().for_each(|d| *d = 0.0);
            let t = match stage {
                0 => self.t,
                1 | 2 => self.t + h / 2.0,
                _ => self.t + h,
            };
            eval(self, t);
            let k = match stage {
                0 => &mut k1,
                1 => &mut k2,
                2 => &mut k3,
                _ => &mut k4,
            };
            k.copy_from_slice(&self.derivative);
            match stage {
                0 => {
                    for i in 0..n {
                        self.state[i] = self.current[i] + h * k1[i] / 2.0;
                    }
                }
                1 => {
                    for i in 0..n {
                        self.state[i] = self.current[i] + h * k2[i] / 2.0;
                    }
                }
                2 => {
                    for i in 0..n {
                        self.state[i] = self.current[i] + h * k3[i];
                    }
                }
                _ => {}
            }
        }

        for i in 0..n {
            self.state[i] =
                self.current[i] + (h * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i])) / 6.0;
            if !self.state[i].is_finite() {
                self.state.copy_from_slice(&self.current);
                break;
            }
        }
        self.t += h;
    }
}

/// The pendulum driver's tuning + I/O, separated from the
/// node that owns it so the derivative math reads plainly.
#[derive(Debug, Clone, Copy)]
pub struct PendulumIo {
    /// Anchor point (world, or local when local-only).
    pub anchor: Vec2,
    /// Effective gravity (already scaled).
    pub gravity: f32,
    /// Effective length.
    pub length: f32,
    /// Effective angle damping.
    pub angle_damping: f32,
}

/// A simple pendulum: state (angle, dAngle), with the bob
/// point re-projected onto the anchor circle each tick.
#[derive(Debug, Clone)]
pub struct Pendulum {
    system: PhysicsSystem,
    /// The bob (pendulum tip) point.
    pub bob: Vec2,
    /// The output point (the bob after projection).
    pub output: Vec2,
}

impl Pendulum {
    /// State indices.
    pub const ANGLE: usize = 0;
    /// State indices.
    pub const D_ANGLE: usize = 1;

    /// Builds a pendulum hanging straight down from the
    /// anchor at the given length.
    pub fn new(anchor: Vec2, length: f32) -> Pendulum {
        Pendulum {
            system: PhysicsSystem::new(2),
            bob: anchor + Vec2::new(0.0, length),
            output: Vec2::ZERO,
        }
    }

    /// The current angle.
    pub fn angle(&self) -> f32 {
        self.system.get(Self::ANGLE)
    }

    /// One tick: re-derive the angle from the bob position,
    /// integrate, then re-project the bob onto the circle
    /// (the reference `Pendulum.tick`).
    pub fn tick(&mut self, h: f32, io: &PendulumIo) {
        let angle = (-(self.bob.x - io.anchor.x)).atan2(self.bob.y - io.anchor.y);
        self.system.set(Self::ANGLE, angle);
        let gravity = io.gravity;
        let length = io.length;
        let damping = io.angle_damping;
        self.system.tick(h, &mut |sys, _t| {
            let angle = sys.get(Self::ANGLE);
            let d_angle = sys.get(Self::D_ANGLE);
            sys.set_derivative(Self::ANGLE, d_angle);
            let length_ratio = gravity / length;
            let crit_damp = 2.0 * length_ratio.sqrt();
            let dd = -length_ratio * angle.sin() - d_angle * damping * crit_damp;
            sys.set_derivative(Self::D_ANGLE, dd);
        });
        let angle = self.system.get(Self::ANGLE);
        let d_bob = Vec2::new(-angle.sin(), angle.cos());
        self.bob = io.anchor + d_bob * io.length;
        self.output = self.bob;
    }
}

/// The spring-pendulum driver's tuning + I/O.
#[derive(Debug, Clone, Copy)]
pub struct SpringIo {
    /// Anchor point.
    pub anchor: Vec2,
    /// Effective gravity (already scaled).
    pub gravity: f32,
    /// Effective length.
    pub length: f32,
    /// Effective frequency.
    pub frequency: f32,
    /// Effective angle damping.
    pub angle_damping: f32,
    /// Effective length damping.
    pub length_damping: f32,
}

/// A spring pendulum: state (bob.x, bob.y, dBob.x, dBob.y).
#[derive(Debug, Clone)]
pub struct SpringPendulum {
    system: PhysicsSystem,
    /// The output point (the bob).
    pub output: Vec2,
}

impl SpringPendulum {
    const BOB_X: usize = 0;
    const BOB_Y: usize = 1;
    const D_BOB_X: usize = 2;
    const D_BOB_Y: usize = 3;

    /// Builds a spring pendulum with the bob hanging below
    /// the anchor at the given length.
    pub fn new(anchor: Vec2, length: f32) -> SpringPendulum {
        let mut system = PhysicsSystem::new(4);
        system.set(Self::BOB_X, anchor.x);
        system.set(Self::BOB_Y, anchor.y + length);
        SpringPendulum {
            system,
            output: Vec2::ZERO,
        }
    }

    /// One tick of the spring system (the reference
    /// derivative function, operation for operation).
    pub fn tick(&mut self, h: f32, io: &SpringIo) {
        let anchor = io.anchor;
        let io = *io;
        self.system.tick(h, &mut |sys, _t| {
            let bob = Vec2::new(sys.get(Self::BOB_X), sys.get(Self::BOB_Y));
            let d_bob = Vec2::new(sys.get(Self::D_BOB_X), sys.get(Self::D_BOB_Y));
            sys.set_derivative(Self::BOB_X, d_bob.x);
            sys.set_derivative(Self::BOB_Y, d_bob.y);

            let spring_k_sqrt = io.frequency * 2.0 * std::f32::consts::PI;
            let spring_k = spring_k_sqrt * spring_k_sqrt;
            let g = io.gravity;
            let rest_length = io.length - g / spring_k;
            let off_pos = bob - anchor;
            let off_pos_norm = off_pos.normalized();
            let length_ratio = io.gravity / io.length;
            let crit_damp_angle = 2.0 * length_ratio.sqrt();
            let crit_damp_length = 2.0 * spring_k_sqrt;
            let dist = anchor.distance(bob);
            let force = Vec2::new(0.0, g) - off_pos_norm * (dist - rest_length) * spring_k;
            let mut dd_bob = force;
            let d_bob_rot = Vec2::new(
                d_bob.x * off_pos_norm.y + d_bob.y * off_pos_norm.x,
                d_bob.y * off_pos_norm.y - d_bob.x * off_pos_norm.x,
            );
            let dd_bob_rot = Vec2::new(
                -(d_bob_rot.x * io.angle_damping * crit_damp_angle),
                -(d_bob_rot.y * io.length_damping * crit_damp_length),
            );
            let dd_bob_damping = Vec2::new(
                dd_bob_rot.x * off_pos_norm.y - d_bob_rot.y * off_pos_norm.x,
                dd_bob_rot.y * off_pos_norm.y + d_bob_rot.x * off_pos_norm.x,
            );
            dd_bob += dd_bob_damping;
            sys.set_derivative(Self::D_BOB_X, dd_bob.x);
            sys.set_derivative(Self::D_BOB_Y, dd_bob.y);
        });
        self.output = Vec2::new(self.system.get(Self::BOB_X), self.system.get(Self::BOB_Y));
    }
}

#[cfg(test)]
mod tests {
    use super::{Pendulum, PendulumIo, PhysicsSystem, SpringIo, SpringPendulum};
    use crate::math::Vec2;

    #[test]
    fn rk4_integrates_a_constant_derivative() {
        let mut sys = PhysicsSystem::new(1);
        sys.set(0, 1.0);
        sys.tick(0.5, &mut |s, _t| s.set_derivative(0, 2.0));
        assert!((sys.get(0) - 2.0).abs() < 1e-6);
    }

    #[test]
    fn pendulum_stays_put_at_rest() {
        let io = PendulumIo {
            anchor: Vec2::new(0.0, 0.0),
            gravity: 9800.0,
            length: 100.0,
            angle_damping: 0.5,
        };
        let mut p = Pendulum::new(io.anchor, io.length);
        for _ in 0..60 {
            p.tick(0.01, &io);
        }
        assert!(p.angle().abs() < 1e-5);
        assert!((p.output.y - 100.0).abs() < 1e-3);
    }

    #[test]
    fn spring_pendulum_settles() {
        let io = SpringIo {
            anchor: Vec2::new(0.0, 0.0),
            gravity: 9800.0,
            length: 100.0,
            frequency: 1.0,
            angle_damping: 0.5,
            length_damping: 0.5,
        };
        let mut p = SpringPendulum::new(io.anchor, io.length);
        for _ in 0..600 {
            p.tick(0.01, &io);
        }
        assert!(p.output.x.abs() < 1.0);
        assert!(p.output.y > 90.0 && p.output.y < 130.0);
    }
}
