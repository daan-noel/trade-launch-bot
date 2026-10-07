//! The curve swap instruction, as the JSON document `trades.swap_ix` and
//! `tokens.initial_buy_instruction` both store. One shape: a `type` plus that
//! variant's fields. A later sell or AMM arm adds a `type` here.

use ingest_pumpfun::event::BuyInstructionArgs;
use serde_json::{json, Value};

pub(crate) fn buy_ix_json(args: &BuyInstructionArgs) -> Value {
    match args {
        BuyInstructionArgs::Buy {
            token_amount,
            max_sol_cost,
        } => json!({
            "type": "Buy",
            "token_amount": token_amount,
            "max_cost_lamports": max_sol_cost,
        }),
        BuyInstructionArgs::BuyV2 {
            token_amount,
            max_sol_cost,
        } => json!({
            "type": "BuyV2",
            "token_amount": token_amount,
            "max_cost_lamports": max_sol_cost,
        }),
        BuyInstructionArgs::BuyExactSolIn {
            spendable_sol_in,
            min_tokens_out,
        } => json!({
            "type": "BuyExactSolIn",
            "spendable_lamports_in": spendable_sol_in,
            "min_tokens_out": min_tokens_out,
        }),
        BuyInstructionArgs::BuyExactQuoteIn {
            spendable_sol_in,
            min_tokens_out,
        } => json!({
            "type": "BuyExactQuoteIn",
            "spendable_lamports_in": spendable_sol_in,
            "min_tokens_out": min_tokens_out,
        }),
        BuyInstructionArgs::BuyExactQuoteInV2 {
            spendable_sol_in,
            min_tokens_out,
        } => json!({
            "type": "BuyExactQuoteInV2",
            "spendable_lamports_in": spendable_sol_in,
            "min_tokens_out": min_tokens_out,
        }),
    }
}
