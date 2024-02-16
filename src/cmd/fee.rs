use serde_json::json;
use serde::Serialize;
use std::time;

use mempool::blockmk::{self, BlockSummary, FeeHistogram};
use mempool::cluster;
use mempool::truncate;

use super::*;
use crate::cli::FeeSubCmd;

/// Format for logging fee report result json
#[derive(Debug, Serialize)]
struct FeeReportResult {
    delta: f64,
    smart_fee: f64,
    height: u64,
}

/// Get network fee statistics
pub fn execute(core: &Client, subcmd: FeeSubCmd) -> Result<()> {
    match subcmd {
        // Collect fee data from mempool
        FeeSubCmd::Report { quiet, check } => {
            // get raw mempool
            let height = core.get_block_count()?;
            let next_height = height + 1;
            let raw_mempool = core.get_raw_mempool_verbose()?;
            if raw_mempool.is_empty() {
                println!("mempool empty");
                return Ok(());
            }

            // generate blocks, validate result
            let raw_mempool_count = raw_mempool.len();
            let blocks = blockmk::audit_fees(next_height, raw_mempool);
            if check {
                validate_result(raw_mempool_count, &blocks);
            }

            if quiet {
                // log output only
                if blocks.len() > 1 {
                    let target_fee = blocks[0].fee_cutoff.expect("fee cutoff is some");

                    // get core smart fee (conf target 2)
                    // and find `delta = (fee_cutoff - smart_fee)`
                    let smart_fee_res = core.estimate_smart_fee(2, None)?;
                    if let Some(smart_fee) = smart_fee_res.fee_rate {
                        let mut smart_fee = smart_fee.to_btc(); // btc/kvb
                        smart_fee *= 1e5_f64; // sat/vb
                        let delta = truncate!(target_fee - smart_fee);
                        let res = FeeReportResult {
                            delta,
                            smart_fee,
                            height,
                        };
                        log::info!("{}", serde_json::to_string(&res)?);
                    }
                }
                return Ok(());
            }

            let mut res = serde_json::Map::new();

            for (i, block) in blocks.iter().enumerate() {
                // add block to cli result
                let name = match (i, blocks.len()) {
                    (0, 3 | 2) => "next_block",
                    (1, 3) => "next_next_block",
                    _ => "remaining",
                };
                res.insert(name.to_string(), json!(block));
            }

            if blocks.len() == 1 {
                println!("{}", serde_json::to_string_pretty(&res)?);
            } else {
                // get fee histogram
                let mut histogram = blocks[0].fee_histogram.expect("fee histogram is some");

                // get core smart fee (conf target 2)
                // and find `delta = (fee_cutoff - smart_fee)`
                let target_fee = blocks[0].fee_cutoff.expect("fee cutoff is some");

                let smart_fee_res = core.estimate_smart_fee(2, None)?;
                if let Some(smart_fee) = smart_fee_res.fee_rate {
                    let mut smart_fee = smart_fee.to_btc(); // btc/kvb
                    smart_fee *= 1e5_f64; // sat/vb
                    let delta = truncate!(target_fee - smart_fee);

                    res.insert("core_smart_fee".to_string(), json!(smart_fee));
                    res.insert("auction_delta".to_string(), json!(delta));
                }

                if blocks.len() == 3 {
                    // combine histogram data from blocks 1-2
                    let fee_array = blocks[1].fee_histogram.expect("fee histogram is some");
                    for i in 0..fee_array.len() {
                        let wu = fee_array[i].1;
                        histogram[i].1 += wu;
                    }
                }

                println!("{}", serde_json::to_string_pretty(&res)?);
                draw_histogram(&histogram);
            }
        },
        // Get fee deltas for current mempool
        FeeSubCmd::Delta => {
            // get raw mempool verbose
            let entries = core.get_raw_mempool_verbose()?;
            if entries.is_empty() {
                println!("mempool empty");
                return Ok(());
            }

            let mut hi_delta = 0u64;
            let mut tx: Option<bitcoin::Txid> = None;

            let sum_fee_deltas: u64 = entries
                .into_iter()
                .map(|(txid, entry)| {
                    let delta = (entry.fees.modified - entry.fees.base).to_sat();
                    if delta > hi_delta {
                        hi_delta = delta;
                        tx = Some(txid);
                    }
                    delta
                })
                .sum();

            println!("Aggregate fee delta: {sum_fee_deltas}");
            if let Some(tx) = tx {
                println!("Highest prioritised tx: {tx}");
            }
        },
        FeeSubCmd::Cluster => {
            let raw_mempool = core.get_raw_mempool_verbose()?;
            if raw_mempool.is_empty() {
                println!("mempool empty");
                return Ok(());
            }

            let res = cluster::analyze(raw_mempool);
            println!("{}", serde_json::to_string_pretty(&res)?);
        },
    }
    Ok(())
}

/// Draw histogram
fn draw_histogram(histogram: &FeeHistogram) {
    println!("\nWeighted feerate distribution (sat/vb)");

    let sum_block_wu = histogram.iter().fold(0, |acc, (_, wu)| acc + wu);

    for (bucket, wu) in histogram {
        let freq = truncate!(*wu as f64 / sum_block_wu as f64);
        let max_bar_len = 30.0;
        let normalized_count = (freq * max_bar_len) as u32;
        print!("{bucket:7}|");
        if *wu > 0 {
            for _ in 0..normalized_count {
                print!("::");
            }
            println!("{freq}");
        } else {
            println!();
        }
    }
}

/// Make sure all mempool tx are accounted for with no duplicates
fn validate_result(raw_mempool_count: usize, blocks: &[BlockSummary]) {
    log::debug!("Validating results for {} entries", raw_mempool_count);
    let now = time::Instant::now();

    let mut total_block_tx: Vec<&usize> =
        blocks.iter().flat_map(|block| block.txn.iter()).collect();
    let total_tx_count = total_block_tx.len();
    if total_tx_count < raw_mempool_count {
        log::warn!("missing {} mempool tx!", raw_mempool_count - total_tx_count);
    }

    total_block_tx.sort_unstable();
    total_block_tx.dedup();
    let sorted_len = total_block_tx.len();
    if total_tx_count > sorted_len {
        log::warn!("{} tx double counted!", total_tx_count - sorted_len);
    }
    log::debug!("Checks completed in {}ms", now.elapsed().as_millis());
}
