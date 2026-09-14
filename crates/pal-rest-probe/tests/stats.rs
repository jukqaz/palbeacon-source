use pal_rest_probe::{DistributionSummary, mean_confidence_interval_95};

#[test]
fn p95_uses_the_fixed_nearest_rank_rule() {
    let summary =
        DistributionSummary::from_samples((1..=100).map(f64::from)).expect("distribution");

    assert_eq!(summary.p50, 50.0);
    assert_eq!(summary.p95, 95.0);
}

#[test]
fn confidence_interval_uses_student_t_for_five_pairs() {
    let interval =
        mean_confidence_interval_95(&[0.0, 0.0, 0.0, 0.0, 3.0]).expect("confidence interval");

    assert!(interval.mean < 1.0);
    assert!(interval.upper > 2.0);
    assert_eq!(interval.sample_count, 5);
}

#[test]
fn non_finite_samples_are_rejected() {
    assert!(DistributionSummary::from_samples([1.0, f64::NAN]).is_err());
    assert!(mean_confidence_interval_95(&[1.0, f64::INFINITY]).is_err());
}
