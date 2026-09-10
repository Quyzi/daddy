//! Tiny numeric helpers shared by more than one layout stage.

/// Runs a few iterations of 1-D 2-means on `data`, returning the two
/// cluster centroids (unordered, `c0 <= c1` not guaranteed). Degenerates
/// gracefully to `(v, v)` when all values are identical.
pub(crate) fn kmeans_1d_2(data: &[f64]) -> (f64, f64) {
    let min = data.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = data.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if (max - min).abs() < 1e-9 {
        return (min, max);
    }
    let (mut c0, mut c1) = (min, max);
    for _ in 0..10 {
        let (mut sum0, mut n0, mut sum1, mut n1) = (0.0, 0u32, 0.0, 0u32);
        for &v in data {
            if (v - c0).abs() <= (v - c1).abs() {
                sum0 += v;
                n0 += 1;
            } else {
                sum1 += v;
                n1 += 1;
            }
        }
        if n0 > 0 {
            c0 = sum0 / n0 as f64;
        }
        if n1 > 0 {
            c1 = sum1 / n1 as f64;
        }
    }
    (c0, c1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separates_two_obvious_clusters() {
        let data = vec![1.0, 1.1, 0.9, 1.05, 10.0, 10.2, 9.8, 10.1];
        let (a, b) = kmeans_1d_2(&data);
        let (lo, hi) = (a.min(b), a.max(b));
        assert!((lo - 1.0).abs() < 0.5);
        assert!((hi - 10.0).abs() < 0.5);
    }

    #[test]
    fn degenerates_on_identical_values() {
        let (a, b) = kmeans_1d_2(&[5.0, 5.0, 5.0]);
        assert_eq!(a, 5.0);
        assert_eq!(b, 5.0);
    }
}
