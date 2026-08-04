//! Wire-encoding helpers for values JSON cannot carry faithfully.
//!
//! **The rule: a raw on-chain `u64` goes over the wire as a JSON string.** A JSON
//! number is an IEEE-754 double to every JavaScript consumer, so anything above
//! 2^53 is silently rounded on arrival — `max_cost_lamports = 18446744073709551615`
//! (pump.fun's "no slippage cap" ceiling, which real creation instructions carry)
//! reaches the browser as `18446744073709552000`. String is the standard encoding
//! for a `u64` in Solana JSON payloads, and both sides already read either shape
//! (`api::table_eval::field_num`, `hunter_engine::grouping::extract_lamports`, the
//! frontend's `u64Wire` helpers).
//!
//! **This is a wire rule, not a storage rule.** Postgres `jsonb` keeps numbers as
//! arbitrary-precision `numeric`, so `tokens.initial_buy_instruction` loses nothing
//! at rest and `->>` yields identical text for either shape. Ingest keeps writing
//! the number; only the response encoding changes.
//!
//! Applied to the whole family of raw `u64` instruction args (supply, token
//! amounts, lamports ceilings), never to just the one field that overflows today —
//! a half-applied encoding rule is worse than either shape, because then a reader
//! has to know which fields it covers. `cu_limit`/`cu_price` are deliberately
//! excluded: compute units and micro-lamport prices are bounded far below 2^53 and
//! the rule editor reads them as numbers.

/// `serde` adapter for `Option<u64>` → JSON string (`None` → `null`).
///
/// Use as `#[serde(with = "crate::serde_wire::u64_as_string")]`.
pub mod u64_as_string {
    use serde::Serializer;

    pub fn serialize<S: Serializer>(v: &Option<u64>, s: S) -> Result<S::Ok, S::Error> {
        match v {
            Some(x) => s.serialize_str(&x.to_string()),
            None => s.serialize_none(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::Serialize;

    #[derive(Serialize)]
    struct Row {
        #[serde(with = "super::u64_as_string")]
        v: Option<u64>,
    }

    /// The whole point: `u64::MAX` survives the round trip to JSON with every digit
    /// intact. As a JSON number it would serialize fine from Rust but arrive in JS
    /// as `18446744073709552000`.
    #[test]
    fn u64_max_serializes_as_exact_digits() {
        let json = serde_json::to_string(&Row { v: Some(u64::MAX) }).unwrap();
        assert_eq!(json, r#"{"v":"18446744073709551615"}"#);
        // And it parses back to the same integer.
        let back: u64 = "18446744073709551615".parse().unwrap();
        assert_eq!(back, u64::MAX);
        // The premise: as a JSON *number* these land on one double, so a JS consumer
        // cannot tell them apart no matter how carefully Rust serialized them.
        assert_eq!(u64::MAX as f64, (u64::MAX - 1) as f64);
    }

    #[test]
    fn none_stays_null() {
        assert_eq!(serde_json::to_string(&Row { v: None }).unwrap(), r#"{"v":null}"#);
    }
}
