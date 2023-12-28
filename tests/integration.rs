use std::process::Command;
use std::str::from_utf8;

const PROG: &str = "mempool";
const USER: &str = env!("RPC_USER");
const PASS: &str = env!("RPC_PASS");

#[test]
fn block_generate() {
    let output = Command::new(PROG)
        .env("RPC_USER", USER)
        .env("RPC_PASS", PASS)
        .arg("audit")
        .arg("fee")
        .output()
        .unwrap();

    assert!(!output.stdout.is_empty());
}

#[test]
fn test_subsidy() {
    use bitcoincore_rpc::{Auth, Client, RpcApi};
    // Check the subsidy we have hardcoded matches consensus.
    let client = Client::new(
        "127.0.0.1:8332",
        Auth::UserPass(USER.to_string(), PASS.to_string()),
    )
    .unwrap();

    let height = client.get_block_count().unwrap();
    let computed = mempool::subsidy(height as u32);
    assert_eq!(computed.to_btc(), mempool::SUBSIDY);
}

/** Sigops tests. The following tests use mainnet tx.
 * Note: we need a full node with -txindex in order to query arbitrary txs. */
#[test]
fn sigops_wpkh() {
    // normal segwit spend
    // 1 sigop
    let txid = "ef320af2f16b6e10a87da78bb98761915523a7ca42f0a72e7b133ec36e2be907";
    let output = Command::new(PROG)
        .env("RPC_USER", USER)
        .env("RPC_PASS", PASS)
        .arg("-n")
        .arg("bitcoin")
        .arg("audit")
        .args(["sigops", "--txid", txid])
        .output()
        .unwrap();

    let sigops = 1;
    let exp = format!("Sigops cost: {}\n", sigops);
    let stdout = from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains(&exp));
}

#[test]
fn sigops_pkh() {
    // single sig pkh -> segwit
    // 1 * 4 sigops (non segwit)
    let txid = "d9ec2ac9b0f0f6e2dde0bc2e89b9db51755e9fab0a21b564c9571b8f4ab62a46";
    let output = Command::new(PROG)
        .env("RPC_USER", USER)
        .env("RPC_PASS", PASS)
        .arg("-n")
        .arg("bitcoin")
        .arg("audit")
        .args(["sigops", "--txid", txid])
        .output()
        .unwrap();

    let sigops = 4;
    let exp = format!("Sigops cost: {}\n", sigops);
    let stdout = from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains(&exp));
}

#[test]
fn sigops_wsh() {
    // 205 2/2 wsh inputs
    // expect 410 sigops
    let txid = "5df6c954010e89a6a0e2db9a5fd3cc6211987c07976664e9c746d689af3ad43a";
    let output = Command::new(PROG)
        .env("RPC_USER", USER)
        .env("RPC_PASS", PASS)
        .arg("-n")
        .arg("bitcoin")
        .arg("audit")
        .args(["sigops", "--txid", txid])
        .output()
        .unwrap();

    let sigops = 410;
    let exp = format!("Sigops cost: {}\n", sigops);
    let stdout = from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains(&exp));
}

#[test]
fn sigops_sh() {
    // p2sh 3/4 multi -> segwit txo
    // expect 16 sigops
    let txid = "0b88ae141f9136baf98755001c4d8a17839e7064f9e7deb8ad725ca25c87cc01";
    let output = Command::new(PROG)
        .env("RPC_USER", USER)
        .env("RPC_PASS", PASS)
        .arg("-n")
        .arg("bitcoin")
        .arg("audit")
        .args(["sigops", "--txid", txid])
        .output()
        .unwrap();

    let sigops = 16;
    let exp = format!("Sigops cost: {}\n", sigops);
    let stdout = from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains(&exp));
}

#[test]
fn sigops_wrapped_sh_wpkh() {
    // sh-wpkh 657 inputs
    // expect 657 sigops
    let txid = "f98a3e0f233dfbca1f93ce63cbee163310b8bc4ae36b1482deec95531c772115";
    let output = Command::new(PROG)
        .env("RPC_USER", USER)
        .env("RPC_PASS", PASS)
        .arg("-n")
        .arg("bitcoin")
        .arg("audit")
        .args(["sigops", "--txid", txid])
        .output()
        .unwrap();

    let sigops = 657;
    let exp = format!("Sigops cost: {}\n", sigops);
    let stdout = from_utf8(&output.stdout).unwrap();
    assert!(stdout.contains(&exp));

    //TODO
    // sh-wsh
    // multi
}
