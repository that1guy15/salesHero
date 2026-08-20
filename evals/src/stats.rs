//! Distribution summaries. Small enough to keep honest by inspection.

/// Percentile by nearest rank on a sorted slice. Returns `None` when empty, so
/// a missing distribution reads as "no data" rather than as zero — the
/// difference matters when deciding whether a threshold is supportable.
pub fn percentile(sorted: &[f32], p: f32) -> Option<f32> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (p / 100.0 * sorted.len() as f32).ceil().max(1.0) as usize;
    sorted.get(rank - 1).copied()
}

pub fn sorted(mut v: Vec<f32>) -> Vec<f32> {
    v.sort_by(f32::total_cmp);
    v
}

pub fn min(v: &[f32]) -> Option<f32> {
    v.first().copied()
}

pub fn max(v: &[f32]) -> Option<f32> {
    v.last().copied()
}

pub fn mean(v: &[f32]) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    Some(v.iter().sum::<f32>() / v.len() as f32)
}

/// Formats an optional score for a report column. `—` rather than `0.000`,
/// because a distribution with no samples is not a distribution of zeroes.
pub fn fmt(v: Option<f32>) -> String {
    v.map_or_else(|| "  —  ".into(), |x| format!("{x:.3}"))
}
