use super::*;
use std::cmp::Ordering;

/// Transaction metadata used for scoring packages during tx selection
#[derive(Clone, Debug)]
pub struct AuditTx {
    pub uid: usize,
    pub order: u32,
    pub fee: u64,
    pub weight: u64,
    pub feerate: f64,
    //sigops: u32,
    pub parents: HashSet<usize>,
    pub ancestors: HashSet<usize>,
    pub children: HashSet<usize>,
    pub ancestor_fee: u64,
    pub ancestor_weight: u64,
    //ancestor_sigops: u32,
    pub score: f64,
    pub used: bool,
    pub modified: bool,
    pub dependency_rate: f64,
    pub relatives_set: bool,
}

impl Default for AuditTx {
    fn default() -> Self {
        Self {
            uid: 0,
            order: 0,
            fee: 0,
            weight: 0,
            feerate: 0.0,
            //sigops: 0,
            parents: HashSet::default(),
            ancestors: HashSet::default(),
            children: HashSet::default(),
            ancestor_fee: 0,
            ancestor_weight: 0,
            //ancestor_sigops: 0,
            score: 0.0,
            used: false,
            modified: false,
            dependency_rate: f64::INFINITY,
            relatives_set: false,
        }
    }
}

impl AuditTx {
    /// Sets more sane defaults for an [`AuditTx`], given initial conditions
    pub fn pre_fill(&mut self) {
        let feerate = self.fee as f64 / (self.weight as f64 / 4.0);
        self.feerate = feerate;
        self.ancestor_fee = self.fee;
        self.ancestor_weight = self.weight;
        //self.ancestor_sigops = self.sigops;
        self.score = feerate;
        self.relatives_set = self.parents.is_empty();
    }
}

/// Defines how a scored mempool entry is prioritized by the modified queue
#[derive(Debug)]
pub struct TxPriority {
    pub uid: usize,
    pub order: u32,
    pub score: f64,
}

impl PartialEq for AuditTx {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
    }
}

impl Eq for AuditTx {}

impl PartialOrd for AuditTx {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let a = (self.score, self.order, self.uid);
        let b = (other.score, other.order, other.uid);
        compare_audit_tx(a, b)
    }
}

impl Ord for AuditTx {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).expect("ordering is Some")
    }
}

impl PartialEq for TxPriority {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
    }
}

impl Eq for TxPriority {}

impl PartialOrd for TxPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let a = (self.score, self.order, self.uid);
        let b = (other.score, other.order, other.uid);
        compare_audit_tx(a, b)
    }
}

impl Ord for TxPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).expect("ordering is Some")
    }
}
