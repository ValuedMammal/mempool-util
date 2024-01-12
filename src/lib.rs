use std::thread;
use std::time::Duration;

use bitcoin::Amount;
use bitcoin::Txid;

pub use {
    // export globals
    bitcoincore_rpc::{self, bitcoincore_rpc_json, Client, RpcApi},
    error::Result,
    std::collections::{HashMap, HashSet},
    util::{
        compare_ancestor_count, compare_audit_tx, key_index, median_from_sorted, target_feerate,
        try_from_value, Percent,
    },
};

pub mod audittx;
pub mod blockmk;
pub mod cluster;
pub mod error;
pub mod sigops;
pub mod taproot;
pub mod util;

mod macros {
    #[macro_export]
    /// Returns bytes from a hex string. See crate [hex-conservative](https://docs.rs/hex-conservative/)
    macro_rules! hex (
        ($hex:expr) => (
            <Vec<u8> as bitcoin::hashes::hex::FromHex>::from_hex($hex).unwrap()
        )
    );
    #[macro_export]
    /// Creates a bitcoin hash from a literal
    macro_rules! hash (
        ($input:expr) => (
            bitcoin::hashes::Hash::hash($input.as_bytes())
        )
    );
}

/// Block subsidy
pub const SUBSIDY: f64 = 6.25;

/// An approximation of the dust level for a transaction.
/// The library defines dust as 3x the minimum transaction vsize
/// assuming a `SegWit V0` transaction containing 1 input
/// with a single signature that produces 1 output,
/// e.g. (3sat/vbyte * 110vbyte) = 330sat
const DUST_LIMIT: u64 = 330;

/// Type for mapping a [`Txid`] to a corresponding primary key in the audit pool
type TxidIndex = HashMap<Txid, usize>;

/// Trait for types that can be transformed into an [`AuditPool`]
pub trait Audit {
    fn into_pool(self) -> (TxidIndex, blockmk::AuditPool);
}

/// Trait for types capable of linking groups of related mempoool transactions
pub trait Cluster {
    fn set_links(&mut self, uid: usize);
}

/// Computes the number of dust-producing transactions in the given block. Note that
/// the function defines dust as 2x the normal [`DUST_LIMIT`].
pub fn check_dust_pruned(block: &bitcoin::Block) -> Option<(usize, usize)> {
    let mut dust_outputs = 0usize;
    let mut dust_tx_count = 0usize;
    let txs = &block.txdata;
    for tx in txs {
        let mut is_dust = false;
        if tx.is_coinbase() {
            continue;
        }
        for txo in &tx.output {
            let spk = txo.script_pubkey.as_script();
            if txo.value.to_sat() <= 2 * DUST_LIMIT && !spk.is_op_return() {
                is_dust = true;
                dust_outputs += 1;
            }
        }
        if is_dust {
            dust_tx_count += 1;
        }
    }
    if dust_tx_count > 0 {
        return Some((dust_outputs, dust_tx_count));
    }
    None
}

/// Computes the number of dust-producing transactions in the given block along with
/// the fraction of total block weight attributable to dust.
///
/// ## Errors
/// if `get_raw_transaction_info_verbose` returns an error
///
/// ## Note
/// We expand the definition of dust to 2x the normal [`DUST_LIMIT`], so that
/// the result catches tx at or near the threshold. Further, if the value of dust-producing
/// outputs for a tx is at least 50% of the total tx value (less fees), then the total tx weight
/// is counted toward the returned dust ratio.
pub fn check_dust_full(block: &bitcoin::Block, core: &Client) -> Result<(usize, usize, f64)> {
    /* return a tuple
    (
        dust_txo_count,
        tx_count_producing_dust,
        block_dust_ratio,
    )
    */
    let block_wu = block.weight().to_wu();
    let mut dust_wu = 0u64;
    let mut dust_outputs = 0usize;
    let mut dust_tx_count = 0usize;

    for tx in &block.txdata {
        if tx.is_coinbase() {
            continue;
        }
        let tx_wu = tx.weight().to_wu();
        let mut is_dust = false;

        // Get tx value from prevouts
        let mut tx_value = 0u64;
        let tx_info = core.get_raw_transaction_info_verbose(&tx.txid(), None)?;
        for input in &tx_info.vin {
            let prevout = input.prevout.as_ref().expect("input has prevout");
            tx_value += prevout.value.to_sat();
        }

        // Check for dust
        let mut txo_value = 0u64;
        let mut tx_dust_amt = 0u64;
        for txo in &tx.output {
            let amt = txo.value.to_sat();
            txo_value += amt;
            let spk = txo.script_pubkey.as_script();
            if amt <= 2 * DUST_LIMIT && !spk.is_op_return() {
                is_dust = true;
                tx_dust_amt += amt;
                dust_outputs += 1;
            }
        }
        if is_dust {
            // count this tx
            dust_tx_count += 1;
        }

        let implied_fee = tx_value - txo_value;
        if tx_dust_amt + implied_fee >= tx_value / 2 {
            // add this tx weight
            dust_wu += tx_wu;
        }

        // to avoid overwhelming bitcoind's rpc interface, wait briefly between iterations
        thread::sleep(Duration::from_millis(100));
    }

    let dust_ratio = (dust_wu as f64 / block_wu as f64).trunc_three();

    Ok((dust_outputs, dust_tx_count, dust_ratio))
}

