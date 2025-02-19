use super::*;
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
