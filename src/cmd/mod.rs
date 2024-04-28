use anyhow::Result;
use bitcoincore_rpc::Client;
use bitcoincore_rpc::RpcApi;

pub mod audit;
pub mod fee;
pub mod tr;

/// Get best block hash
pub fn hash(core: &Client) -> Result<()> {
    // useful for testing setup
    let h = core.get_best_block_hash()?;
    log::info!("Starting!");
    println!("{h}");

    Ok(())
}

/// Convert bitcoin script hex to asm string
pub fn parse_script(s: &str) -> Result<()> {
    let script = bitcoin::ScriptBuf::from_hex(s)?;
    println!("{}", script.to_asm_string());
    Ok(())
}