/// Scores a newly connected block on its similarity to a given set of txids
pub fn block_audit(block: &bitcoin::Block, projected: &[Txid]) -> f64 {
    // In general, block audit works by polling Core's `getblocktemplate` rpc at a regular
    // interval, say 5 minutes, and storing the result. (Note, we assume that step has
    // already occurred somewhere else). Upon hearing of a newly confirmed block, we compare
    // the set of txids in the block against those projected by the most recent template.
    // We then produce a 'score' that indicates the fraction of block txs that was expected
    // with any deviation from 1.0 indicating the new block contains tx not previously
    // projected to be confirmed in the next block.
    let mut txids: Vec<Txid> = block
        .txdata
        .iter()
        .filter_map(|tx| {
            if !tx.is_coinbase() {
                Some(tx.txid())
            } else {
                None
            }
        })
        .collect();

    let num_actual = txids.len() as f64;

    // to get the number of txs unseen, more precisely 'unexpected', we retain only the txids
    // in the given `block` that are *not* contained in `projected`.
    txids.retain(|tx| !projected.contains(tx));
    let num_unseen = txids.len() as f64;

    // find score
    ((num_actual - num_unseen) / num_actual).trunc_three() * 100.0
}

/// Returns block subsidy from the given `height`
pub fn subsidy(height: u32) -> Amount {
    // see bitcoin/src/validation.cpp#GetBlockSubsidy
    let nhalvings = height / bitcoin::blockdata::constants::SUBSIDY_HALVING_INTERVAL;
    if nhalvings >= 64 {
        return Amount::ZERO;
    }
    let subsidy = 50.0 / 2u32.pow(nhalvings) as f64;
    Amount::from_btc(subsidy).expect("parse Amount")
}

#[derive(Debug)]
pub struct TestMempoolEntry {
    pub uid: usize,
    pub fee: u64,
    pub weight: u64,
    pub parents: HashSet<usize>,
}

