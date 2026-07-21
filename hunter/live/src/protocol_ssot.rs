//! Cross-crate guard: executor + ingest pump.fun protocol constants must agree.
//!
//! The two crates deliberately stay decoupled (no shared protocol crate), so
//! program IDs / Anchor discriminators are duplicated. This test is the SSOT
//! equality check — if either side drifts, CI fails here.

#[cfg(test)]
mod tests {
    use ingest_laserstream::Protocol;
    use pump_trader::protocol as exec;
    use solana_sdk::pubkey::Pubkey;

    fn pk(s: &str) -> Pubkey {
        s.parse().expect("program id")
    }

    #[test]
    fn executor_and_ingest_program_ids_match() {
        let ingest = Protocol::pump_fun();
        assert_eq!(ingest.programs.pump_fun.base58, exec::PUMP_FUN.to_string());
        assert_eq!(ingest.programs.pump_swap.base58, exec::PUMP_SWAP.to_string());
        assert_eq!(ingest.programs.token.base58, exec::TOKEN.to_string());
        assert_eq!(ingest.programs.token_2022.base58, exec::TOKEN_2022.to_string());
        assert_eq!(
            ingest.programs.associated_token.base58,
            exec::ASSOCIATED_TOKEN_PROGRAM.to_string()
        );
        assert_eq!(ingest.programs.wsol.base58, exec::WSOL_MINT.to_string());
        // Sanity: base58 round-trips to the same Pubkey the executor embeds.
        assert_eq!(pk(&ingest.programs.pump_fun.base58), exec::PUMP_FUN);
    }

    #[test]
    fn executor_and_ingest_buy_sell_create_discs_match() {
        let ingest = Protocol::pump_fun();
        assert_eq!(ingest.discriminators.buy, exec::BUY_DISC);
        assert_eq!(ingest.discriminators.sell, exec::SELL_DISC);
        assert_eq!(ingest.discriminators.buy_exact_sol_in, exec::BUY_EXACT_SOL_IN_DISC);
        assert_eq!(ingest.discriminators.buy_v2, exec::BUY_V2_DISC);
        assert_eq!(
            ingest.discriminators.buy_exact_quote_in_v2,
            exec::BUY_EXACT_QUOTE_IN_V2_DISC
        );
        assert_eq!(ingest.discriminators.create_ix, exec::CREATE_DISC);
        assert_eq!(ingest.discriminators.create_v2_ix, exec::CREATE_V2_DISC);
    }
}
