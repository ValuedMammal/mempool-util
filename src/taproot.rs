use std::str::from_utf8;

use bitcoin::Script;
use bitcoin::ScriptBuf;
use bitcoin::Transaction;
use bitcoin::TxOut;
use lazy_static::lazy_static;
use regex_lite::Regex;

use crate::hex;

lazy_static! {
    /// An ordinal script fragment. The pattern is a slice of the witness data and is distinguished
    /// by the pattern `OP_0 OP_IF OP_PUSHBYTES_3 6f7264`. The trailing fragment is meant to
    /// capture the content-type, prefixed by its length, and terminating at the next `OP_0`.
    static ref RE: Regex = Regex::new(r"^.*OP_0 OP_IF OP_PUSHBYTES_3 6f7264 OP_PUSHBYTES_1 01 OP_PUSHBYTES_([\d]+) ([0-9a-f]+) OP_0")
        .unwrap();
}

/// Count number of taproot outputs in the given block
pub fn tr_txo_count(block: bitcoin::Block) -> usize {
    let txos: Vec<TxOut> = block
        .txdata
        .into_iter()
        .flat_map(|tx| tx.output.into_iter())
        .filter(|txo| txo.script_pubkey.is_p2tr())
        .collect();

    txos.len()
}

/// Whether the given transaction matches the "ord" pattern
#[allow(unused)]
fn is_ord(tx: &Transaction) -> bool {
    for input in &tx.input {
        if input.witness.len() >= 2 {
            let data = &input.witness[1];
            let script = Script::from_bytes(data).to_asm_string();
            if RE.is_match(&script) {
                return true;
            }
        }
    }
    false
}

/// Ord content type
#[allow(dead_code)]
fn ord_content_type(script_asm: &str) -> Option<String> {
    if let Some(cap) = RE.captures(script_asm) {
        let content_type_len: usize = (cap[1]).parse().expect("parse numerical");
        let content_type_hex = &cap[2];
        let data = hex!(content_type_hex);
        if data.len() == content_type_len {
            let content_type = from_utf8(&data).expect("parse utf8");
            return Some(content_type.to_string());
        }
    }
    None
}

/// Returns an iterator over the witness stack elements.
pub fn witness_elements(txin: &bitcoin::TxIn) -> impl Iterator<Item = ScriptBuf> + '_ {
    txin.witness
        .iter()
        .map(|bytes| Script::from_bytes(bytes).to_owned())
}

#[cfg(test)]
mod test {
    use super::*;
    use std::env;

    #[test]
    #[ignore = "depends on local file system"]
    fn test_ord() {
        let cwd = env::var("CARGO_MANIFEST_DIR").unwrap();
        let path_to_rawtx = format!("{cwd}/tests/rawtx1.dat");
        let rawtx = std::fs::read(path_to_rawtx).unwrap();
        let tx: Transaction = bitcoin::consensus::deserialize(&rawtx).unwrap();
        assert!(is_ord(&tx));

        let witness_data = &tx.input[0].witness[1];
        let script_asm = Script::from_bytes(witness_data).to_asm_string();
        let content_type = ord_content_type(&script_asm);
        assert!(content_type.is_some());
        //dbg!(content_type); // image/png
    }

    #[test]
    #[ignore = "depends on local file system"]
    fn captures_from_rawtx_file() {
        // get raw tx from txid
        // txid: e85602c03f9566bab21246e9fa16f0039c887a70d2f2e79147f4770f6ced5ac5
        // curl --output rawtx.dat https://blockstream.info/api/tx/:txid/raw
        /*
            template:
            OP_0 OP_IF OP_PUSHBYTES_3 6f7264 OP_PUSHBYTES_1 01 OP_PUSHBYTES_([\d]+) ([0-9a-f]+) OP_0
        */
        let cwd = env::var("CARGO_MANIFEST_DIR").unwrap();
        let path_to_rawtx = format!("{cwd}/tests/rawtx.dat");
        let rawtx = std::fs::read(path_to_rawtx).unwrap();
        let tx: Transaction = bitcoin::consensus::deserialize(&rawtx).unwrap();
        let witness = &tx.input[0].witness;
        //dbg!(witness.len()); // 3
        let data = &witness[1];
        //dbg!(data.len()); // 15903
        let script = Script::from_bytes(data).to_asm_string();

        assert!(RE.is_match(&script));
        let caps = RE.captures(&script).unwrap();
        let content_type_len: usize = caps[1].parse().unwrap();
        //dbg!(content_type_len); // 10
        let content_type_hex = &caps[2];
        let content_type_data = hex!(content_type_hex);
        if content_type_data.len() == content_type_len {
            let content_type = from_utf8(&content_type_data).unwrap();
            assert_eq!(content_type, "image/webp");
        }
    }

    #[test]
    fn arbitrary_no_checksig() {
        // txid: 14f1de6e994dabe705f980d0ca079d01a41ca7524b4d96e75e57a3a6552d664c
        /*
           OP_PUSHBYTES_1 ac
           OP_PUSHBYTES_1 ac
           OP_EQUALVERIFY
           OP_0
           OP_IF
        */
        let data = hex!("02000000000101dc628dbe1bd077aff4476d42e766a679645fd7012f879c8e9182e878c93d34cf1100000000ffffffff012601000000000000160014a40897ac0756778584e7dbe457cca54abc6daf4c0301024f01ac01ac880063036f726401010a746578742f706c61696e00347b2270223a226272632d3230222c226f70223a226d696e74222c227469636b223a22626e7378222c22616d74223a22313030227d6821c1782891272861d4104f524ac31855e20aa1bdb507ac4a6619c030768496b90e8400000000");
        let tx: Transaction = bitcoin::consensus::deserialize(&data).unwrap();
        //dbg!(tx);
        assert!(is_ord(&tx));
    }

    #[test]
    fn display_witness() {
        let data = hex!("02000000000101dc628dbe1bd077aff4476d42e766a679645fd7012f879c8e9182e878c93d34cf1100000000ffffffff012601000000000000160014a40897ac0756778584e7dbe457cca54abc6daf4c0301024f01ac01ac880063036f726401010a746578742f706c61696e00347b2270223a226272632d3230222c226f70223a226d696e74222c227469636b223a22626e7378222c22616d74223a22313030227d6821c1782891272861d4104f524ac31855e20aa1bdb507ac4a6619c030768496b90e8400000000");
        let tx: Transaction = bitcoin::consensus::deserialize(&data).unwrap();

        let txin = tx.input.first().unwrap();
        for elem in witness_elements(txin) {
            println!("{}", elem);
        }
    }
}