impl Audit for Vec<TestMempoolEntry> {
    fn into_pool(self) -> (TxidIndex, blockmk::AuditPool) {
        let index = HashMap::new();

        let pool = self
            .into_iter()
            .map(|entry| {
                let uid = entry.uid;
                let mut audit_tx = audittx::AuditTx {
                    uid,
                    order: u32::try_from(uid).unwrap(),
                    weight: entry.weight,
                    fee: entry.fee,
                    parents: entry.parents,
                    ..Default::default()
                };
                audit_tx.pre_fill();
                (uid, audit_tx)
            })
            .collect();
        (index, pool)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bitcoin::consensus::encode::deserialize;
    use bitcoin::Block;
    use bitcoin::OutPoint;
    use bitcoin::transaction;
    use bitcoin::Transaction;
    use bitcoin::TxIn;

    #[test]
    fn test_block_audit() {
        // testnet block: 00000000000000cd0d597595d1a1700ac48a889e1fe6c53e62a46b521e75f05d
        // $ curl --output ./raw_block.dat https://blockstream.info/testnet/api/block/:hash/raw
        // txs len: 3

        let data = hex!("00002020441f39efccdb87b456dbc0a46cb7b75a7fde865ad0d60d115a00000000000000ce88773ce45bd5cd3f5f2bcfce119ff6a99b0f772817c94df41a035d68b5f85162d422658cec001a0279ab7e03010000000001010000000000000000000000000000000000000000000000000000000000000000ffffffff1a03a49f26012013090909200909200904da240016bd1103000000ffffffff02d0f8120000000000160014820d4a343a44e915c36494995c2899abe37418930000000000000000266a24aa21a9edbdc662966a0f845f7c8ca21f10dbfdad3546e6ed945ba690b217dd04695c42dd012000000000000000000000000000000000000000000000000000000000000000000000000002000000016f1b6f9615c959f01db94242cdd6be6df30c16f10577fc994ddb738a56bdbf55010000006a473044022029f7f30e6b4fd1b99daf77070ff01887f2c233828b2989494252dacc3bb465fd0220419986c34074556188ac33e6607f1e9bbe1c16459413832e4f7d30e9069df59f012102c5faee837f09ec075734fa77b8b895397ff9c93b2987db845e2f5e47000a5226fdffffff028aca1100000000001976a914bf3394503fc358700f2dd3388296c6ff5ab245a488ac6d695c94010000001976a91413cd3dcae193017f36a33152c4b01c2390e74fed88aca29f260002000000000101deb21f0a88775bad7a984650ab36f07dc25492e5241020b381793b5b1470cca10000000000fdffffff023ee2010000000000160014633bf3c375206e357ff31754be6c4a858733571fe803000000000000160014b1cbd2ca1b6eb558dc9210e9bd13600413ab3f2802473044022025e2158ec5dc5cdf5dcf766a583ac9784ce4c37de3b59f9d61933498c8df38190220350d93d1cd07af15e17dcd90cc1cee61948bb3907dbd15b9f75dfd4ea4b4825d012102e1798b4c71209f4ab7f63b348335459313a2b23127e26cf3e8b62082f1897086a29f2600");
        let block: Block = deserialize(&data).unwrap();

        /*  Tx data
            "4bc8f64bdc54bcdc99390c01cbc27bed78693233e54a9eda6cd316d87ed8d18f",
            "8d44470e162965e68ef177be00b8d5f4263f6f7ca5af3643b6a749b074f06e73",
            "09e0b530cb39c33bc7ec93bff68a2f99f974863cf7f75b9cff925fbf7fcd8a67"
        */

        let mut projected: Vec<Txid> = vec![
            "8d44470e162965e68ef177be00b8d5f4263f6f7ca5af3643b6a749b074f06e73",
            "09e0b530cb39c33bc7ec93bff68a2f99f974863cf7f75b9cff925fbf7fcd8a67",
        ]
        .into_iter()
        .map(|s| s.parse().unwrap())
        .collect();

        // score == 100
        let score = block_audit(&block, &projected);
        assert_eq!(score, 100.0);

        // pop 1 from projected
        // score == 50
        projected.pop();
        let score = block_audit(&block, &projected);
        assert_eq!(score, 50.0);
    }

    #[test]
    fn not_a_transaction_test() {
        use bitcoin::{absolute::LockTime, Script, ScriptBuf, Sequence, TxOut, Witness};

        // Create dust
        // Not actually a test, just a nice exercise
        // Note: to validate a tx, we need feature "bitcoinconsensus" from crate bitcoin
        let tx = Transaction {
            version: transaction::Version::TWO,
            lock_time: LockTime::from_consensus(810_000),
            input: vec![
                TxIn {
                    previous_output: OutPoint { txid: "a6350951c1e44c95bd16c2fbdab36cf8292201f8fe4a408b001b4243590ef09f".parse().unwrap(), vout: 1 },
                    script_sig: ScriptBuf::default(),
                    sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
                    witness: Witness::from_slice(
                        &[
                            &hex!("30440220197575179073b1817b9f0ab67eee40def36fafc9c15658165dd163b25b80931602206ab6692fbbb62a769dcb2b069cbbe10efcb76caf7e53256b86386f3b5c7d391701"), 
                            &hex!("03005933581ac2d9c25e0299c4c17bf313bee78f75cfd311f341ec5218fcfd0a8e")
                        ]
                    )
                }
            ],
            output: vec![
                TxOut::minimal_non_dust(
                    Script::from_bytes(&hex!("0014170ef448a233262c316d983f3f76ff9941df5e17")).to_owned()
                )
            ]
        };
        let amt = tx.output[0].value.to_sat();
        //dbg!(amt);
        assert!(amt <= DUST_LIMIT);

        // 1 in 1 out
        // p2sh-p2wpkh -> p2pkh
        //let txid = "bc9384919ad5d08b2c66e31f29e7c63572c398a87631c03a4ce9e94ff1cbe62f";
        let block: u32 = 502936;
        let locktime = LockTime::from_height(block).unwrap();
        let vin = vec![
            TxIn {
                previous_output: "665680369954c1b880d2a5284477b1813202290229c6e52bba5a3c7721122a98:27".parse().unwrap(),                
                // asm OP_PUSHBYTES_22 0014e83d1d02a3844c34995ec3fc1ef0b49bf02936f5
                script_sig: ScriptBuf::from_hex("160014e83d1d02a3844c34995ec3fc1ef0b49bf02936f5").unwrap(),
                sequence: Sequence::MAX,
                witness: Witness::from_slice(
                    &[
                        &hex!("30440220012432fdcc626510bd77815257bdcf761db2608c76281176fc5a2fa4ed60cda402205061c264051b7811891af1bc802d0ef770dbbc1d5f63fd9bb6902e76653125e601"),
                        &hex!("020ebf45fb179f47e9d818219903069dcff3e3b4c51641f69000610a4c07fb5a0d"),
                    ]
                ),
            }
        ];
        let vout = vec![TxOut {
            value: Amount::from_sat(13_220_000),
            script_pubkey: ScriptBuf::from_hex(
                "76a914645e1f9be127080712ae9cd36fb2e65b3121060088ac",
            )
            .unwrap(),
        }];
        let _tx = Transaction {
            version: transaction::Version::ONE,
            lock_time: locktime,
            input: vin,
            output: vout,
        };

        //TODO
        //assert!(tx.is_consensus_valid());
    }

    #[test]
    fn subsidy_from_height() {
        let heights_expected_subsidy = vec![
            (1_u32, 50.0_f64),
            (210_000, 25.0),
            (420_000, 12.5),
            (630_000, 6.25),
            (840_000, 3.125),
        ];

        for case in heights_expected_subsidy {
            let expect = Amount::from_btc(case.1).unwrap();
            assert_eq!(subsidy(case.0), expect);
        }
    }
}
