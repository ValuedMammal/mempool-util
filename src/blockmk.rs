use super::*;
use crate::audittx::{AuditTx, TxPriority};
use bitcoin::hashes::Hash;
use bitcoin::Amount;
use bitcoin::Txid;
use priority_queue::PriorityQueue;
use serde::Serialize;
use std::cmp::Ordering;
use std::time;

/// Maximum block weight
const MAX_BLOCK_WU: u64 = 4_000_000;
/// Number of attempts to fit a package in a block before considering it full
const MAX_FAILURES: usize = 500;
/// The most blocks that `BlockAssembler` will build, provided sufficient inventory.
const BLOCK_GOAL: usize = 2;

/// Type for managing block assembly
struct BlockAssembler {
    pool: AuditPool,
    next_height: u64,
    fees: u64,
    weight: u64,
    inv: Inventory,
    blocks: Vec<BlockSummary>,
    modified: PriorityQueue<usize, TxPriority>,
    overflow: Vec<usize>,
}

/// Data the [`BlockAssembler`] keeps track of while generating blocks
struct Inventory {
    txn: Vec<usize>,
    scores: Vec<f64>,
    failures: usize,
    lo_score: f64,
    hi_score: f64,
}

/// Type for storing the result of `BlockAssembler::make_block`
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BlockSummary {
    /// Block height (projected)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
    /// List of mempool uid's
    #[serde(skip)]
    pub txn: Vec<usize>,
    /// Transaction count
    pub tx_count: usize,
    /// Block weight
    pub weight: u64,
    /// Block fees (btc)
    pub fees: f64,
    /// Feerate range (effective)
    pub fee_range: (f64, f64),
    /// Median effective feerate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_effective_feerate: Option<f64>,
    /// Ancestor score distribution
    #[serde(skip)]
    pub fee_histogram: Option<FeeHistogram>,
}

