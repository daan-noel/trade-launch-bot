//! Cross-crate SSOT guard: `trading_core` and `pump-trader` each keep their own
//! copy of the pump.fun protocol constants (program IDs, mints, `LAMPORTS_PER_SOL`).
//! The duplication is deliberate — `pump-trader` is a standalone drop-in library that
//! must NOT depend on `trading_core` — but "split on purpose" is not "allowed to
//! drift": the decoder (`trading_core`/ingest) and the executor (`pump-trader`) must
//! agree on every address or a program upgrade applied to one copy silently breaks
//! the other. `live` is the only crate that depends on both, so this equality check
//! lives here. It needs no DB/network, so it runs on a plain `cargo test -p hunter-live`.

use pump_trader::protocol as pt;
use trading_core::config::constants as tc;

#[test]
fn program_ids_and_units_match_across_crates() {
    // Addresses: trading_core stores base58 `&str`; pump-trader stores `const Pubkey`.
    // Compare in base58 string form.
    assert_eq!(tc::PUMP_FUN_PROGRAM_ID, pt::PUMP_FUN.to_string(), "PUMP_FUN program id");
    assert_eq!(tc::PUMP_SWAP_PROGRAM_ID, pt::PUMP_SWAP.to_string(), "PUMP_SWAP program id");
    assert_eq!(tc::WSOL_MINT, pt::WSOL_MINT.to_string(), "WSOL mint");
    assert_eq!(tc::EVENT_AUTHORITY, pt::EVENT_AUTHORITY.to_string(), "event authority");
    assert_eq!(tc::FEE_PROGRAM_ID, pt::FEE_PROGRAM.to_string(), "fee program");
    assert_eq!(tc::TOKEN_PROGRAM_ID, pt::TOKEN.to_string(), "SPL Token program");
    assert_eq!(tc::TOKEN_2022_PROGRAM_ID, pt::TOKEN_2022.to_string(), "Token-2022 program");
    assert_eq!(
        tc::ASSOCIATED_TOKEN_PROGRAM_ID,
        pt::ASSOCIATED_TOKEN_PROGRAM.to_string(),
        "associated-token program"
    );

    // pump-trader also keeps `&str` forms of the two token programs (for its JSON-RPC
    // params / string classifier) — those must equal trading_core's `&str` too.
    assert_eq!(tc::TOKEN_PROGRAM_ID, pt::TOKEN_PROGRAM_ID, "SPL Token program (&str form)");
    assert_eq!(tc::TOKEN_2022_PROGRAM_ID, pt::TOKEN_2022_PROGRAM_ID, "Token-2022 (&str form)");

    // Unit conversion.
    assert_eq!(tc::LAMPORTS_PER_SOL, pt::LAMPORTS_PER_SOL, "LAMPORTS_PER_SOL");
}
