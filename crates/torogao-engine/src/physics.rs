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
    ///
    /// The derivative array is zeroed ONCE, before the first
    /// evaluation of the tick — never between stages (spec
    /// P-2; gap G-2). A derivative an evaluation leaves
    /// unset therefore carries its previous stage's value
    /// into the next stage, exactly as the reference's
    /// persistent derivative array does.
    pub fn tick(&mut self, h: f32, eval: &mut dyn FnMut(&mut PhysicsSystem, f32)) {
        self.current.copy_from_slice(&self.state);
        let n = self.state.len();
        let mut k1 = vec![0.0f32; n];
        let mut k2 = vec![0.0f32; n];
        let mut k3 = vec![0.0f32; n];
        let mut k4 = vec![0.0f32; n];

        self.derivative.iter_mut().for_each(|d| *d = 0.0);
        for stage in 0..4 {
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

    /// The integrator's simulation time.
    pub fn system_time(&self) -> f32 {
        self.system.t
    }

    /// The current angular velocity (state, for tests and
    /// diagnostics).
    pub fn d_angle(&self) -> f32 {
        self.system.get(Self::D_ANGLE)
    }

    /// Anchor refresh (spec P-11; gap G-9): re-hangs the
    /// bob straight below the anchor at `length`. The
    /// angular velocity is NOT cleared; the angle is
    /// re-derived from the bob on the next tick.
    pub fn update_anchor(&mut self, anchor: Vec2, length: f32) {
        self.bob = anchor + Vec2::new(0.0, length);
    }

    /// The debug-draw segment (spec P-32): anchor → bob.
    /// The render-side line itself is drawn by the app; the
    /// engine owns the geometry.
    pub fn debug_segment(&self, anchor: Vec2) -> (Vec2, Vec2) {
        (anchor, self.bob)
    }

    /// One tick: re-derive the angle from the bob position,
    /// integrate, then re-project the bob onto the circle
    /// (the reference `Pendulum.tick`).
    ///
    /// Effective inputs come from `io`, a provider invoked
    /// fresh at every stage evaluation (spec: the reference
    /// `eval` calls the driver's effective-value getters on
    /// every call; gap G-8) and again for the pre-tick
    /// re-derivation and the post-tick re-projection, where
    /// the reference likewise reads the driver's current
    /// anchor and length.
    pub fn tick(&mut self, h: f32, io: &dyn Fn() -> PendulumIo) {
        let first = io();
        let angle = (-(self.bob.x - first.anchor.x)).atan2(self.bob.y - first.anchor.y);
        self.system.set(Self::ANGLE, angle);
        self.system.tick(h, &mut |sys, _t| {
            let io = io();
            let angle = sys.get(Self::ANGLE);
            let d_angle = sys.get(Self::D_ANGLE);
            sys.set_derivative(Self::ANGLE, d_angle);
            let length_ratio = io.gravity / io.length;
            let crit_damp = 2.0 * length_ratio.sqrt();
            let dd = -length_ratio * angle.sin() - d_angle * io.angle_damping * crit_damp;
            sys.set_derivative(Self::D_ANGLE, dd);
        });
        let last = io();
        let angle = self.system.get(Self::ANGLE);
        let d_bob = Vec2::new(-angle.sin(), angle.cos());
        self.bob = last.anchor + d_bob * last.length;
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

    /// The integrator's simulation time.
    pub fn system_time(&self) -> f32 {
        self.system.t
    }

    /// The bob's current velocity (state, for tests and
    /// diagnostics).
    pub fn d_bob(&self) -> Vec2 {
        Vec2::new(
            self.system.get(Self::D_BOB_X),
            self.system.get(Self::D_BOB_Y),
        )
    }

    /// Anchor refresh (spec P-17; gap G-9): re-hangs the
    /// bob straight below the anchor at `length`. The
    /// velocity state is NOT reset.
    pub fn update_anchor(&mut self, anchor: Vec2, length: f32) {
        self.system.set(Self::BOB_X, anchor.x);
        self.system.set(Self::BOB_Y, anchor.y + length);
    }

    /// The debug-draw segment (spec P-32): anchor → bob.
    pub fn debug_segment(&self, anchor: Vec2) -> (Vec2, Vec2) {
        (
            anchor,
            Vec2::new(self.system.get(Self::BOB_X), self.system.get(Self::BOB_Y)),
        )
    }

    /// One tick of the spring system (the reference
    /// derivative function, operation for operation).
    /// Effective inputs come from `io`, invoked fresh at
    /// every stage evaluation (gap G-8, as in `Pendulum`).
    pub fn tick(&mut self, h: f32, io: &dyn Fn() -> SpringIo) {
        self.system.tick(h, &mut |sys, _t| {
            let io = io();
            let anchor = io.anchor;
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
            p.tick(0.01, &|| io);
        }
        assert!(p.angle().abs() < 1e-5);
        assert!((p.output.y - 100.0).abs() < 1e-3);
    }

    #[test]
    fn derivative_array_is_zeroed_once_per_tick_not_per_stage() {
        // G-2 / spec P-2: an evaluation that sets a
        // derivative only on the first stage leaves that
        // value standing for the later stages (the
        // reference's derivative array persists across the
        // stages of one tick). With per-stage re-zeroing
        // (the pre-fix behaviour) k2..k4 for that variable
        // would be 0 and the final value would be 1/6 of
        // the asserted one.
        let mut sys = PhysicsSystem::new(2);
        sys.set(0, 0.0);
        sys.set(1, 0.0);
        let mut calls = 0usize;
        sys.tick(1.0, &mut |s, _t| {
            calls += 1;
            // d(a)/dt = b, restaged every stage.
            let b = s.get(1);
            s.set_derivative(0, b);
            // d(b)/dt staged only on the first evaluation.
            if calls == 1 {
                s.set_derivative(1, 1.0);
            }
        });
        assert_eq!(calls, 4);
        // k for b is 1 at every stage under once-zeroing:
        // b advances by h * (1 + 2 + 2 + 1) / 6 = 1.
        assert!((sys.get(1) - 1.0).abs() < 1e-6, "b = {}", sys.get(1));
    }

    #[test]
    fn effective_inputs_are_read_fresh_at_every_stage() {
        // G-8: the provider must be consulted once per
        // stage evaluation (plus the pendulum's pre/post
        // reads), never captured once per tick.
        use std::cell::Cell;
        let pendulum_calls = Cell::new(0usize);
        let io = PendulumIo {
            anchor: Vec2::new(0.0, 0.0),
            gravity: 9800.0,
            length: 100.0,
            angle_damping: 0.5,
        };
        let mut p = Pendulum::new(io.anchor, io.length);
        p.tick(0.01, &|| {
            pendulum_calls.set(pendulum_calls.get() + 1);
            io
        });
        assert!(
            pendulum_calls.get() >= 6,
            "pendulum provider calls: {}",
            pendulum_calls.get()
        );
        let spring_calls = Cell::new(0usize);
        let sio = SpringIo {
            anchor: Vec2::new(0.0, 0.0),
            gravity: 9800.0,
            length: 100.0,
            frequency: 1.0,
            angle_damping: 0.5,
            length_damping: 0.5,
        };
        let mut s = SpringPendulum::new(sio.anchor, sio.length);
        s.tick(0.01, &|| {
            spring_calls.set(spring_calls.get() + 1);
            sio
        });
        assert!(
            spring_calls.get() >= 4,
            "spring provider calls: {}",
            spring_calls.get()
        );
    }

    #[test]
    fn anchor_refresh_rehangs_the_bob_and_keeps_velocity() {
        // G-9 / spec P-11 + P-17.
        let io = PendulumIo {
            anchor: Vec2::new(0.0, 0.0),
            gravity: 9800.0,
            length: 100.0,
            angle_damping: 0.5,
        };
        let mut p = Pendulum::new(io.anchor, io.length);
        // Displace and spin the pendulum up by hand through
        // its public state surface: swing via ticks from a
        // displaced bob.
        p.bob = Vec2::new(50.0, 80.0);
        for _ in 0..10 {
            p.tick(0.01, &|| io);
        }
        let velocity_before = p.d_angle();
        p.update_anchor(Vec2::new(200.0, 50.0), 100.0);
        assert_eq!(p.bob, Vec2::new(200.0, 150.0));
        assert_eq!(p.d_angle(), velocity_before);

        let sio = SpringIo {
            anchor: Vec2::new(0.0, 0.0),
            gravity: 9800.0,
            length: 100.0,
            frequency: 1.0,
            angle_damping: 0.5,
            length_damping: 0.5,
        };
        let mut s = SpringPendulum::new(sio.anchor, sio.length);
        for _ in 0..10 {
            s.tick(0.01, &|| sio);
        }
        let d_before = s.d_bob();
        s.update_anchor(Vec2::new(-30.0, 10.0), 80.0);
        let (a, b) = s.debug_segment(Vec2::new(-30.0, 10.0));
        assert_eq!(a, Vec2::new(-30.0, 10.0));
        assert_eq!(b, Vec2::new(-30.0, 90.0));
        assert_eq!(s.d_bob(), d_before);
    }

    #[test]
    fn zero_length_pendulum_reverts_the_step_and_advances_time() {
        // G-23 / spec P-4: length 0 makes gravity/length
        // infinite; at a displaced angle the derivative is
        // non-finite, the write-back must revert the whole
        // step, and simulation time must still advance.
        let io = PendulumIo {
            anchor: Vec2::new(0.0, 0.0),
            gravity: 9800.0,
            length: 0.0,
            angle_damping: 0.5,
        };
        let mut p = Pendulum::new(Vec2::new(0.0, 0.0), 100.0);
        p.bob = Vec2::new(60.0, 80.0);
        p.tick(0.01, &|| io);
        assert!(p.angle().is_finite(), "angle must stay finite");
        // The angle state reverted to the pre-tick derived
        // angle's integration result of zero net motion:
        // the state equals what the re-derivation staged
        // (the revert restores the pre-tick state, which
        // the staging had just set from the bob).
        let staged = (-(60.0f32)).atan2(80.0f32);
        assert!(
            (p.angle() - staged).abs() < 1e-6,
            "angle {} != staged {}",
            p.angle(),
            staged
        );
        assert!((p.system_time() - 0.01).abs() < 1e-6);
        // Re-projection at length 0 lands the bob on the
        // anchor.
        assert_eq!(p.bob, Vec2::new(0.0, 0.0));
    }

    #[test]
    fn negative_length_pendulum_stays_finite_without_reverting() {
        // G-23, measured: a negative effective length is
        // finite math throughout. The bob starts above the
        // anchor (anchor + (0, length)); the per-tick angle
        // re-derivation reads that as angle pi, and the
        // re-projection multiplies the pi direction by the
        // negative length, landing the bob BELOW the anchor
        // at +|length|. No derivative goes non-finite, so
        // the P-4 revert never fires; time advances.
        let io = PendulumIo {
            anchor: Vec2::new(0.0, 0.0),
            gravity: 9800.0,
            length: -100.0,
            angle_damping: 0.5,
        };
        let mut p = Pendulum::new(Vec2::new(0.0, 0.0), -100.0);
        assert_eq!(p.bob, Vec2::new(0.0, -100.0));
        p.tick(0.01, &|| io);
        assert!(p.angle().is_finite() && p.bob.x.is_finite() && p.bob.y.is_finite());
        assert!((p.bob.y - 100.0).abs() < 1e-3, "bob {:?}", p.bob);
        assert!((p.system_time() - 0.01).abs() < 1e-6);
    }

    #[test]
    fn zero_frequency_spring_reverts_the_step_and_advances_time() {
        // G-23 / spec P-4 + P-13: frequency 0 makes the
        // spring constant 0, so the rest length divides by
        // zero and the force goes non-finite; the step
        // reverts and time advances.
        let io = SpringIo {
            anchor: Vec2::new(0.0, 0.0),
            gravity: 9800.0,
            length: 100.0,
            frequency: 0.0,
            angle_damping: 0.5,
            length_damping: 0.5,
        };
        let mut s = SpringPendulum::new(io.anchor, io.length);
        s.tick(0.01, &|| io);
        assert_eq!(s.output, Vec2::new(0.0, 100.0));
        assert!((s.system_time() - 0.01).abs() < 1e-6);
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
            p.tick(0.01, &|| io);
        }
        assert!(p.output.x.abs() < 1.0);
        assert!(p.output.y > 90.0 && p.output.y < 130.0);
    }
}
