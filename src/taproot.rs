use crate::hex;
use bitcoin::Script;
use bitcoin::Transaction;
use bitcoin::TxOut;
use lazy_static::lazy_static;
use regex_lite::Regex;
use std::str::from_utf8;

lazy_static! {
    /// An ordinal script template. The pattern spans 53 bytes of an element of the witness stack, beginning with
    /// a 32-byte data push. The distinguishing feature of the "ord" pattern is the fragment `OP_0 OP_IF OP_PUSHBYTES_3 6f7264`.
    /// Additionally, a valid match includes a 10-byte push representing the content-type of the inscribed data.
    static ref RE: Regex = Regex::new(r"OP_PUSHBYTES_32 [0-9a-f]{64} OP_CHECKSIG OP_0 OP_IF OP_PUSHBYTES_3 6f7264 OP_PUSHBYTES_1 01 OP_PUSHBYTES_10 ([0-9a-f]{20})")
        .unwrap();
}

/// Count number of taproot outputs in the given block
pub fn tr_txo_count(block: bitcoin::Block) -> usize {
    let txos: Vec<TxOut> = block
        .txdata
        .into_iter()
        .flat_map(|tx| tx.output.into_iter())
        .filter(|txo| txo.script_pubkey.is_v1_p2tr())
        .collect();

    txos.len()
}

/// Count number of transactions matching the "ord" pattern in the given block
pub fn tr_ord_count(block: bitcoin::Block) -> usize {
    let mut txs = block.txdata;
    txs.retain(is_ord);
    txs.len()
}

/// is ord
fn is_ord(tx: &Transaction) -> bool {
    for input in &tx.input {
        if input.witness.len() >= 2 {
            let witness_data = &input.witness[1];
            if witness_data.len() >= 53 {
                let slice = &witness_data[..53];
                let script = Script::from_bytes(slice).to_asm_string();
                if RE.is_match(&script) {
                    return true;
                }
            }
        }
    }
    false
}

/// ord content type
#[allow(unused)]
fn ord_content_type(script_asm: &str) -> Option<String> {
    if let Some(cap) = RE.captures(script_asm) {
        let content_type_hex = &cap[1];
        let content_type_data = hex!(content_type_hex);
        if let Ok(content_type) = from_utf8(&content_type_data) {
            return Some(content_type.to_string());
        }
    }
    None
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::hex;
    use std::env;

    #[test]
    fn test_ord() {
        let cwd = env::var("CARGO_MANIFEST_DIR").unwrap();
        let path_to_rawtx = format!("{cwd}/tests/rawtx.dat");
        let rawtx = std::fs::read(path_to_rawtx).unwrap();
        let tx: Transaction = bitcoin::consensus::deserialize(&rawtx).unwrap();
        assert!(is_ord(&tx));

        let witness_data = &tx.input[0].witness[1];
        let script_asm = Script::from_bytes(witness_data).to_asm_string();
        let content_type = ord_content_type(&script_asm);
        assert!(content_type.is_some());
        //dbg!(content_type.unwrap());
    }

    #[test]
    fn parse_tapscript_from_rawtx_file() {
        // get raw tx from txid
        // txid: e85602c03f9566bab21246e9fa16f0039c887a70d2f2e79147f4770f6ced5ac5
        // curl --output rawtx.dat https://blockstream.info/api/tx/:txid/raw
        /*
            template: 0x20 <32bytes> ac 00 63 03 6f7264 01 01 0a <10bytes>
            OP_PUSHBYTES_32 [0-9a-f]{64} OP_CHECKSIG OP_0 OP_IF OP_PUSHBYTES_3 6f7264 OP_PUSHBYTES_1 01 OP_PUSHBYTES_10 [0-9a-f]{20}
        */
        let cwd = env::var("CARGO_MANIFEST_DIR").unwrap();
        let path_to_rawtx = format!("{cwd}/tests/rawtx.dat");
        let rawtx = std::fs::read(path_to_rawtx).unwrap();
        let tx: Transaction = bitcoin::consensus::deserialize(&rawtx).unwrap();
        let witness = &tx.input[0].witness;
        //dbg!(witness.len()); // 3
        let witness_data = &witness[1];

        assert!(witness_data.len() >= 53);
        let template = &witness_data[..53];
        let script = Script::from_bytes(template).to_asm_string();
        //dbg!(script);

        assert!(RE.is_match(&script));
        let cap = RE.captures(&script).unwrap();
        let content_type_hex = &cap[1];
        let content_type_data = hex!(content_type_hex);
        let content_type = from_utf8(&content_type_data).unwrap();
        assert_eq!(content_type, "image/webp");
    }
}
