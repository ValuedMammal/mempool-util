use serde::Serialize;

use std::fs;
use std::path;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use bitcoin::Amount;
use bitcoin::BlockHash;
use bitcoin::hashes::sha256d;
use bitcoin::Transaction;
use bitcoin::Txid;
use mempool::sigops;
use mempool::SUBSIDY;

use super::*;
use crate::cli::AuditSubCmd;

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
            let block = core.get_block(&block_hash)?;

            // Get block fees
            let coinbase = block
                .txdata
                .iter()
                .find(|tx| tx.is_coin_base())
                .expect("find coinbase");
            let subsidy = Amount::from_btc(SUBSIDY).expect("Amount from subsidy");
            let txout_sum: u64 = coinbase.output.iter().map(|txo| txo.value).sum();
            let block_fees: f64 =
                Amount::from_sat(txout_sum.saturating_sub(subsidy.to_sat())).to_btc();

            let block_score = if block.txdata.len() == 1 {
                // In case we get an empty block (containing only the coinbase), rather than
                // return early, give an invalid score, so we can still produce a log record.
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
