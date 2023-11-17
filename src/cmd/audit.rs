use super::*;
use crate::cli::AuditSubCmd;
use bitcoin::Amount;
use bitcoin::BlockHash;
use bitcoin::hashes::sha256d;
use bitcoin::Transaction;
use bitcoin::Txid;
use mempool::sigops;
use serde::Serialize;
use std::fs;
use std::path;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

const HOME_DIR: &str = env!("HOME");

/// Format for logging audit block result json
#[derive(Debug, Serialize)]
struct AuditBlockResult {
    /// Block fees in btc
    block_fees: f64,
    /// Block score
    block_score: f64,
    /// Block hash
    hash: String,
}

/// Run block analytics
pub fn execute(core: &Client, subcmd: AuditSubCmd) -> Result<()> {
    match subcmd {
        // Check a block for dust tx
        AuditSubCmd::Dust { block, pruned } => {
            let height = if block.height.is_none() {
                core.get_block_count()?
            } else {
                block.height.unwrap()
            };
            let hash = core.get_block_hash(height)?;
            let block = core.get_block(&hash)?;

            let result = {
                if pruned {
                    if let Some((txo, ct)) = mempool::check_dust_pruned(&block) {
                        format!("{txo} dust outputs created in {ct} tx")
                    } else {
                        String::from("None")
                    }
                } else {
                    let (txo, ct, ratio) = mempool::check_dust_full(&block, core)?;
                    if ct > 0 {
                        format!(
                            "{txo} dust outputs created in {ct} tx\n
                            Dust ratio (by weight): {ratio}",
                        )
                    } else {
                        String::from("None")
                    }
                }
            };

            println!("Height: {height}");
            println!("{} total tx", block.txdata.len());
            println!("{result}");
        },
        // Crunch sigops for a block or tx
        AuditSubCmd::Sigops { block, txid } => {
            let mut sigops = 0_u32;

            // Collect txids
            let txs: Vec<Transaction> = {
                if let Some(s) = txid {
                    let txid: Txid = s.parse().unwrap();
                    let tx = core.get_raw_transaction(&txid, None)?;
                    vec![tx]
                } else if let Some(height) = block.height {
                    let hash = core.get_block_hash(height)?;
                    let block = core.get_block(&hash)?;
                    block.txdata
                } else {
                    // no option specified. get chain tip
                    let height = core.get_block_count()?;
                    let hash = core.get_block_hash(height)?;
                    let block = core.get_block(&hash)?;
                    block.txdata
                }
            };

            // Get raw tx info and crunch sigops
            let pause = txs.len() > 1;
            for tx in txs {
                if tx.is_coin_base() {
                    continue;
                }
                let Ok(tx_info) = core.get_raw_transaction_info_verbose(&tx.txid(), None) else {
                    log::debug!("tx info not available, continuing anyway");
                    continue;
                };
                sigops += sigops::get_sigops_count(&tx_info);
                if pause {
                    thread::sleep(Duration::from_millis(100));
                }
            }

            println!("Sigops cost: {sigops}");
        },
        // Compare the new tip with what was projected
        AuditSubCmd::Block { hash } => {
            // Get projected txids
            // assume file `~/mempool-util/gbt.txt`
            // containing a list of txids from getblocktemplate
            let path = path::PathBuf::from(format!("{HOME_DIR}/mempool-util/gbt.txt"));
            let projected = fs::read_to_string(path)?;

            if projected.as_bytes().len() < 32 {
                // nothing to do
                return Ok(());
            }

            let projected: Vec<Txid> = projected
                .split_whitespace()
                .map(|s| s.parse().expect("parse txid"))
                .collect();

            // Get new block
            let hash_inner = <sha256d::Hash>::from_str(&hash)?;
            let block_hash = BlockHash::from_raw_hash(hash_inner);
            let height = core.get_block_header_info(&block_hash)?.height as u32;
            let block = core.get_block(&block_hash)?;

            // Get block fees
            let coinbase = {
                let tx = block.txdata.first().expect("is some");
                if tx.is_coin_base() {
                    tx
                } else {
                    // coinbase not first in txdata ?
                    let mut cb: Option<&Transaction> = None;
                    for tx in &block.txdata {
                        if tx.is_coin_base() {
                            cb = Some(tx);
                            break;
                        }
                    }
                    cb.expect("coinbase present in txdata")
                }
            };
            let subsidy = subsidy(height);
            let txout_sum: u64 = coinbase.output.iter().map(|txo| txo.value).sum();
            let block_fees: f64 =
                Amount::from_sat(txout_sum.saturating_sub(subsidy.to_sat())).to_btc();

            let block_score = if block.txdata.len() == 1 {
                // not enough tx data
                -1.0
            } else {
                mempool::block_audit(&block, &projected)
            };

            let obj = AuditBlockResult {
                block_fees,
                block_score,
                hash,
            };
            log::info!("{}", serde_json::to_string(&obj)?);
        },
    }

    Ok(())
}

/// Returns block subsidy from the given `height`
fn subsidy(height: u32) -> Amount {
    // see bitcoin/src/validation.cpp#GetBlockSubsidy
    let nhalvings = height / bitcoin::blockdata::constants::SUBSIDY_HALVING_INTERVAL;
    if nhalvings >= 64 {
        return Amount::ZERO;
    }
    let subsidy = 50.0 / 2u32.pow(nhalvings) as f64;
    Amount::from_btc(subsidy).expect("parse Amount")
}

#[test]
fn test_subsidy() {
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
