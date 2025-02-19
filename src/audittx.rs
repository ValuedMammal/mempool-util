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
    pub descendant_score: f64,
    pub links_set: bool,
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
            descendant_score: f64::INFINITY,
            links_set: false,
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
        self.links_set = self.parents.is_empty();
    }
}

impl PartialEq for AuditTx {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
    }
}

impl Eq for AuditTx {}

impl Ord for AuditTx {
    fn cmp(&self, other: &Self) -> Ordering {
        let a = (self.score, self.order, self.uid);
        let b = (other.score, other.order, other.uid);
        compare_audit_tx(a, b)
    }
}

impl PartialOrd for AuditTx {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Defines how a scored mempool entry is prioritized by the modified queue
#[derive(Debug)]
pub struct TxPriority {
    pub uid: usize,
    pub order: u32,
    pub score: f64,
}

impl PartialEq for TxPriority {
    fn eq(&self, other: &Self) -> bool {
        self.uid == other.uid
    }
}

impl Eq for TxPriority {}

impl Ord for TxPriority {
    fn cmp(&self, other: &Self) -> Ordering {
        let a = (self.score, self.order, self.uid);
        let b = (other.score, other.order, other.uid);
        compare_audit_tx(a, b)
    }
}

impl PartialOrd for TxPriority {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
