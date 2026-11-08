//! Gate tests for torogao-coin. Half validation (the measured
//! table and the measured rules behave as the RE record
//! states), half adversarial (corrupt tables, hostile inputs,
//! and the boundaries the source leaves UNKNOWN are refused,
//! never improvised).

use torogao_coin::dynamics::StatTriple;
use torogao_coin::swing::accumulate;
use torogao_coin::{CoinError, Dynamics, Session, measured_dynamics};

const EPS: f64 = 1e-9;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() <= EPS * (1.0 + a.abs() + b.abs())
}

// ---------- validation ----------

#[test]
fn measured_table_matches_the_re_record() {
    let d = measured_dynamics().expect("embedded table must load");
    // COIN-DYNAMIC.md §5: gauge maxima, levels 0..=4.
    let expected = [0.5, 4.8, 15.0, 30.0, 70.0];
    assert_eq!(d.measured_level_count(), expected.len());
    for (level, want) in expected.iter().enumerate() {
        assert!(
            approx(d.gauge_maximum(level).unwrap(), *want),
            "level {level}"
        );
    }
    assert!(approx(d.swing_gain(), 1.5));
    assert!(approx(d.timer_divisor(), 20.0));
    assert!(approx(d.timer_start(), 1.0));
    assert!(approx(d.setup_wait_seconds(), 2.0));
    assert!(approx(d.bonus_threshold(), 8.0));
    assert!(approx(d.bonus_multiplier(), 1.15));
    assert!(approx(d.prep_scale(), 0.35));
    assert!(approx(d.message_divisor(), 100.0));
    assert!(approx(d.message_scale(), 0.65));
    assert_eq!(d.prep_action_count(), 15);
    assert_eq!(d.direct_tuples().len(), 9);
    assert_eq!(d.leaving_minute(), 1080);
    let a0 = d.prep_action(0).unwrap();
    assert!(approx(a0.delta.hp, 0.15) && approx(a0.delta.mp, 0.0) && approx(a0.delta.sp, 0.0));
    assert_eq!(a0.minutes, 20);
    let a14 = d.prep_action(14).unwrap();
    assert!(approx(a14.delta.mp, 0.3) && a14.minutes == 20);
}

#[test]
fn swing_banks_squared_amplitude_on_crossing_with_bonus() {
    let d = measured_dynamics().unwrap();
    // One full stroke: out to +10, back through 0, to -10.
    // Banked at the crossing: 10^2 * 1.5 * 1.15 (bonus: 10 >= 8).
    let samples = [0.0, 5.0, 10.0, 5.0, 0.0, -5.0, -10.0, -5.0, 0.0];
    let points = accumulate(&samples, &d).unwrap();
    assert!(approx(points, 172.5), "points {points}");
}

#[test]
fn swing_below_bonus_threshold_gets_no_bonus() {
    let d = measured_dynamics().unwrap();
    let samples = [0.0, 4.0, 0.0, -4.0, 0.0];
    let points = accumulate(&samples, &d).unwrap();
    assert!(approx(points, 24.0), "points {points}"); // 4^2 * 1.5
}

#[test]
fn attempt_timer_expires_at_the_derived_twenty_seconds() {
    let d = measured_dynamics().unwrap();
    let mut s = Session::new(d, 9 * 60).unwrap();
    // dt chosen so dt / 20 is exact in binary (0.3125 / 20 =
    // 0.015625): the timer reaches exactly 0 at step 64, and
    // the derived window is exactly 64 * 0.3125 = 20 s.
    let samples = vec![0.0; 200];
    let out = s.run_attempt(&samples, 0.3125).unwrap();
    assert!(!out.success);
    assert!(
        approx(out.elapsed_seconds, 20.0),
        "elapsed {}",
        out.elapsed_seconds
    );
    assert!(approx(out.points, 0.0));
}

#[test]
fn gauge_multiplier_is_a_product_not_a_sum() {
    let d = measured_dynamics().unwrap();
    let mut s = Session::new(d, 9 * 60).unwrap();
    s.apply_prep_action(13).unwrap(); // (0.2, 0.02, 0.16) * 0.35
    let product = 1.07 * 1.007 * 1.056;
    let sum = 1.0 + 0.07 + 0.007 + 0.056;
    assert!(
        approx(s.gauge_multiplier(), product),
        "multiplier {}",
        s.gauge_multiplier()
    );
    assert!(!approx(s.gauge_multiplier(), sum));
}

#[test]
fn prep_action_scales_delta_and_advances_the_clock() {
    let d = measured_dynamics().unwrap();
    let mut s = Session::new(d, 9 * 60).unwrap();
    let spent = s.apply_prep_action(0).unwrap();
    assert_eq!(spent, 20);
    assert!(approx(s.prep().hp, 0.15 * 0.35));
    assert_eq!(s.clock_minute(), 9 * 60 + 20);
}

