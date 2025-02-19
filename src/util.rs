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

/// Computes the median value from a sorted sequence.
///
/// ## Panics
///
/// If `seq` is empty
pub fn median_from_sorted(seq: &[f64]) -> f64 {
    assert!(!seq.is_empty());
    if seq.len() % 2 == 0 {
        // length even, avg two mid elements
        let lhs = (seq.len() / 2) - 1;
        let rhs = seq.len() / 2;
        let feerate = (seq[lhs] + seq[rhs]) / 2.0;
        truncate!(feerate)
    } else {
        // length odd
        let mid = (seq.len() - 1) / 2;
        let feerate = seq[mid];
        truncate!(feerate)
    }
}

/// `a` and `b` as (score, order, uid)
pub fn compare_audit_tx(a: (f64, u32, usize), b: (f64, u32, usize)) -> Ordering {
    if (a.0 - b.0).abs() > f64::EPSILON {
        // a != b
        a.0.total_cmp(&b.0)
    } else if a.1 != b.1 {
        a.1.cmp(&b.1)
    } else {
        a.2.cmp(&b.2)
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
