use clap::{Parser, Subcommand};

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
#[clap(propagate_version = true)]
pub struct Args {
    /// Network (bitcoin, testnet, signet, regtest) [default: signet]
    #[clap(long, short = 'n')]
    pub network: Option<String>,
    /// Bitcoin Core RPC user
    #[clap(long, env = "RPC_USER")]
    pub rpc_user: Option<String>,
    /// Bitcoin Core RPC password [env: RPC_PASS]
    #[clap(long)]
    pub rpc_pass: Option<String>,
    #[clap(subcommand)]
    pub cmd: Cmd,
}

#[derive(Subcommand, Clone)]
pub enum Cmd {
    /// Get best block hash of the current chain
    Hash,
    /// Get network fee statistics
    #[clap(subcommand)]
    Fee(FeeSubCmd),
    /// Run tests on a confirmed block
    #[clap(subcommand)]
    Audit(AuditSubCmd),
    /// Stats on Taproot usage
    #[clap(subcommand)]
    Tr(TaprootSubCmd),
}

#[derive(Subcommand, Clone)]
pub enum FeeSubCmd {
    /// A snapshot of the current fee environment
    Report {
        /// Don't print to stdout
        #[clap(long, short = 'q')]
        quiet: bool,
    },
    /// Tx prioritisation deltas
    Delta,
    /// Mempool cluster analysis
    Cluster,
}

#[derive(Subcommand, Clone)]
pub enum AuditSubCmd {
    /// The dust profile of a given block. Use -n if running in pruned mode
    Dust {
        /// Block height
        #[clap(flatten)]
        block: Block,
        /// Denotes the chain backend is non-archival
        #[clap(long, short = 'n')]
        pruned: bool,
    },
    /// Quick and dirty sigops counter. For optimal results, bitcoind should set -txindex=1
    Sigops {
        /// Block height
        #[clap(flatten)]
        block: Block,
        /// Txid
        #[clap(long, short = 't')]
        txid: Option<String>,
    },
    /// Scores the difference between the last confirmed block and what was projected
    Block {
        /// Block hash
        #[clap(required(true))]
        hash: String,
    },
}

#[derive(Subcommand, Clone)]
pub enum TaprootSubCmd {
    /// Count the number of p2tr outputs
    Outputs(Block),
    /// Scan a block for the "ord" pattern
    Ord(Block),
    /// Display the witness elements for an input
    Witness {
        /// Transaction hex
        #[clap(required(true))]
        transaction: String,
        /// vin index
        #[clap(required(true))]
        index: usize,
    },
}

/// A required block height
#[derive(Parser, Clone)]
pub struct Block {
    /// Block height
    #[clap(long, short = 'b')]
    pub height: Option<u64>,
}