#[test]
fn successful_attempt_raises_the_level() {
    let d = measured_dynamics().unwrap();
    let mut s = Session::new(d, 9 * 60).unwrap();
    let mut samples = vec![0.0];
    for i in 0..120 {
        samples.push(if i % 2 == 0 { -100.0 } else { 100.0 });
    }
    let out = s.run_attempt(&samples, 1.0 / 60.0).unwrap();
    assert!(out.success);
    assert_eq!(out.level_after, 1);
    assert_eq!(s.level(), 1);
}

#[test]
fn save_unlocks_only_at_the_leaving_minute() {
    let d = measured_dynamics().unwrap();
    let mut s = Session::new(d, 15 * 60).unwrap();
    assert!(!s.can_save());
    for i in 0..15 {
        s.apply_prep_action(i).unwrap();
    }
    assert_eq!(s.clock_minute(), 15 * 60 + 320);
    assert!(s.can_save());
}

// ---------- adversarial ----------

#[test]
fn corrupt_tables_are_refused() {
    let good = torogao_coin::DYNAMICS_RON;
    let non_increasing = good.replace("15.0, 30.0", "30.0, 15.0");
    assert!(matches!(
        Dynamics::from_ron(&non_increasing),
        Err(CoinError::Invalid {
            field: "gauge_maxima",
            ..
        })
    ));
    let zero_divisor = good.replace("timer_divisor: 20.0", "timer_divisor: 0.0");
    assert!(matches!(
        Dynamics::from_ron(&zero_divisor),
        Err(CoinError::Invalid {
            field: "timer_divisor",
            ..
        })
    ));
    let negative_delta = good.replace("(0.15, 0.0, 0.0)", "(-0.15, 0.0, 0.0)");
    assert!(matches!(
        Dynamics::from_ron(&negative_delta),
        Err(CoinError::Invalid {
            field: "prep_actions",
            ..
        })
    ));
    assert!(matches!(
        Dynamics::from_ron("not ron at all"),
        Err(CoinError::Parse(_))
    ));
}

#[test]
fn unknown_prep_action_is_refused() {
    let d = measured_dynamics().unwrap();
    let mut s = Session::new(d, 9 * 60).unwrap();
    assert!(matches!(
        s.apply_prep_action(15),
        Err(CoinError::UnknownPrepAction { index: 15 })
    ));
}

#[test]
fn hostile_step_inputs_are_refused() {
    let d = measured_dynamics().unwrap();
    let mut s = Session::new(d.clone(), 9 * 60).unwrap();
    assert!(matches!(
        s.run_attempt(&[0.0], 0.0),
        Err(CoinError::InvalidStep { .. })
    ));
    assert!(matches!(
        s.run_attempt(&[0.0], -0.5),
        Err(CoinError::InvalidStep { .. })
    ));
    assert!(matches!(
        s.run_attempt(&[0.0], 2.0),
        Err(CoinError::InvalidStep { .. })
    ));
    assert!(matches!(
        s.run_attempt(&[f64::NAN], 0.05),
        Err(CoinError::InvalidStep { .. })
    ));
    let oversize = vec![0.0; torogao_coin::swing::SAMPLES_MAX + 1];
    assert!(matches!(
        s.run_attempt(&oversize, 0.05),
        Err(CoinError::InvalidStep { .. })
    ));
    assert!(matches!(
        s.apply_message_stats(StatTriple {
            hp: f64::INFINITY,
            mp: 0.0,
            sp: 0.0
        }),
        Err(CoinError::InvalidStep { .. })
    ));
}

#[test]
fn monotonic_stream_banks_nothing() {
    let d = measured_dynamics().unwrap();
    let samples: Vec<f64> = (0..500).map(|i| i as f64).collect();
    let points = accumulate(&samples, &d).unwrap();
    assert!(approx(points, 0.0));
}

#[test]
fn measured_cap_holds_the_level_and_flags_the_cap() {
    let d = measured_dynamics().unwrap();
    let mut s = Session::new(d, 9 * 60).unwrap();
    let mut samples = vec![0.0];
    for i in 0..120 {
        samples.push(if i % 2 == 0 { -100.0 } else { 100.0 });
    }
    for _ in 0..4 {
        let out = s.run_attempt(&samples, 1.0 / 60.0).unwrap();
        assert!(out.success);
    }
    assert_eq!(s.level(), 4);
    let out = s.run_attempt(&samples, 1.0 / 60.0).unwrap();
    assert!(out.success);
    assert!(out.at_measured_cap);
    assert_eq!(out.level_after, 4);
}

#[test]
fn sixth_level_maximum_is_unknown_not_invented() {
    let d = measured_dynamics().unwrap();
    assert!(matches!(
        d.gauge_maximum(5),
        Err(CoinError::LevelTableExhausted { level: 5 })
    ));
}

#[test]
fn message_stats_floor_at_zero() {
    let d = measured_dynamics().unwrap();
    let mut s = Session::new(d, 9 * 60).unwrap();
    s.apply_message_stats(StatTriple {
        hp: -10_000.0,
        mp: 0.0,
        sp: 0.0,
    })
    .unwrap();
    assert!(approx(s.prep().hp, 0.0));
}
