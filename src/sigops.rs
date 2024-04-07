use super::*;
use crate::hex;
use bitcoin::ScriptBuf;
use bitcoincore_rpc_json::{GetRawTransactionResult, ScriptPubkeyType};
use lazy_static::lazy_static;
use regex_lite::Regex;

/// Segwit scale factor when comparing tx size
const SEGWIT_SCALAR: u32 = 4;

lazy_static! {
    static ref RE: Regex = Regex::new(r".*OP_(PUSHNUM_)?(\d{1,2}) OP_CHECKMULTISIG.*").unwrap();
}

/// Counts signature operations for a `tx`
pub fn get_sigops_count(tx: &GetRawTransactionResult) -> u32 {
    let mut sigops = 0u32;

    for input in &tx.vin {
        let scriptsig = input.script_sig.as_ref().expect("has scriptsig");
        // get script sig raw sigops
        sigops += script_sigops_count_raw(&scriptsig.asm);

        // get prevout spk type
        let prevout = input.prevout.as_ref().expect("has prevout");
        if let Some(prevout_spk_type) = prevout.script_pub_key.type_ {
            sigops += match prevout_spk_type {
                ScriptPubkeyType::ScriptHash => {
                    let data = &scriptsig.hex;
                    if data[1] == 0x00 && data[2] == 0x14 {
                        // sh-wpkh
                        1
                    } else if data[1] == 0x00 && data[2] == 0x20 {
                        // sh-wsh
                        let script =
                            script_try_from_witness(&input.txinwitness).unwrap_or_default();
                        script_sigops_count(&script.to_asm_string())
                    } else {
                        // legacy p2sh
                        let redeem_script = parse_p2sh_redeem_script(&scriptsig.asm);
                        script_sigops_count(&redeem_script.to_asm_string()) * SEGWIT_SCALAR
                    }
                }
                ScriptPubkeyType::Witness_v0_KeyHash => 1,
                ScriptPubkeyType::Witness_v0_ScriptHash | ScriptPubkeyType::Witness_Unknown => {
                    let script = script_try_from_witness(&input.txinwitness).unwrap_or_default();
                    script_sigops_count(&script.to_asm_string())
                }
                ScriptPubkeyType::Pubkey | ScriptPubkeyType::PubkeyHash => SEGWIT_SCALAR,
                ScriptPubkeyType::MultiSig | ScriptPubkeyType::Nonstandard => {
                    let prevout_spk = &prevout.script_pub_key;
                    script_sigops_count_raw(&prevout_spk.asm)
                }
                // ScriptPubkeyType::Witness_v1_Taproot
                // ScriptPubkeyType::NullData
                _ => 0,
            };
        }
    }

    sigops
}

/// Finds sigops cost from Script (as asm string slice) in p2sh redeem script or witness field
fn script_sigops_count(script: &str) -> u32 {
    let mut sigops = 0_u32;

    // count OP_CHECKMULTISIG
    let matches: Vec<&str> = script.matches("OP_CHECKMULTISIG").collect();
    for _ in 0..matches.len() {
        if let Some(cap) = RE.captures(script) {
            // redeem script or witness
            // +N in OP_N where N is total number of keys in multisig
            let n: u32 = cap[2].to_owned().parse().expect("parse int");
            sigops += if n <= 16 { n } else { 20 };
        } else {
            // number of pubkeys missing ?
            sigops += 20;
        }
    }

    sigops
}

/// Finds sigops cost from Script (as asm string slice) in raw `ScriptSig` and `ScriptPubkey`
fn script_sigops_count_raw(script: &str) -> u32 {
    let mut sigops = 0u32;

    // bare multisig
    let matches: Vec<&str> = script.matches("OP_CHECKMULTISIG").collect();
    for _ in 0..matches.len() {
        sigops += 20 * SEGWIT_SCALAR;
    }

    // count OP_CHECKSIG[VERIFY]
    let matches: Vec<&str> = script.matches("CHECKSIG").collect();
    for _ in 0..matches.len() {
        sigops += SEGWIT_SCALAR;
    }

    sigops
}

/// Returns the last element of the witness field as `ScriptBuf` if it exists, else `None`
fn script_try_from_witness(txin_witness: &Option<Vec<Vec<u8>>>) -> Option<ScriptBuf> {
    // script is the last element of the witness field
    if let Some(witness) = txin_witness.as_ref() {
        if let Some(witness_data) = witness.last() {
            let script = ScriptBuf::from_bytes(witness_data.clone());
            return Some(script);
        }
    }
    None
}

/// Returns the redeem script from the given script (as asm string) slice
fn parse_p2sh_redeem_script(script: &str) -> ScriptBuf {
    // redeem script hex is last element of scriptsig
    let redeem_script_hex = script.split(' ').last().expect("scriptsig last element");
    let data = hex!(redeem_script_hex);
    ScriptBuf::from_bytes(data)
}

#[allow(unused)]
fn regex_match(input: &str) -> bool {
    RE.is_match(input)
}

