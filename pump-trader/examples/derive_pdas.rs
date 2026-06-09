//! Identify the cashback tail accounts (idx23 = cashback WSOL acct, idx24 =
//! constant) of a real Token-2022/cashback pump_amm buy by deriving candidates
//! and matching against the on-chain values. Read-only; no funds move.
//!
//!   cargo run -p pump-trader --example derive_pdas

use solana_sdk::pubkey::Pubkey;
use spl_associated_token_account::get_associated_token_address_with_program_id;
use std::str::FromStr;

const PUMP_SWAP: &str = "pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA";
const FEE_PROGRAM: &str = "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ";
const WSOL: &str = "So11111111111111111111111111111111111111112";
const TOKEN_PROG: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

fn main() {
    let swap = Pubkey::from_str(PUMP_SWAP).unwrap();
    let pfee = Pubkey::from_str(FEE_PROGRAM).unwrap();
    let wsol = Pubkey::from_str(WSOL).unwrap();
    let tok = Pubkey::from_str(TOKEN_PROG).unwrap();

    // From a real STORY buy: user, its idx20 (user_volume_accumulator), idx23
    // (cashback acct), idx24 (constant), idx9 protocol_fee_recipient.
    let user = Pubkey::from_str("Ar2Y6o1QmrRAskjii1cRfijeKugHH13ycxW5cd7rro1x").unwrap();
    let idx20 = "3JXY2VkGLhMAPCDVuvuc6Nh4QgbVsdbh4mvwuyHojRGX";
    let idx23 = "cUb26fdQYDB2mMLv2gTdLRxnQ9jq89CEg9VsnHhFPPY";
    let idx24 = "5817UmPM7KLKu2mSQVsXMJ7X2rr3PtEb3j4EioyoJgd1";

    let m = |got: Pubkey, want: &str| if got.to_string() == want { " <== MATCH" } else { "" };

    // idx20 = user_volume_accumulator PDA
    let uva = Pubkey::find_program_address(
        &[b"user_volume_accumulator", user.as_ref()],
        &swap,
    )
    .0;
    println!("user_volume_accumulator : {uva}{}", m(uva, idx20));

    // idx23 candidate = ATA(user_volume_accumulator, WSOL)
    let cb = get_associated_token_address_with_program_id(&uva, &wsol, &tok);
    println!("ATA(uva, WSOL)          : {cb}{}", m(cb, idx23));

    // idx24 candidates — constant, no on-chain account, not in global config.
    println!("\nidx24 target = {idx24}");
    let gva = Pubkey::find_program_address(&[b"global_volume_accumulator"], &swap).0;
    let cands: Vec<(String, Pubkey)> = vec![
        ("ATA(gva, WSOL)".into(), get_associated_token_address_with_program_id(&gva, &wsol, &tok)),
        ("PDA[global_volume_accumulator]".into(), gva),
        ("PDA[fee_vault](swap)".into(), Pubkey::find_program_address(&[b"fee_vault"], &swap).0),
        ("PDA[cashback](swap)".into(), Pubkey::find_program_address(&[b"cashback"], &swap).0),
        ("PDA[global_cashback](swap)".into(), Pubkey::find_program_address(&[b"global_cashback"], &swap).0),
        ("PDA[global](pfee)".into(), Pubkey::find_program_address(&[b"global"], &pfee).0),
        ("PDA[global_config](pfee)".into(), Pubkey::find_program_address(&[b"global_config"], &pfee).0),
        ("PDA[__event_authority](pfee)".into(), Pubkey::find_program_address(&[b"__event_authority"], &pfee).0),
        ("PDA[fee](pfee)".into(), Pubkey::find_program_address(&[b"fee"], &pfee).0),
        ("PDA[fee_vault](pfee)".into(), Pubkey::find_program_address(&[b"fee_vault"], &pfee).0),
        ("PDA[cashback](pfee)".into(), Pubkey::find_program_address(&[b"cashback"], &pfee).0),
        ("PDA[buyback](swap)".into(), Pubkey::find_program_address(&[b"buyback"], &swap).0),
        ("PDA[buyback](pfee)".into(), Pubkey::find_program_address(&[b"buyback"], &pfee).0),
        ("ATA(uva, WSOL, 2022prog?)->skip".into(), uva),
    ];
    for (name, pk) in cands {
        println!("  {:<32}: {pk}{}", name, m(pk, idx24));
    }

    // On-chain Anchor IDL account address for the deployed pump_amm program:
    //   base = find_program_address([], program); idl = create_with_seed(base, "anchor:idl", program)
    let (base, _) = Pubkey::find_program_address(&[], &swap);
    let idl = Pubkey::create_with_seed(&base, "anchor:idl", &swap).unwrap();
    println!("\nanchor IDL account for pump_amm: {idl}");

    let (pbase, _) = Pubkey::find_program_address(&[], &pfee);
    let pidl = Pubkey::create_with_seed(&pbase, "anchor:idl", &pfee).unwrap();
    println!("anchor IDL account for pfee:     {pidl}");

    // Candidate pfee PDAs that idx24 (constant, no account) might be.
    println!("\nmore idx24 candidates (pfee/global seeds):");
    for seed in [
        "fee_config", "config", "state", "global_state", "fee_state", "pfee",
        "fee_recipient", "authority", "fee_authority", "treasury", "vault",
    ] {
        let pk = Pubkey::find_program_address(&[seed.as_bytes()], &pfee).0;
        println!("  pfee[{seed}] : {pk}{}", m(pk, idx24));
    }
}
