use bitcoin::Transaction;
use mempool::hex;
use mempool::taproot;

use super::*;
use crate::cli::TaprootSubCmd;

/// Taproot Stats
pub fn execute(core: &Client, subcmd: TaprootSubCmd) -> Result<()> {
    match subcmd {
        // Count p2tr txos
        TaprootSubCmd::Outputs(block) => {
            let height = if block.height.is_none() {
                core.get_block_count()?
            } else {
                block.height.unwrap()
            };
            let hash = core.get_block_hash(height)?;
            let block = core.get_block(&hash)?;
            println!("{} p2tr outputs", taproot::tr_txo_count(block));
        },
        // Count ord tx
        TaprootSubCmd::Ord(block) => {
            let height = if block.height.is_none() {
                core.get_block_count()?
            } else {
                block.height.unwrap()
            };
            let hash = core.get_block_hash(height)?;
            let block = core.get_block(&hash)?;
            println!(
                "{} tx matching the \"ord\" pattern",
                taproot::tr_ord_count(block)
            );
        },
        // Display witness elements for a txin
        TaprootSubCmd::Witness { transaction, index } => {
            let tx: Transaction = bitcoin::consensus::deserialize(&hex!(transaction.as_str()))?;

            if index >= tx.input.len() {
                anyhow::bail!("index out of range");
            }

            let txin = &tx.input[index];
            for (i, script) in taproot::witness_elements(txin).enumerate() {
                // only get the second element
                if i == 1 {
                    println!("{}", script.to_asm_string());
                }
            }
        },
    }

    Ok(())
}
