use super::*;
use std::cmp::Ordering;
use std::hash::Hash;

/// Creates a "reverse" index by mapping keys of the given `map`
/// to the index value obtained by iterating it.
pub fn key_index<'a, T, M>(map: impl IntoIterator<Item = (&'a T, &'a M)>) -> HashMap<T, usize>
where
    T: Copy + Eq + Hash + 'a,
    M: 'a,
{
    map.into_iter()
        .enumerate()
        .map(|(i, (&t, _))| (t, i))
        .collect()
}

/// Searches a key-value `map` for the given `value`,
/// returning an owned key if it exists, else `None`.
pub fn try_from_value<'a, K, V>(
    map: impl IntoIterator<Item = (&'a K, &'a V)>,
    value: &V,
) -> Option<K>
where
    K: 'a + Copy,
    V: 'a + Eq,
{
    for (k, v) in map {
        if v == value {
            return Some(*k);
        }
    }
    None
}

/// for numerical types formattable to two decimals
pub trait Percent {
    fn trunc_three(self) -> Self;
}

impl Percent for f64 {
    /// Truncate to 3 significant figures
    fn trunc_three(self) -> Self {
        let val = (self * 1000.0).floor();
        val / 1000.0
    }
}

/// Computes the median value from a sorted sequence
///
/// ## Panics
/// If scores is empty
pub fn median_from_sorted(seq: &[f64]) -> f64 {
    assert!(!seq.is_empty());
    if seq.len() % 2 == 0 {
        // length even, avg two mid elements
        let lhs = (seq.len() / 2) - 1;
        let rhs = seq.len() / 2;
        let feerate = (seq[lhs] + seq[rhs]) / 2.0;
        feerate.trunc_three()
    } else {
        // length odd
        let mid = (seq.len() - 1) / 2;
        let feerate = seq[mid];
        feerate.trunc_three()
    }
}

/// Returns the value at the 90%-ile from a sorted sequence
///
/// ## Panics
/// If scores is empty
pub fn target_feerate(seq: &[f64]) -> f64 {
    assert!(!seq.is_empty());
    let largest_index = seq.len() - 1;
    let i = (largest_index as f64 * 0.9).trunc() as usize;
    seq[i].trunc_three()
}

/// `a` and `b` as (score, order, uid)
pub fn compare_audit_tx(a: (f64, u32, usize), b: (f64, u32, usize)) -> Option<Ordering> {
    if (a.0 - b.0).abs() > f64::EPSILON {
        // a != b
        a.0.partial_cmp(&b.0)
    } else if a.1 != b.1 {
        a.1.partial_cmp(&b.1)
    } else {
        a.2.partial_cmp(&b.2)
    }
}

/// `a` and `b` as (ancestor count, order, uid)
pub fn compare_ancestor_count(a: &(usize, u32, usize), b: &(usize, u32, usize)) -> Ordering {
    if a.0 != b.0 {
        // compare ancestor count
        a.2.cmp(&b.2)
    } else if a.1 != b.1 {
        // compare txid
        a.1.cmp(&b.1)
    } else {
        // fallback to uid
        a.2.cmp(&b.2)
    }
}

#[test]
fn finds_ninety_percentile() {
    let mut scores = vec![];
    for i in 1..=10 {
        scores.push(i as f64);
    }

    let res = target_feerate(&scores);
    assert_eq!(res, 9.0);

    scores = vec![0.0];
    let res = target_feerate(&scores);
    assert_eq!(res, 0.0);
}

#[test]
#[should_panic]
fn ninety_percentile_fails_empty_vec() {
    let empty = vec![];
    let _res = target_feerate(&empty);
}
