use serde::Serialize;

use bitcoin::Txid;

use super::*;

/// Type that holds mempool entries to be analyzed
#[derive(Debug)]
struct Auditor {
    /// Collection of uniquely identifiable mempool entries
    pool: HashMap<usize, Entry>,
}

/// Result of analyzing a local mempool for clusters of related tx
#[derive(Debug, Serialize)]
pub struct ClusterResult {
    /// Max descendant tree depth
    pub depth: u32,
    /// Largest cluster size
    pub size: u32,
    /// Most common ancestors
    pub ancestors: Vec<Txid>,
    /// Total clusters
    pub count: usize,
}

/// A stripped-down mempool entry suitable for cluster analysis
#[derive(Debug, Default)]
struct Entry {
    uid: usize,
    children: HashSet<usize>,
    parents: HashSet<usize>,
    ancestors: HashSet<usize>,
    links_set: bool,
}

impl Auditor {
    /// Creates a new [`Auditor`] from the given `pool`
    fn from(pool: HashMap<usize, Entry>) -> Self {
        Self { pool }
    }

    /// Returns the uids of the current mempool ancestors. Note, to be considered
    /// an ancestor, an entry must have direct descendants while having no ancestors
    /// of its own.
    fn ancestors(&self) -> Option<Vec<&Entry>> {
        let res: Vec<&Entry> = self
            .pool
            .values()
            .filter(|tx| tx.ancestors.is_empty() && !tx.children.is_empty())
            .collect();

        if res.is_empty() {
            return None;
        }

        Some(res)
    }
}

/* Called from main */
/// Handles analyzing mempool clusters
pub fn analyze(
    entries: HashMap<Txid, bitcoincore_rpc_json::GetMempoolEntryResult>,
) -> ClusterResult {
    let index = util::key_index(&entries);
    let pool = pool_from_entries_with_index(entries, &index);
    let mut auditor = Auditor::from(pool);

    for uid in 0..auditor.pool.len() {
        auditor.set_links(uid);
    }

    let (size, ancestor_ids) = auditor.most_common_ancestors();
    let ancestors: Vec<Txid> = ancestor_ids
        .iter()
        .map(|uid| util::try_from_value(&index, uid).expect("uid exist"))
        .collect();

    let depth = auditor.max_descendant_depth();
    let count = auditor.cluster_count();

    ClusterResult {
        depth,
        size,
        ancestors,
        count,
    }
}

impl Auditor {
    /// Returns the total descendant count of the largest cluster, along with
    /// the uid (or uids in the case of a tie) corresponding to the root
    /// ancestor of the associated cluster.
    fn most_common_ancestors(&self) -> (u32, Vec<usize>) {
        // create a histogram representing the count of
        // each ancestor in the mempool
        let mut map = HashMap::<usize, u32>::new();

        let ancestors: Vec<usize> = self
            .pool
            .values()
            .flat_map(|tx| tx.ancestors.iter().copied())
            .collect();

        for ancestor in ancestors {
            map.entry(ancestor).and_modify(|ct| *ct += 1).or_insert(1);
        }

        // get highest count
        let mut hi_count = 1u32;
        map.iter().for_each(|(_, &ct)| hi_count = hi_count.max(ct));

        let ancestors: Vec<usize> = map
            .into_iter()
            .filter(|(_, ct)| *ct == hi_count)
            .map(|(uid, _)| uid)
            .collect();

        (hi_count, ancestors)
    }

    /// Returns the maximum tree height of all ancestors.
    fn max_descendant_depth(&self) -> u32 {
        let mut heights = vec![];
        let Some(ancestors) = self.ancestors() else {
            // nothing to do
            return 0;
        };
        for root in ancestors {
            heights.push(self.tree_height(root));
        }
        heights.sort_unstable();
        heights.pop().expect("collected tree heights is not empty")
    }

    /// Computes height of a tree given a root node, based on an algorithm for finding
    /// the height of a binary tree, generalized for nodes with many children.
    fn tree_height(&self, tx: &Entry) -> u32 {
        if tx.children.is_empty() {
            return 0;
        }

        let mut heights: Vec<u32> = vec![];
        for child in &tx.children {
            let tx = self.pool.get(child).expect("uid exists");
            /* recursive step */
            heights.push(self.tree_height(tx));
        }

        heights.sort_unstable();
        heights.pop().expect("children not empty") + 1
    }

