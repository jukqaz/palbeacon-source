use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A bounded aggregate. The observations used to construct it are never retained.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DistributionSummary {
    pub count: u64,
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
}

impl DistributionSummary {
    pub fn from_samples<I>(samples: I) -> Result<Self, StatsError>
    where
        I: IntoIterator<Item = f64>,
    {
        let mut sorted = samples.into_iter().collect::<Vec<_>>();
        if sorted.is_empty() {
            return Err(StatsError::Empty);
        }
        if sorted.iter().any(|sample| !sample.is_finite()) {
            return Err(StatsError::NonFinite);
        }

        sorted.sort_by(f64::total_cmp);
        let count = u64::try_from(sorted.len()).map_err(|_| StatsError::TooManySamples)?;
        let sum = sorted.iter().try_fold(0.0, |sum, sample| {
            let next = sum + sample;
            next.is_finite().then_some(next).ok_or(StatsError::Overflow)
        })?;
        let mean = sum / count as f64;

        Ok(Self {
            count,
            min: sorted[0],
            max: sorted[sorted.len() - 1],
            mean,
            p50: nearest_rank(&sorted, 50),
            p95: nearest_rank(&sorted, 95),
        })
    }

    pub fn from_constant(value: f64, count: u64) -> Result<Self, StatsError> {
        if count == 0 {
            return Err(StatsError::Empty);
        }
        if !value.is_finite() {
            return Err(StatsError::NonFinite);
        }
        Ok(Self {
            count,
            min: value,
            max: value,
            mean: value,
            p50: value,
            p95: value,
        })
    }

    pub fn is_valid_nonnegative(self) -> bool {
        self.count > 0
            && [self.min, self.max, self.mean, self.p50, self.p95]
                .into_iter()
                .all(|value| value.is_finite() && value >= 0.0)
            && self.min <= self.p50
            && self.p50 <= self.p95
            && self.p95 <= self.max
            && self.min <= self.mean
            && self.mean <= self.max
    }
}

fn nearest_rank(sorted: &[f64], percentile: usize) -> f64 {
    debug_assert!(!sorted.is_empty());
    debug_assert!((1..=100).contains(&percentile));
    let rank = (percentile * sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1)]
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeanConfidenceInterval {
    pub sample_count: u64,
    pub mean: f64,
    pub lower: f64,
    pub upper: f64,
}

/// Returns a two-sided 95% Student-t interval. No bootstrap or normal fallback is used.
pub fn mean_confidence_interval_95(samples: &[f64]) -> Result<MeanConfidenceInterval, StatsError> {
    if samples.len() < 2 {
        return Err(StatsError::InsufficientSamples);
    }
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err(StatsError::NonFinite);
    }

    let sample_count = u64::try_from(samples.len()).map_err(|_| StatsError::TooManySamples)?;
    let count = sample_count as f64;
    let sum = samples.iter().try_fold(0.0, |sum, sample| {
        let next = sum + sample;
        next.is_finite().then_some(next).ok_or(StatsError::Overflow)
    })?;
    let mean = sum / count;
    let squared_deviations = samples.iter().try_fold(0.0, |sum, sample| {
        let delta = sample - mean;
        let next = sum + delta * delta;
        next.is_finite().then_some(next).ok_or(StatsError::Overflow)
    })?;
    let standard_error = (squared_deviations / (count - 1.0)).sqrt() / count.sqrt();
    let critical = t_critical_95(samples.len() - 1);
    let margin = critical * standard_error;

    Ok(MeanConfidenceInterval {
        sample_count,
        mean,
        lower: mean - margin,
        upper: mean + margin,
    })
}

fn t_critical_95(degrees_of_freedom: usize) -> f64 {
    const SMALL_DF: [f64; 30] = [
        12.706, 4.303, 3.182, 2.776, 2.571, 2.447, 2.365, 2.306, 2.262, 2.228, 2.201, 2.179, 2.160,
        2.145, 2.131, 2.120, 2.110, 2.101, 2.093, 2.086, 2.080, 2.074, 2.069, 2.064, 2.060, 2.056,
        2.052, 2.048, 2.045, 2.042,
    ];
    match degrees_of_freedom {
        0 => f64::INFINITY,
        1..=30 => SMALL_DF[degrees_of_freedom - 1],
        31..=40 => 2.042,
        41..=60 => 2.021,
        61..=120 => 2.000,
        _ => 1.980,
    }
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum StatsError {
    #[error("aggregate requires at least one observation")]
    Empty,
    #[error("confidence interval requires at least two observations")]
    InsufficientSamples,
    #[error("observation is not finite")]
    NonFinite,
    #[error("aggregate overflowed")]
    Overflow,
    #[error("sample count does not fit in the report")]
    TooManySamples,
}
