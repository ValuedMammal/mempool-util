//#![allow(unused)]
#![warn(clippy::all)]
use crate::cli::{Args, Cmd};
use bitcoincore_rpc::{Auth, Client};
use clap::Parser;

mod cli;
mod cmd;

fn main() -> anyhow::Result<()> {
    pretty_env_logger::init_timed();
    let args = Args::parse();

    // By default core is running on local host.
    // TODO: consider allowing set server url
    let mut url = String::from("http://127.0.0.1:");

    // Set the default port for network
    // default signet
    let net = args.network.unwrap_or_default();
    let port = match net.as_str() {
        "bitcoin" => "8332",
        "testnet" => "18332",
        "regtest" => "18443",
        _ => "38332", // signet
    };
    url.push_str(port);

    let cookie = args.rpc_cookie.unwrap_or_default();
    let auth = Auth::CookieFile(cookie.into());
    let core = Client::new(&url, auth)?;

    match args.cmd {
        Cmd::Hash => cmd::hash(&core)?,
        Cmd::Script { hex } => cmd::parse_script(&hex)?,
        Cmd::Fee(cmd) => cmd::fee::execute(&core, cmd)?,
        Cmd::Audit(cmd) => cmd::audit::execute(&core, cmd)?,
        Cmd::Tr(cmd) => cmd::tr::execute(&core, cmd)?,
    }
    Ok(())
}