    /// Counts the number of mempool clusters
    fn cluster_count(&self) -> usize {
        // count defined as number of tx having at least one child
        // and having no ancestors
        let ancestors: Vec<&Entry> = self
            .pool
            .values()
            .filter(|tx| tx.ancestors.is_empty() && !tx.children.is_empty())
            .collect();

        ancestors.len()
    }

    /// Counts the number of descendants of the given `tx` entry
    #[deprecated]
    #[allow(unused)]
    fn count_descendants(&self, tx: &Entry) -> usize {
        if tx.children.is_empty() {
            return 0;
        }

        let mut ct = tx.children.len();

        for child in &tx.children {
            let tx = self.pool.get(child).unwrap();
            ct += self.count_descendants(tx);
        }
        ct
    }
}

impl Cluster for Auditor {
    fn set_links(&mut self, uid: usize) {
        let tx = self.pool.get(&uid).expect("uid exists");
        if tx.links_set {
            return;
        }

        // get this tx's parents
        // (clone to avoid holding the borrow in the next step)
        let parents = tx.parents.clone();

        // get ancestor uid's for this tx
        let mut ancestors: HashSet<usize> = HashSet::new();
        for parent_id in parents {
            /* recursive step */
            self.set_links(parent_id);
            let parent = self.pool.get_mut(&parent_id).expect("uid exists");

            // add the current uid to parent's children
            parent.children.insert(uid);

            // include this parent as an ancestor
            ancestors.insert(parent.uid);

            // collect parent's ancestors
            for ancestor_id in &parent.ancestors {
                ancestors.insert(*ancestor_id);
            }
        }

        // set this tx's ancestors
        let tx = self.pool.get_mut(&uid).expect("uid exists");
        tx.ancestors = ancestors;
        tx.links_set = true;
    }
}

/// Creates a pool of auditable `Entry`s
fn pool_from_entries_with_index(
    entries: HashMap<Txid, bitcoincore_rpc_json::GetMempoolEntryResult>,
    index: &HashMap<Txid, usize>,
) -> HashMap<usize, Entry> {
    entries
        .into_iter()
        .map(|(txid, mempool_entry)| {
            let uid = index.get(&txid).expect("txid exists");
            let parents: HashSet<usize> = mempool_entry
                .depends
                .iter()
                .map(|txid| *index.get(txid).expect("txid exists"))
                .collect();

            let entry = Entry {
                uid: *uid,
                parents,
                ..Default::default()
            };
            (*uid, entry)
        })
        .collect()
}

#[cfg(test)]
mod test {
    use super::*;

    fn entry_with_parent(uid: usize, parent: Option<usize>) -> Entry {
        let parents = if let Some(p) = parent {
            let mut parents = HashSet::new();
            parents.insert(p);
            parents
        } else {
            HashSet::new()
        };
        Entry {
            uid,
            parents,
            ..Default::default()
        }
    }

    fn pool_from(entries: Vec<Entry>) -> HashMap<usize, Entry> {
        entries
            .into_iter()
            .map(|entry| (entry.uid, entry))
            .collect()
    }

    #[test]
    fn test_clusters() {
        let ancestor = 0usize;
        let ancestor1 = 1usize;
        let parent = 2usize;
        let parent1 = 3usize;
        let parent2 = 4usize;
        let child = 5usize;
        let child1 = 6usize;
        let child2 = 7usize;
        let gchild = 8usize;

        /*
             A       A1
             /\      /
            P  P1  P2
           /\      /
          C C1    C2
             \
              G
        */

        let entries = vec![
            entry_with_parent(ancestor, None),
            entry_with_parent(ancestor1, None),
            entry_with_parent(parent, Some(ancestor)),
            entry_with_parent(parent1, Some(ancestor)),
            entry_with_parent(parent2, Some(ancestor1)),
            entry_with_parent(child, Some(parent)),
            entry_with_parent(child1, Some(parent)),
            entry_with_parent(child2, Some(parent2)),
            entry_with_parent(gchild, Some(child1)),
        ];
        let pool = pool_from(entries);
        let mut auditor = Auditor::from(pool);
        for uid in 0..9 {
            auditor.set_links(uid);
        }

        // max depth 3
        let tree_height = auditor.max_descendant_depth();
        assert_eq!(tree_height, 3);
        // ancestor0 is most common with a cluster size 5
        let (size, ancestors) = auditor.most_common_ancestors();
        assert_eq!(size, 5);
        assert_eq!(ancestors, vec![0]);
        // cluster count
        assert_eq!(auditor.cluster_count(), 2);
    }
}
