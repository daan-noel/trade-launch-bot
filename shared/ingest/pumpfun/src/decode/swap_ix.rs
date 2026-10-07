//! Curve swap-instruction args. One parser, called from the create path and from
//! every curve trade. The document it produces is what a host stores; a later
//! sell or AMM variant is another arm here, not another column.

use borsh::BorshDeserialize;

use crate::event::BuyInstructionArgs;
use crate::protocol::Protocol;

#[derive(BorshDeserialize)]
struct BuyArgs {
    token_amount: u64,
    max_sol_cost: u64,
}

#[derive(BorshDeserialize)]
struct BuyExactArgs {
    spendable_sol_in: u64,
    min_tokens_out: u64,
}

/// The pump buy instruction in `data`, or `None` when the bytes are a different
/// instruction. Sell and AMM layouts are not read yet: a host still stores the
/// column, and those arms land here when a reader needs them.
pub(super) fn parse_buy_ix(data: &[u8], p: &Protocol) -> Option<BuyInstructionArgs> {
    if data.len() < 8 {
        return None;
    }
    let d = &p.discriminators;
    let (disc, rest) = data.split_at(8);
    let mut buf = rest;
    if disc == d.buy {
        let a = BuyArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::Buy {
            token_amount: a.token_amount,
            max_sol_cost: a.max_sol_cost,
        });
    }
    if disc == d.buy_v2 {
        let a = BuyArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::BuyV2 {
            token_amount: a.token_amount,
            max_sol_cost: a.max_sol_cost,
        });
    }
    if disc == d.buy_exact_sol_in {
        let a = BuyExactArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::BuyExactSolIn {
            spendable_sol_in: a.spendable_sol_in,
            min_tokens_out: a.min_tokens_out,
        });
    }
    if disc == d.buy_exact_quote_in {
        let a = BuyExactArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::BuyExactQuoteIn {
            spendable_sol_in: a.spendable_sol_in,
            min_tokens_out: a.min_tokens_out,
        });
    }
    if disc == d.buy_exact_quote_in_v2 {
        let a = BuyExactArgs::deserialize(&mut buf).ok()?;
        return Some(BuyInstructionArgs::BuyExactQuoteInV2 {
            spendable_sol_in: a.spendable_sol_in,
            min_tokens_out: a.min_tokens_out,
        });
    }
    None
}

fn is_buy_disc(data: &[u8], p: &Protocol) -> bool {
    if data.len() < 8 {
        return false;
    }
    let disc = &data[..8];
    let d = &p.discriminators;
    disc == d.buy
        || disc == d.buy_v2
        || disc == d.buy_exact_sol_in
        || disc == d.buy_exact_quote_in
        || disc == d.buy_exact_quote_in_v2
}

/// Buy instructions in execution order. A create or a sell is skipped, so it
/// does not shift the pairing. A buy whose args do not parse keeps its slot as
/// `None`, so the next buy is not given this one's successor.
pub(super) fn buy_ixs_in_order<'a>(
    datas: impl IntoIterator<Item = &'a [u8]>,
    p: &Protocol,
) -> Vec<Option<BuyInstructionArgs>> {
    datas
        .into_iter()
        .filter(|d| is_buy_disc(d, p))
        .map(|d| parse_buy_ix(d, p))
        .collect()
}

/// First known buy in `datas`. The create path keeps the dev buy this way.
pub(super) fn first_buy_ix(datas: &[&[u8]], p: &Protocol) -> Option<BuyInstructionArgs> {
    datas.iter().find_map(|d| parse_buy_ix(d, p))
}

/// Hands each buy event the next buy instruction. A sell takes nothing: its
/// layout is not parsed yet, and it must not consume the buy that follows it.
pub(super) struct BuyCursor {
    ix: std::vec::IntoIter<Option<BuyInstructionArgs>>,
}

impl BuyCursor {
    pub(super) fn new(ix: Vec<Option<BuyInstructionArgs>>) -> Self {
        Self { ix: ix.into_iter() }
    }

    pub(super) fn take(&mut self, is_buy: bool) -> Option<BuyInstructionArgs> {
        if is_buy {
            self.ix.next().flatten()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buy_bytes(disc: &[u8; 8], a: u64, b: u64) -> Vec<u8> {
        let mut out = disc.to_vec();
        out.extend_from_slice(&a.to_le_bytes());
        out.extend_from_slice(&b.to_le_bytes());
        out
    }

    #[test]
    fn buys_pair_in_order_and_a_sell_does_not_consume_one() {
        let p = Protocol::pump_fun();
        let d = &p.discriminators;
        let datas = [
            buy_bytes(&d.buy, 100, 720_859_238),
            buy_bytes(&d.sell, 1, 2),
            buy_bytes(&d.buy_v2, 200, 50_000),
            buy_bytes(&d.buy_exact_sol_in, 1_000, 9),
        ];
        let refs: Vec<&[u8]> = datas.iter().map(Vec::as_slice).collect();
        let mut cursor = BuyCursor::new(buy_ixs_in_order(refs, &p));

        match cursor.take(true) {
            Some(BuyInstructionArgs::Buy {
                token_amount,
                max_sol_cost,
            }) => {
                assert_eq!((token_amount, max_sol_cost), (100, 720_859_238));
            }
            other => panic!("first buy, got {other:?}"),
        }
        assert!(cursor.take(false).is_none(), "a sell takes no buy args");
        assert!(matches!(
            cursor.take(true),
            Some(BuyInstructionArgs::BuyV2 {
                token_amount: 200,
                max_sol_cost: 50_000
            })
        ));
        assert!(matches!(
            cursor.take(true),
            Some(BuyInstructionArgs::BuyExactSolIn {
                spendable_sol_in: 1_000,
                min_tokens_out: 9
            })
        ));
        assert!(cursor.take(true).is_none());
    }

    #[test]
    fn a_truncated_buy_keeps_its_slot() {
        let p = Protocol::pump_fun();
        let d = &p.discriminators;
        let good = buy_bytes(&d.buy, 1, 2);
        let mut short = d.buy.to_vec();
        short.extend_from_slice(&5u64.to_le_bytes());
        let next = buy_bytes(&d.buy_v2, 3, 4);
        let datas = [good.as_slice(), short.as_slice(), next.as_slice()];
        let mut cursor = BuyCursor::new(buy_ixs_in_order(datas, &p));
        assert!(matches!(
            cursor.take(true),
            Some(BuyInstructionArgs::Buy { .. })
        ));
        assert!(
            cursor.take(true).is_none(),
            "the truncated buy occupies a slot"
        );
        assert!(matches!(
            cursor.take(true),
            Some(BuyInstructionArgs::BuyV2 {
                token_amount: 3,
                max_sol_cost: 4
            })
        ));
    }
}
