use std::fmt;

/// Crate errors
#[derive(Debug)]
pub enum Error {
    /// bitcoind RPC error
    CoreRpc(bitcoincore_rpc::Error),
}

impl From<bitcoincore_rpc::Error> for Error {
    fn from(e: bitcoincore_rpc::Error) -> Self {
        Self::CoreRpc(e)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::CoreRpc(e) => e.to_string(),
        };
        f.write_str(&s)
    }
}

impl std::error::Error for Error {}

/// Crate `Result` type
pub type Result<T, E = Error> = core::result::Result<T, E>;