/// Defines the distribution of transaction weight across twelve feerate buckets
pub type FeeHistogram = [(&'static str, u64); 12];

impl Default for Inventory {
    fn default() -> Self {
        Self {
            txn: Vec::default(),
            scores: Vec::default(),
            failures: 0,
            lo_score: f64::INFINITY,
            hi_score: 0.0,
        }
    }
}

/// A collection of [`AuditTx`]s indexed by a unique primary key
pub type AuditPool = HashMap<usize, AuditTx>;

impl<S: std::hash::BuildHasher> Audit
    for HashMap<Txid, bitcoincore_rpc_json::GetMempoolEntryResult, S>
{
    /// Create an [`AuditPool`] from `self`
    fn into_pool(self) -> (TxidIndex, AuditPool) {
        let index = key_index(&self);

        let pool = self
            .into_iter()
            .map(|(txid, entry)| {
                // Parse mempool entries and initialize audit pool
                let uid = index.get(&txid).expect("txid exists");
                let txid_slice = &txid.as_byte_array()[28..];
                let buf: [u8; 4] = txid_slice.try_into().expect("4 bytes");
                let order = u32::from_be_bytes(buf);
                let fee = entry.fees.modified.to_sat();
                let weight = entry.weight.unwrap_or(entry.vsize * 4);
                let parents = entry
                    .depends
                    .iter()
                    .map(|input_txid| *index.get(input_txid).expect("txid exists"))
                    .collect();
                // Create audit tx
                let mut audittx = AuditTx {
                    uid: *uid,
                    order,
                    fee,
                    weight,
                    parents,
                    ..Default::default()
                };
                audittx.pre_fill();

                (*uid, audittx)
            })
            .collect();

        (index, pool)
    }
}

impl Cluster for BlockAssembler {
    /// Recursively walks in-mempool ancestors of `uid`, setting ancestor/descendant links
    /// and computes the ancestor score.
    fn set_links(&mut self, uid: usize) {
        let tx = self.pool.get(&uid).expect("uid exists");
        if tx.links_set {
            return;
        }

        // get parent uid's
        // (clone to avoid holding the borrow in the next step)
        let parents = tx.parents.clone();

        // get ancestor uid's for this tx
        let mut ancestors: HashSet<usize> = HashSet::new();
        for parent_id in parents {
            /* recursive step */
            self.set_links(parent_id);
            let parent = self.pool.get_mut(&parent_id).expect("uid exists");

            // add this uid to the parent's children
            parent.children.insert(uid);

            // include this parent as an ancestor
            ancestors.insert(parent.uid);

            // collect parent's ancestors
            for ancestor_id in &parent.ancestors {
                ancestors.insert(*ancestor_id);
            }
        }

        // count ancestor data
        let mut ancestor_fee = 0u64;
        let mut ancestor_weight = 0u64;
        for ancestor_id in &ancestors {
            let ancestor = self.pool.get(ancestor_id).expect("uid exists");
            ancestor_fee += ancestor.fee;
            ancestor_weight += ancestor.weight;
        }

        // score this tx
        let tx = self.pool.get_mut(&uid).expect("uid exists");
        tx.ancestors = ancestors;
        tx.ancestor_fee += ancestor_fee;
        tx.ancestor_weight += ancestor_weight;
        tx.score = tx.ancestor_fee as f64 / (tx.ancestor_weight as f64 / 4.0);
        tx.links_set = true;
    }
}

impl BlockAssembler {
    /// Creates a new empty [`BlockAssembler`]
    fn new() -> Self {
        Self {
            pool: AuditPool::new(),
            next_height: 0,
            fees: 0,
            weight: 4000,
            inv: Inventory::default(),
            blocks: vec![],
            modified: PriorityQueue::new(),
            overflow: vec![],
        }
    }

    /// Creates a [`BlockAssembler`] from a given `audit_pool`
    #[allow(unused)]
    fn from(audit_pool: AuditPool) -> Self {
        let mut maker = BlockAssembler::new();
        maker.pool = audit_pool;
        maker
    }

    /// Creates a [`BlockAssembler`] from a given `audit_pool` and block `height`
    fn from_pool_with_height(audit_pool: AuditPool, height: u64) -> Self {
        let mut maker = BlockAssembler::new();
        maker.pool = audit_pool;
        maker.next_height = height;
        maker
    }

    /// Resets accumulating statistics to prepare for building the next block
    fn clear(&mut self) {
        self.fees = 0;
        self.weight = 4000;
        self.inv = Inventory::default();
        self.next_height += 1;
    }

    /// Whether the current block is at capacity (99.9%)
    fn is_full(&self) -> bool {
        let margin = MAX_BLOCK_WU / 1000;
        self.weight >= (MAX_BLOCK_WU - margin)
    }

    /// Test if the given package will fit in the candidate block
    fn test_package_fits(&self, tx: &AuditTx) -> bool {
        self.weight + tx.ancestor_weight < MAX_BLOCK_WU
    }

    /// Select the given `tx` and its ancestors for inclusion in a block,
    /// returning a copy of the package uids
    fn add_package_tx(&mut self, tx: &AuditTx) -> Vec<usize> {
        let package = if tx.ancestors.is_empty() {
            vec![tx.uid]
        } else {
            // To ensure good sorting in block, we sort the package by ancestor
            // count, breaking ties with either the tx order or uid.
            let mut sorted = vec![(tx.ancestors.len(), tx.order, tx.uid)];
            for uid in &tx.ancestors {
                let ancestor = self.pool.get(uid).expect("uid exists");
                sorted.push((ancestor.ancestors.len(), ancestor.order, ancestor.uid));
                if ancestor.modified {
                    self.modified.remove(&ancestor.uid);
                }
            }
            sorted.sort();
            let package: Vec<usize> = sorted.into_iter().map(|(_, _, uid)| uid).collect();
            package
        };

        // Add this package
        for uid in &package {
            if let Some(tx) = self.pool.get_mut(uid) {
                if !tx.used {
                    self.inv.txn.push(tx.uid);
                }
                tx.used = true;
                self.weight += tx.weight;
                self.fees += tx.fee;
                self.inv.lo_score = self.inv.lo_score.min(tx.score);
                self.inv.hi_score = self.inv.hi_score.max(tx.score);
                self.inv.scores.push(tx.score);
            }
        }
        package
    }

    /// Creates a new [`BlockSummary`]
    fn make_block(&mut self, is_full: bool) -> BlockSummary {
        let txn: Vec<usize> = self.inv.txn.clone();
        let tx_count = txn.len();

        let mut height: Option<u64> = None;
        let mut fee_histogram: Option<FeeHistogram> = None;
        let mut median_effective_feerate: Option<f64> = None;

        if is_full {
            height = Some(self.next_height);
            fee_histogram = Some(self.histogram_generate(&txn));
            self.inv.scores.sort_by(|a, b| a.total_cmp(b));
            let scores = &self.inv.scores;
            median_effective_feerate = Some(median_from_sorted(scores));
        }

        let fee_range = {
            let lo = truncate!(self.inv.lo_score);
            let hi = truncate!(self.inv.hi_score);
            (lo, hi)
        };

        BlockSummary {
            height,
            txn,
            tx_count,
            weight: self.weight,
            fees: Amount::from_sat(self.fees).to_btc(),
            fee_range,
            median_effective_feerate,
            fee_histogram,
        }
    }

    /// Generates block projections provided `self` has data to work on
    fn generate(mut self) -> Vec<BlockSummary> {
        let start = time::Instant::now();
        for uid in 0..self.pool.len() {
            self.set_links(uid);
        }

        // Sort by ancestor score (ascending), and create a stack of uids
        let mut pool_stack: Vec<&AuditTx> = self.pool.values().collect();
        pool_stack.sort();
        let mut pool_stack: Vec<usize> = pool_stack.into_iter().map(|tx| tx.uid).collect();

        // Build blocks
        while !pool_stack.is_empty() || !self.modified.is_empty() {
            let next_tx = self.next_audit_tx(&mut pool_stack);
            let next_modified_tx = self.next_modified_tx();
            let tx = match (&next_tx, &next_modified_tx) {
                (None, None) => {
                    break;
                }
                (None, Some(modtx)) => {
                    self.modified.pop();
                    modtx
                }
                (Some(tx), None) => {
                    pool_stack.pop();
                    tx
                }
                (Some(tx), Some(modtx)) => {
                    match tx.cmp(modtx) {
                        Ordering::Equal => {
                            self.modified.pop();
                            pool_stack.pop(); // drop duplicates
                            modtx
                        }
                        Ordering::Less => {
                            self.modified.pop();
                            modtx
                        }
                        Ordering::Greater => {
                            pool_stack.pop();
                            tx
                        }
                    }
                }
            };

            // Check if this package fits, or if we're done building blocks, continue on packages until queues empty
            if self.test_package_fits(tx) || self.blocks.len() >= BLOCK_GOAL {
                let package = self.add_package_tx(tx);
                for uid in package {
                    let tx = self.pool.get(&uid).expect("uid exists");
                    if !tx.children.is_empty() {
                        self.update_descendants(tx.uid);
                    }
                }
                self.inv.failures = 0;
            } else {
                self.overflow.push(tx.uid);
                self.inv.failures += 1;
            }

            let exceeded_attempts = self.inv.failures >= MAX_FAILURES && self.is_full();
            let queue_empty = pool_stack.is_empty() && self.modified.is_empty();
            if (exceeded_attempts || queue_empty) && self.blocks.len() < BLOCK_GOAL {
                // Build this block
                let block = self.make_block(/*is_full: */ true);
                self.blocks.push(block);
                self.clear();

                // Recycle overflow
                while let Some(uid) = self.overflow.pop() {
                    let tx = self.pool.get(&uid).expect("uid exists");
                    if tx.used {
                        continue;
                    }
                    if tx.modified {
                        self.modified.push_increase(
                            tx.uid,
                            TxPriority {
                                uid: tx.uid,
                                order: tx.order,
                                score: tx.score,
                            },
                        );
                    } else {
                        pool_stack.push(tx.uid);
                    }
                }
            }
        }

        if !self.inv.txn.is_empty() {
            // Collect remaining tx in a final unbounded block
            let block = self.make_block(false);
            self.blocks.push(block);
        }
        log::debug!(
            "Finished making blocks in {}ms",
            start.elapsed().as_millis()
        );
        self.blocks
    }

    /// Walk remaining descendants, removing this ancestor `uid` and updating scores
    fn update_descendants(&mut self, uid: usize) {
        let mut visited = vec![];
        let mut descendant_stack = vec![];

        // get this tx's children
        let ancestor = self.pool.get(&uid).expect("uid exist");
        let root_fee = ancestor.fee;
        let root_weight = ancestor.weight;
        for child in &ancestor.children {
            if !visited.contains(child) {
                descendant_stack.push(*child);
                visited.push(*child);
            }
        }

        while let Some(descendant) = descendant_stack.pop() {
            let tx = self.pool.get_mut(&descendant).expect("uid exists");

            // Add this descendant's children to the descendant stack
            for child in &tx.children {
                if !visited.contains(child) {
                    descendant_stack.push(*child);
                    visited.push(*child);
                }
            }

            // Remove root tx as ancestor
            if tx.ancestors.remove(&uid) {
                tx.ancestor_fee -= root_fee;
                tx.ancestor_weight -= root_weight;
                let old_score = tx.score;
                tx.score = tx.ancestor_fee as f64 / (tx.ancestor_weight as f64 / 4.0);

                // Add or update modified queue
                if tx.score < old_score {
                    tx.modified = true;
                    self.modified.push_decrease(
                        tx.uid,
                        TxPriority {
                            uid: tx.uid,
                            order: tx.order,
                            score: tx.score,
                        },
                    );
                } else if tx.score > old_score {
                    tx.modified = true;
                    self.modified.push_increase(
                        tx.uid,
                        TxPriority {
                            uid: tx.uid,
                            order: tx.order,
                            score: tx.score,
                        },
                    );
                }
            }
        }
    }
}

/* Called from main */
/// Produce a fee report from the given mempool entries and block height
pub fn audit_fees(height: u64, entries: impl Audit) -> Vec<BlockSummary> {
    let (_index, pool) = entries.into_pool();
    let maker = BlockAssembler::from_pool_with_height(pool, height);
    maker.generate()
}

/**  Helpers */
impl BlockAssembler {
    /// Iterate audit pool returning an option to the first unused tx (cloned), else None
    fn next_audit_tx(&self, pool_stack: &mut Vec<usize>) -> Option<AuditTx> {
        let mut next_tx: Option<AuditTx> = None;
        while !pool_stack.is_empty() {
            let uid = pool_stack.iter().next_back().expect("pool not empty");
            let tx = self.pool.get(uid).expect("uid exists");
            if !tx.used {
                next_tx = Some((*tx).clone());
                break;
            }
            pool_stack.pop();
        }
        next_tx
    }

    /// Iterate modified queue returning an option to the first unused tx (cloned), else None
    fn next_modified_tx(&mut self) -> Option<AuditTx> {
        let mut next_modtx: Option<AuditTx> = None;
        while !self.modified.is_empty() {
            let (uid, _) = self.modified.peek().expect("modified not empty");
            let modtx = self.pool.get(uid).expect("uid exists");
            if !modtx.used {
                next_modtx = Some((*modtx).clone());
                break;
            }
            self.modified.pop();
        }
        next_modtx
    }

    /// Creates a [`FeeHistogram`] from the given tx uids
    fn histogram_generate(&self, txs: &[usize]) -> FeeHistogram {
        let mut histogram: [(&'static str, u64); 12] = [
            ("1-2", 0),
            ("2-3", 0),
            ("3-4", 0),
            ("4-5", 0),
            ("5-10", 0),
            ("10-15", 0),
            ("15-20", 0),
            ("20-25", 0),
            ("25-50", 0),
            ("50-100", 0),
            ("100-500", 0),
            ("500+", 0),
        ];
        for uid in txs {
            let tx = self.pool.get(uid).expect("uid exists");
            let idx = match tx.score {
                s if s < 2.0 => 0usize,
                s if s < 3.0 => 1,
                s if s < 4.0 => 2,
                s if s < 5.0 => 3,
                s if s < 10.0 => 4,
                s if s < 15.0 => 5,
                s if s < 20.0 => 6,
                s if s < 25.0 => 7,
                s if s < 50.0 => 8,
                s if s < 100.0 => 9,
                s if s < 500.0 => 10,
                _ => 11,
            };
            histogram[idx].1 += tx.weight;
        }
        histogram
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::TestMempoolEntry;

    fn entry_with_fee_and_parent(uid: usize, fee: u64, parent: Option<usize>) -> TestMempoolEntry {
        let parents = if let Some(p) = parent {
            let mut parents = HashSet::new();
            parents.insert(p);
            parents
        } else {
            HashSet::new()
        };
        TestMempoolEntry {
            uid,
            fee,
            weight: 800,
            parents,
        }
    }

    #[test]
    fn txid_order() {
        let txid: bitcoin::Txid =
            "2a000000dc54bcdc99390c01cbc27bed78693233e54a9eda6cd316d87ed8d18f"
                .parse()
                .unwrap();
        let slice = &txid[28..]; // 0000002a
        assert_eq!(slice.len(), 4);
        //dbg!(slice);

        let arr: [u8; 4] = slice.try_into().unwrap();
        let n = u32::from_be_bytes(arr);
        //dbg!(n);
        assert_eq!(n, 42);
    }

    #[test]
    fn test_no_entries() {
        let maker = BlockAssembler::from(AuditPool::new());
        maker.generate();
    }

    #[test]
    fn sort_audit_tx() {
        let (_, pool) = vec![
            entry_with_fee_and_parent(0, 4000, None),
            entry_with_fee_and_parent(1, 1000, None),
            entry_with_fee_and_parent(2, 2000, None),
        ]
        .into_pool();

        let mut txs: Vec<AuditTx> = pool.into_values().collect();
        txs.sort(); // ascending feerate
        let uids: Vec<usize> = txs.into_iter().map(|tx| tx.uid).collect();
        let expect = vec![1, 2, 0];
        assert_eq!(uids, expect);
    }

    #[test]
    fn make_single() {
        let (_, pool) = vec![TestMempoolEntry {
            uid: 0usize,
            fee: 1000,
            weight: 840,
            parents: HashSet::new(),
        }]
        .into_pool();
        let maker = BlockAssembler::from(pool);
        let blocks = maker.generate();
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn make_some() {
        let parent = 0usize;
        let parent1 = 1usize;
        let child = 2usize;
        let child1 = 3usize;

        let (_, pool) = vec![
            entry_with_fee_and_parent(parent, 4000, None),
            entry_with_fee_and_parent(child, 1000, Some(parent)),
            entry_with_fee_and_parent(parent1, 4000, None),
            entry_with_fee_and_parent(child1, 1000, Some(parent1)),
        ]
        .into_pool();

        let maker = BlockAssembler::from(pool);
        let blocks = maker.generate();
        let exp = vec![parent1, parent, child1, child];
        let txs = &blocks[0].txn;
        //dbg!(txs);
        assert_eq!(txs, &exp);
    }

    #[test]
    fn make_many() {
        // should return three blocks:
        // 2 hardcoded limit
        // +1 unbounded with remaining tx

        // Approach: for a max tx weight of 400kWU and a max block weight
        // of 4MWU, that leaves room for around 10 tx in a block
        // For the test, we pass 30 max-weight txs to the block assembler
        // and expect three blocks to be returned.
        let mut entries: Vec<TestMempoolEntry> = vec![];
        for i in 0usize..30 {
            entries.push(TestMempoolEntry {
                uid: i,
                fee: 100_000,
                weight: 396_000,
                parents: HashSet::new(),
            });
        }
        let maker = BlockAssembler::from(entries.into_pool().1);
        let blocks = maker.generate();
        let mut txs = vec![];
        for block in &blocks {
            for tx in &block.txn {
                txs.push(*tx);
            }
        }
        // No tx missing, no duplicates
        assert_eq!(txs.len(), 30);
        for i in 0usize..30 {
            assert!(txs.contains(&i));
        }
    }

    #[test]
    fn test_set_links() {
        let ancestor = 0usize;
        let parent = 1usize;
        let child = 2usize;
        let gchild = 3usize;
        let mut maker = BlockAssembler::from(
            vec![
                entry_with_fee_and_parent(ancestor, 1000, None),
                entry_with_fee_and_parent(parent, 2000, Some(ancestor)),
                entry_with_fee_and_parent(child, 2000, Some(parent)),
                entry_with_fee_and_parent(gchild, 4000, Some(child)),
            ]
            .into_pool()
            .1,
        );
        maker.set_links(gchild);

        // all children have at least one ancestor
        for uid in 1..4 {
            let tx = maker.pool.get(&uid).unwrap();
            assert!(!tx.ancestors.is_empty());
        }

        // all parents have one child
        let ancestor = maker.pool.get(&ancestor).unwrap();
        assert!(ancestor.children.contains(&parent));
        let parent = maker.pool.get(&parent).unwrap();
        assert!(parent.children.contains(&child));
        let child = maker.pool.get(&child).unwrap();
        assert!(child.children.contains(&gchild));
        let gchild = maker.pool.get(&gchild).unwrap();

        // descendants have new ancestor scores
        assert_eq!(ancestor.score, 5.0);
        assert_eq!(parent.score, 7.5);
        assert_eq!(child.score, (5000.0 / (2400.0 / 4.0)));
        assert_eq!(gchild.score, (9000.0 / (3200.0 / 4.0)));
    }

    #[test]
    fn test_update_descendants() {
        let ancestor = 0usize;
        let parent = 1usize;
        let child = 2usize;
        let mut maker = BlockAssembler::from(
            vec![
                entry_with_fee_and_parent(ancestor, 1000, None),
                entry_with_fee_and_parent(parent, 2000, Some(ancestor)),
                entry_with_fee_and_parent(child, 4000, Some(parent)),
            ]
            .into_pool()
            .1,
        );
        maker.set_links(child);

        // Ancestor removed from descendant's ancestors
        let ancestor_id = 0usize;
        maker.update_descendants(0);
        let parent = maker.pool.get(&1).unwrap();
        assert!(!parent.ancestors.contains(&ancestor_id));
        let child = maker.pool.get(&2).unwrap();
        assert!(!child.ancestors.contains(&ancestor_id));

        // Score changed
        // if a (comparatively) lo-fee ancestor is now in block,
        // expect descendant score increase.
        // likewise, if hi-fee ancestor is in block,
        // expect descendant score decrease.
        let old_score = 7000.0 / (2400.0 / 4.0);
        let score = 6000.0 / (1600.0 / 4.0);
        assert!(child.score > old_score);
        let expect = TxPriority {
            uid: 2,
            order: 2,
            score,
        };

        // Modified queue has two entries and
        // child is front of queue
        assert_eq!(maker.modified.len(), 2);
        let (uid, priority) = maker.modified.pop().unwrap();
        assert_eq!(uid, child.uid);
        assert_eq!(priority, expect);
    }
}
