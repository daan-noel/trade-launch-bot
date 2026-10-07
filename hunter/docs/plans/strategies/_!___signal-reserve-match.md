# Reserve match

His buy freezes two numbers at the moment he decides. He computes the second from one earlier print, the signal.

`Buy` and `BuyV2` freeze `token_amount` and `max_sol_cost`. `BuyExactSolIn`, `BuyExactQuoteIn`, and `BuyExactQuoteInV2` freeze `spendable_sol_in` and `min_tokens_out`. The first number is the order. The second names the print. `BuyV2` uses the ceiling formulas. The three exact-quote instructions use the floor formulas.

Reserve match names the transaction: the print he priced. The pages run it. Its window is the 2 slots before his entry. A print is before him when `(slot, tx_index)` is strictly earlier than his buy.

[Instruction pick](_!___signal-ix-pick.md) is a separate reference. Nothing in the product runs it.

## Formulas

`vsol` and `vtok` are the virtual reserves a print left behind (`reserve_lamports`, `reserve_token`). His filled amounts can differ from the numbers he set when other prints land before him. The frozen arguments stay the ones he set at the signal. A wallet on another fee uses that fee in place of 125 bps: `1.0125` on a ceiling, and `10000 / (10000 + fee_bps)` on a floor.

`max_sol_cost` is `max_cost_lamports` on `trades.swap_ix`. `spendable_sol_in` is `spendable_lamports_in`, and `min_tokens_out` is `min_tokens_out`, on the same document. The document is the pump buy in the transaction, including a buy another program invoked. It is null on a row written before migration 0024, on a sell, and on an AMM swap. A crowded reserve match stays blank on those rows.

A ceiling of `u64::MAX`, or a floor of `0` or `1`, names no print. He set no bound. 1A yields no slippage and 1B stays blank.

Slippage on a ceiling marks the SOL up by `(1 + slippage)`. Slippage on a floor marks the tokens down by `(1 - slippage)`. Each family keeps its own set. A value read on a ceiling entry is not tried on a floor entry.

### Ceiling

Three formulas. Nothing else. `Buy` and `BuyV2`.

```
POOL      vsol * vtok = (vsol + curve_sol) * (vtok - token_amount)

QUOTE     curve_sol = vsol * token_amount / (vtok - token_amount)

CEILING   max_sol_cost = curve_sol * 1.0125 * (1 + slippage)
```

| Formula | In words |
| --- | --- |
| POOL | Virtual SOL times virtual tokens stays the same across his buy. |
| QUOTE | The SOL that his `token_amount` adds to the pool. This is the buy size he decided. |
| CEILING | That SOL, times the 1.25% fee (`1.0125`), times one slippage from his set. 20% is `1.20`, so that setting's ceiling is `curve_sol * 1.215`. |

### Floor

Three formulas. Nothing else. `BuyExactSolIn`, `BuyExactQuoteIn`, and `BuyExactQuoteInV2`.

```
NET       curve_sol = spendable_sol_in * 10000 / 10125

TOKENS    tokens = vtok * curve_sol / (vsol + curve_sol)

FLOOR     min_tokens_out = tokens * (1 - slippage)
```

`NET` and `TOKENS` are integer division. The remainder is dropped.

| Formula | In words |
| --- | --- |
| NET | The SOL that reaches the curve. The 1.25% fee sits inside `spendable_sol_in`. |
| TOKENS | The tokens that SOL buys at that print. |
| FLOOR | Those tokens, times one slippage from his set, as a haircut. 20% keeps `0.80` of the tokens. |

`spendable_sol_in` is his order. It is often one round size on every entry, so it does not name the print. `min_tokens_out` does.

## The match

On a ceiling buy, his ceiling equals that print's quote. On a floor buy, his floor equals that print's tokens after the haircut.

### 1A. Definite entry: read his slippage set

One other buy sits in the 2 slots before him, and that buy is the print immediately before his entry. Each such entry yields one slippage. A wallet can use more than one. Ceiling entries and floor entries are read apart.

```
+--------------------------------------------------+
| 1A  DEFINITE, CEILING                            |
| one other buy, immediately before him            |
+--------------------------------------------------+
        that buy's vsol, vtok
        his token_amount, his max_sol_cost
                      |
                      v
        curve_sol  =  QUOTE
                      |
                      v
        slippage = max_sol_cost / (curve_sol * 1.0125) - 1
                      |
                      v
        repeat on every definite ceiling entry
                      |
                      v
        the values that repeat are his ceiling set
```

```
+--------------------------------------------------+
| 1A  DEFINITE, FLOOR                              |
| one other buy, immediately before him            |
+--------------------------------------------------+
        that buy's vsol, vtok
        his spendable_sol_in, his min_tokens_out
                      |
                      v
        curve_sol  =  NET
        tokens     =  TOKENS
                      |
                      v
        slippage = 1 - min_tokens_out / tokens
                      |
                      v
        repeat on every definite floor entry
                      |
                      v
        the values that repeat are his floor set
```

Trust a reading when that prior buy also follows at least 5 seconds with no trade on the mint. Round a ceiling reading to the setting it hits within 1 lamport. Round a floor reading to the setting it hits within 1 raw token. On 8dtx the ceiling set is one value, 20%, read on 133 entries. Another wallet's set can be several values, for example 15% and 25%. 1B tries every value in that family's set.

Worked ceiling entry, mint `ie8FuxvKNFcHwpBxHKCSUpsNCLgacLPDZEYsvubynCQ`. Quiet for 305 seconds, one other wallet buys, his buy lands in that same slot.

```
QUOTE      curve_sol    = 0.593299784 SOL
fee        0.593299784 * 1.0125 = 0.600716031 SOL
CEILING    0.600716031 * 1.20   = 0.720859238 SOL
```

### 1B. Crowded entry: find the signal

Several prints sit in the 2 slots before him. His set for this family is already known from 1A. Each value in the set is a separate ceiling, or a separate haircut.

```
+--------------------------------------------------+
| 1B  CROWDED, CEILING                             |
| several prints in the 2 slots before him         |
+--------------------------------------------------+
        his max_sol_cost, his token_amount
        each slippage in the ceiling set from 1A
                      |
                      v
        for each slippage:
            curve_sol = max_sol_cost
                        / (1.0125 * (1 + slippage))
                      |
                      v
        for each print before him:
            quote = QUOTE, using that print's vsol, vtok
                      |
                      v
        a print matches when its quote equals
        one of those curve_sol values
        (within 1 lamport)
                      |
                      v
        one match  -> that print is the signal
        many matches -> no print is named
```

```
+--------------------------------------------------+
| 1B  CROWDED, FLOOR                               |
| several prints in the 2 slots before him         |
+--------------------------------------------------+
        his spendable_sol_in, his min_tokens_out
        each slippage in the floor set from 1A
                      |
                      v
        curve_sol = NET, from his spendable_sol_in
                      |
                      v
        for each print before him, for each slippage:
            tokens = TOKENS, using that print's vsol, vtok
            floor  = tokens * (1 - slippage)
                      |
                      v
        a print matches when its floor equals
        his min_tokens_out
        (within 1 raw token)
                      |
                      v
        one match  -> that print is the signal
        many matches -> no print is named
```

On the worked ceiling entry the ceiling `0.720859238` is already on his `Buy`, and his set is `{20%}`. Divide by `1.215` and the buy size he decided is `0.593299784` SOL. The signal is the print whose reserves turn his `token_amount` into that SOL. With a set of several values, the same ceiling is divided once per value, and each result is tested the same way. A floor entry tests each haircut the same way against `min_tokens_out`.