#[allow(unused)]
fn regex_capture(input: &str) -> Option<u32> {
    if let Some(cap) = RE.captures(input) {
        let s: u32 = cap[2].to_owned().parse().unwrap();
        return Some(s);
    }
    None
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::hex;
    use bitcoin::Script;
    use bitcoin::ScriptBuf;
    use bitcoin::Witness;

    #[test]
    fn test_regex() {
        assert!(regex_match("OP_PUSHNUM_0 OP_CHECKMULTISIG"))
    }

    #[test]
    fn test_capture() {
        let res = regex_capture("OP_16 OP_CHECKMULTISIG");
        assert_eq!(res, Some(16));
    }

    #[test]
    fn test_witness_sigops() {
        // 2-3 multi wsh
        let witness = Witness::from_slice(
            &[
                &hex!("3044022048bcbdf960c3ab3507564790e4c2238200724efa94320644892eb6f5343a0c5b02202d153fc423c89d1cd089c7ddbb1457652571fabced467f19bc0a421d76fad3a701"),
                &hex!("3045022100b1aed88c20a06c97955105e6fbd74cf38cbcad55990f66db1b869f2d0db40ace02201223e47ebcef0fb0eab88025233eeb2f20a797cbd1fe6cb285e1c26d0d2af93d01"),
                &hex!("5221020c1929d70ed907e2a8d20fb4cd356a325367a4f667b2a6b441632773c5cb42e6210349a4cb2b92fa9bb579ee73b5d0cedc6e796d60584a173813960b43d4868976012103f01a75f7d5c2e03226bfec90291cd78643d60adfee8b03e81642b804b2b814d453ae"),
            ]
        );

        let script = Script::from_bytes(witness.last().unwrap()).to_asm_string();
        /*
            OP_PUSHNUM_2
            OP_PUSHBYTES_33 020c1929d70ed907e2a8d20fb4cd356a325367a4f667b2a6b441632773c5cb42e6
            OP_PUSHBYTES_33 0349a4cb2b92fa9bb579ee73b5d0cedc6e796d60584a173813960b43d486897601
            OP_PUSHBYTES_33 03f01a75f7d5c2e03226bfec90291cd78643d60adfee8b03e81642b804b2b814d4
            OP_PUSHNUM_3
            OP_CHECKMULTISIG
        */
        let res = script_sigops_count(&script);
        assert_eq!(res, 3);
    }

    #[test]
    fn test_parse_p2sh_redeem_script() {
        let scriptsig = ScriptBuf::from_hex("00473044022079140c84496ef0b844ac1292780cb93f88ab674dda467ec3e7abf81f1f9302ea02201ab1b057b4fd97759a82331c31e970cdb1ccb569c3520b2da97eb2d8b4e925e80147304402204f5945441105f40d04bb2590f1322f288ef65a0164154103c0ee531f5aba9d1902204ea757c8d2c935fac799012079091c704952f9d76f8cd28496aab4737ebc4cb20147304402202f21d46a43f48a49270a2f5cb95409efa1b4dd15c08af9f2f544b9172257de5c02207889db77047587dc74cc7312dd85605a405e38390313071fabea50f594adc38f014d0b01534104220936c3245597b1513a9a7fe96d96facf1a840ee21432a1b73c2cf42c1810284dd730f21ded9d818b84402863a2b5cd1afe3a3d13719d524482592fb23c88a3410472225d3abc8665cf01f703a270ee65be5421c6a495ce34830061eb0690ec27dfd1194e27b6b0b659418d9f91baec18923078aac18dc19699aae82583561fefe54104a24db5c0e8ed34da1fd3b6f9f797244981b928a8750c8f11f9252041daad7b2d95309074fed791af77dc85abdd8bb2774ed8d53379d28cd49f251b9c08cab7fc4104c64bf6e940708e7e46ccb3d65ea68c4fbfd05c1a4aedd8a1d68eefaa8233f63e24c2a03565497423b4f637f0d468d291237c481eb279260b266ec3b70e521b6854ae")
            .unwrap()
            .to_asm_string();
        let redeem_script = parse_p2sh_redeem_script(&scriptsig).to_asm_string();
        /*
            OP_PUSHNUM_3
            OP_PUSHBYTES_65
            04220936c3245597b1513a9a7fe96d96facf1a840ee21432a1b73c2cf42c1810284dd730f21ded9d818b84402863a2b5cd1afe3a3d13719d524482592fb23c88a3
            OP_PUSHBYTES_65
            0472225d3abc8665cf01f703a270ee65be5421c6a495ce34830061eb0690ec27dfd1194e27b6b0b659418d9f91baec18923078aac18dc19699aae82583561fefe5
            OP_PUSHBYTES_65
            04a24db5c0e8ed34da1fd3b6f9f797244981b928a8750c8f11f9252041daad7b2d95309074fed791af77dc85abdd8bb2774ed8d53379d28cd49f251b9c08cab7fc
            OP_PUSHBYTES_65
            04c64bf6e940708e7e46ccb3d65ea68c4fbfd05c1a4aedd8a1d68eefaa8233f63e24c2a03565497423b4f637f0d468d291237c481eb279260b266ec3b70e521b68
            OP_PUSHNUM_4
            OP_CHECKMULTISIG
        */

        //dbg!(redeem_script);
        assert!(redeem_script.contains("OP_PUSHNUM_3"));
        assert!(redeem_script.contains("OP_PUSHNUM_4"));
        assert!(redeem_script.contains("OP_CHECKMULTISIG"));
    }
}
