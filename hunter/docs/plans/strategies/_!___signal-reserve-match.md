# Reserve match

Reserve match names the **signal tx**: the earlier transaction he prices when he builds his buy.

`1.0125` is the pump.fun fee (1.25%). A wallet on another fee uses that fee in place of `1.0125`.

20% **slippage** is `(1 + 0.20)` on a `Buy`. It is `(1 - 0.20)` on a `BuyExactSolIn`.

The window is the 2 slots before his entry. An earlier tx is before him when `(slot, tx_index)` is strictly earlier than his buy.

| Name | Meaning |
| --- | --- |
| `vsol`, `vtok` | Pool reserves on a tx (`reserve_lamports`, `reserve_token`). |
| `curve_sol` | SOL he prices into the pool at **fire**. |
| `token_amount` | Tokens a `Buy` stores. **Frozen**. |
| `max_sol_cost` | Most SOL a `Buy` stores. The bound on what he pays. |
| `spendable_sol_in` | SOL a `BuyExactSolIn` stores. **Frozen**. |
| `min_tokens_out` | Fewest tokens a `BuyExactSolIn` stores. The bound on what he receives. |

`max_sol_cost` is `max_cost_lamports` on `trades.swap_ix`. `spendable_sol_in` is `spendable_lamports_in`, and `min_tokens_out` is `min_tokens_out`, on the same document. The document is the pump buy in the transaction, including a buy another program invoked. It is null on a row written before migration 0024, on a sell, and on an AMM swap. With no document, a buy that has several earlier txs stays unnamed.

The SOL the pool takes at land is `amount_lamports` on the trade. The tokens the fill receives are `token_amount` on the trade.

[Instruction pick](_!___signal-ix-pick.md) is a separate reference. Nothing in the product runs it.

---

## 1. How he sets the values on his tx

He decides the instruction, the buy size, and the **slippage** at **fire**. He prices them against the **signal tx** (`vsol`, `vtok` on that tx). The tx stores those numbers, then he fires.

Other txs can land before his tx. The pool at land is then a different pool. One stored number stays the **fire** number. The other fill number moves, and it has to stay inside the bound he stored.

### · Buy

`Buy` and `BuyV2` store **token_amount** and **max_sol_cost**.

**token_amount** is **frozen**. It is the same number at **fire** and on the **landed** tx.

**curve_sol** is the SOL those tokens cost at the **signal tx**. **max_sol_cost** is computed from that **curve_sol**, the fee, and **slippage**. The SOL the pool takes when the tx **lands** is a different number once other txs have landed between the **signal tx** and his entry. A buy between pushes that landed SOL up, toward **max_sol_cost**. His payment stays under **max_sol_cost**.

At **fire**, against the **signal tx**:

```
+----------------------------------------------------------+
| Buy                                                      |
| curve_sol = 0.530172839 SOL, slippage 20%                |
+----------------------------------------------------------+
        signal tx: vsol, vtok
                      |
                      v
        token_amount = floor( vtok * curve_sol / (vsol + curve_sol) )
                      |
                      v
        max_sol_cost = ceil( curve_sol * 1.0125 * (1 + 0.20) )
        1.0125 is the pump.fun fee
        (1 + 0.20) is slippage
```

`ceil` rounds up to the next lamport. The pool charges that next lamport, and **max_sol_cost** has to cover `curve_sol * 1.0125 * (1 + slippage)` or the buy fails. A fraction of a lamport cannot be stored.

> He decides **curve_sol** = `530,172,839` lamports (`0.530172839` SOL) and **slippage** 20%. He reads the **signal tx**: `vsol = 39,825,996,407`, `vtok = 808,266,030,621,502`.
>
> How many tokens that SOL buys on that pool. This number is **frozen** in the tx:
>
> ```
> vsol + curve_sol = 40,356,169,246
> vtok * curve_sol / (vsol + curve_sol) = 10,618,468,108,549.141
> token_amount = floor( ... ) = 10,618,468,108,549
> ```
>
> `0.141` of a token is not a token. `floor` drops it.
>
> Most SOL he will pay. The fee comes first, then room for the pool to move:
>
> ```
> curve_sol * 1.0125                 = 536,799,999.4875
> 536,799,999.4875 * (1 + 0.20)      = 644,159,999.385
> max_sol_cost = ceil( ... )         = 644,160,000
> ```
>
> `0.385` of a lamport is not enough. `ceil` stores the next lamport, `0.644160000` SOL.
>
> No tx lands between the **signal tx** and his entry. The pool is still that pool, so it takes `530,172,839` lamports for the same **token_amount**. Mint and signatures: [References](#references).

> He decides the same **curve_sol** = `530,172,839` lamports and the same **slippage** 20%, on another mint. The cap arithmetic is the same, so **max_sol_cost** = `644,160,000` again. The pool he reads is a different **signal tx**: `vsol = 43,175,797,262`, `vtok = 745,556,588,736,212`.
>
> ```
> vsol + curve_sol = 43,705,970,101
> vtok * curve_sol / (vsol + curve_sol) = 9,043,932,725,254.599
> token_amount = floor( ... ) = 9,043,932,725,254
> ```
>
> That **token_amount** is **frozen**. One buy then lands, and the pool he actually hits is `vsol = 43,566,379,496`, `vtok = 738,872,508,933,578`. The tx still demands the same **token_amount**, so the pool charges a new SOL:
>
> ```
> vtok - token_amount = 729,828,576,208,324
> vsol * token_amount / (vtok - token_amount) = 539,868,426.764
> SOL the pool takes = ceil( ... ) = 539,868,427
> ```
>
> `9,695,588` lamports above the **curve_sol** he priced. His payment is that SOL times the fee:
>
> ```
> 539,868,427 * 1.0125 = 546,616,782.3375
> ```
>
> `546,616,782` lamports is still under **max_sol_cost** `644,160,000`, so the buy lands. Mint and signatures: [References](#references).

### · BuyExactSolIn

`BuyExactSolIn`, `BuyExactQuoteIn`, and `BuyExactQuoteInV2` store **spendable_sol_in** and **min_tokens_out**.

**spendable_sol_in** is **frozen**, and so is **curve_sol** = `floor( spendable_sol_in / 1.0125 )`. The SOL the pool takes at land is that size on every buy of this spend. **spendable_sol_in** is the same size on every buy of this kind, so it does not name the **signal tx**.

**min_tokens_out** does name it, because the token quote at **fire** depends on that tx's `vsol` and `vtok`. The tokens he receives when the tx **lands** are a different number once other txs have landed between the **signal tx** and his entry. They stay above **min_tokens_out**. A buy between pushes that landed token amount down, toward **min_tokens_out**.

At **fire**, against the **signal tx**:

```
+----------------------------------------------------------+
| BuyExactSolIn                                            |
| spendable_sol_in = 0.500000000 SOL, slippage 20%         |
+----------------------------------------------------------+
        spendable_sol_in = 0.500000000 SOL
                      |
                      v
        curve_sol = floor( spendable_sol_in / 1.0125 )
        1.0125 is the pump.fun fee
                      |
                      v
        tokens = floor( vtok * curve_sol / (vsol + curve_sol) )
                      |
                      v
        min_tokens_out = floor( tokens * (1 - 0.20) )
        (1 - 0.20) is slippage
```

`floor` drops the fraction. A fraction of a lamport is not a lamport. A fraction of a token is not a token.

> He decides **spendable_sol_in** = `500,000,000` lamports (`0.500000000` SOL) and **slippage** 20%. He reads a **signal tx** at `vsol = 30,000,000,000`, `vtok = 1,073,000,000,000,000`.
>
> The fee comes off first. The pool does not receive it:
>
> ```
> spendable_sol_in / 1.0125 = 493,827,160.493827
> curve_sol = floor( ... ) = 493,827,160
> ```
>
> `0.493` of a lamport is not a lamport. `floor` drops it. This **curve_sol** is **frozen** with **spendable_sol_in**.
>
> How many tokens that SOL buys on the **signal tx**:
>
> ```
> vsol + curve_sol = 30,493,827,160
> vtok * curve_sol / (vsol + curve_sol) = 17,376,518,201,528.365
> tokens = floor( ... ) = 17,376,518,201,528
> ```
>
> Fewest tokens he will accept if the pool moves before he lands:
>
> ```
> tokens * (1 - 0.20) = 13,901,214,561,222.4
> min_tokens_out = floor( ... ) = 13,901,214,561,222
> ```
>
> `0.4` of a token is not a token. `floor` drops it. This pool is the arithmetic. The two buys below are real, and their pools are not this 30 SOL pool.

> He decides **spendable_sol_in** = `500,000,000` on two mints, and both txs store **min_tokens_out** = `12,530,789,183,073`. The fee is off first, so both put the same SOL into the pool: `493,827,159` lamports.
>
> On `6XCU…` the **signal tx** is `vsol = 30,987,654,320`, `vtok = 1,038,800,796,845,859`. No tx lands between, so the fill receives `16,294,914,379,839` tokens.
>
> On `9p5M…` he prices a **signal tx** with those same reserves. One buy lands between and leaves `vsol = 31,679,012,344`, `vtok = 1,016,130,163,732,735`. **spendable_sol_in** is still `500,000,000`. The pool takes the same `493,827,159` lamports and pays `15,596,779,105,649` tokens, because `vsol` is higher and `vtok` is lower. That is still above **min_tokens_out**. Mint and signatures: [References](#references).

---

## 2. How slippage is derived

### · Definite entry

A **definite entry** is his buy when both of these are true:

- Exactly one other tx sits in the 2 slots before him.
- No trade hits the mint for the 5 seconds before that tx.

That one tx is the **signal tx**. Read **slippage** from it. Repeat on every **definite entry**. The values that repeat are his set.

`Buy` and `BuyExactSolIn` are read apart. A **slippage** read on a `Buy` is not tried on a `BuyExactSolIn`. On 8dtx the `Buy` set is one value, 20%, read on 133 entries. Another wallet's set can be several values, for example 15% and 25%.

### · Buy

`vsol` and `vtok` are the **signal tx**. **token_amount** and **max_sol_cost** are the **frozen** numbers on his buy.

```
curve_sol = ceil( vsol * token_amount / (vtok - token_amount) )
slippage  = max_sol_cost / (curve_sol * 1.0125) - 1
```

> The 8dtx buy above, once its **signal tx** is known. **token_amount** = `10,618,468,108,549`, **max_sol_cost** = `644,160,000`, `vsol = 39,825,996,407`, `vtok = 808,266,030,621,502`.
>
> SOL those tokens cost on that pool, which is the **curve_sol** he decided:
>
> ```
> vtok - token_amount = 797,647,562,512,953
> vsol * token_amount / (vtok - token_amount) = 530,172,838.99999
> curve_sol = ceil( ... ) = 530,172,839
> ```
>
> **slippage** from the cap he stored:
>
> ```
> curve_sol * 1.0125 = 536,799,999.4875
> 644,160,000 / 536,799,999.4875 - 1 = 0.200000001
> ```
>
> That is his 20% set.

### · BuyExactSolIn

**spendable_sol_in** and **min_tokens_out** are on his buy. `vsol` and `vtok` are the **signal tx**.

```
curve_sol = floor( spendable_sol_in / 1.0125 )
tokens    = floor( vtok * curve_sol / (vsol + curve_sol) )
slippage  = 1 - min_tokens_out / tokens
```

---

## 3. How the signal tx is found

Use this when several earlier txs sit in the 2 slots. **slippage** is already known from **definite entries** (part 2).

The search replays the **frozen** **fire** numbers. On a `Buy` those are **max_sol_cost** and **token_amount**. On a `BuyExactSolIn` those are **min_tokens_out** and **curve_sol**. The SOL the pool takes at land, and the tokens the fill receives, stay out of the test.

One earlier tx matches: that tx is the **signal tx**. Several match: no **signal tx** is named.

### · Buy

`Buy` and `BuyV2`. Each **slippage** in the `Buy` set is tried.

```
+----------------------------------------------------------+
| SIGNAL TX  Buy                                           |
+----------------------------------------------------------+
        curve_sol = floor( max_sol_cost / (1.0125 * (1 + slippage)) )
                      |
                      v
        for each earlier tx:
            its curve_sol = ceil( vsol * token_amount / (vtok - token_amount) )
                      |
                      v
        its curve_sol = curve_sol  ->  that tx is the signal tx
```

**max_sol_cost** and **token_amount** are on his buy. `vsol` and `vtok` are that earlier tx. The **curve_sol** in the test is the **fire** size, recovered from **max_sol_cost**.

> His buy stores **max_sol_cost** = `644,160,000` and **token_amount** = `10,618,468,108,549`. From part 2 his **slippage** is 20%. Two earlier txs sit in the window.
>
> Undo the cap. This is the **curve_sol** he decided at **fire**:
>
> ```
> 644,160,000 / (1.0125 * (1 + 0.20)) = 530,172,839.506
> curve_sol = floor( ... ) = 530,172,839
> ```
>
> He built the cap with `ceil`, so the undo uses `floor`. Rounding `530,172,839.506` to the nearest lamport gives `530,172,840`, one lamport above the SOL he decided.
>
> Each earlier tx: what SOL does this pool charge for the **frozen** **token_amount**?
>
> The **signal tx**, `vsol = 39,825,996,407`, `vtok = 808,266,030,621,502`:
>
> ```
> vsol * token_amount / (vtok - token_amount) = 530,172,838.99999
> ceil( ... ) = 530,172,839
> ```
>
> That equals the **curve_sol** he decided. This tx is the **signal tx**.
>
> The other tx, `vsol = 38,838,342,087`, `vtok = 828,820,137,546,460`:
>
> ```
> vsol * token_amount / (vtok - token_amount) = 504,036,733.539
> ceil( ... ) = 504,036,734
> ```
>
> `504,036,734` is not `530,172,839`, so that tx is not the one he priced.
>
> The pool takes `530,172,839` lamports at land on this buy, and `539,868,427` on the buy in part 1 where one tx lands between. The search uses `530,172,839` either way. Mint and signatures: [References](#references).

### · BuyExactSolIn

`BuyExactSolIn`, `BuyExactQuoteIn`, and `BuyExactQuoteInV2`. Each **slippage** in the `BuyExactSolIn` set is tried. **spendable_sol_in** is the same on every buy, so the test uses **min_tokens_out**.

```
+----------------------------------------------------------+
| SIGNAL TX  BuyExactSolIn                                 |
+----------------------------------------------------------+
        curve_sol = floor( spendable_sol_in / 1.0125 )
                      |
                      v
        for each earlier tx, for each slippage:
            tokens         = floor( vtok * curve_sol / (vsol + curve_sol) )
            min_tokens_out = floor( tokens * (1 - slippage) )
                      |
                      v
        that min_tokens_out = his min_tokens_out  ->  that tx is the signal tx
```

> His buy stores **spendable_sol_in** = `500,000,000` and **min_tokens_out** = `13,901,214,561,222`. From part 2 his **slippage** is 20%. **spendable_sol_in** is the same on every buy of this size, so the test uses **min_tokens_out**.
>
> Fee off first. This **curve_sol** is **frozen**, and it is the same at every candidate:
>
> ```
> 500,000,000 / 1.0125 = 493,827,160.493827
> curve_sol = floor( ... ) = 493,827,160
> ```
>
> Each earlier tx: how few tokens would he have accepted on this pool?
>
> The **signal tx**, `vsol = 30,000,000,000`, `vtok = 1,073,000,000,000,000`. Part 1 already has `tokens = 17,376,518,201,528`.
>
> ```
> tokens * (1 - 0.20) = 13,901,214,561,222.4
> floor( ... ) = 13,901,214,561,222
> ```
>
> That equals the **min_tokens_out** he stored.
>
> Another tx whose pool is `0.1` SOL higher, `vsol = 30,100,000,000`, same `vtok`:
>
> ```
> vtok * curve_sol / (vsol + curve_sol) = 17,319,720,736,763.161
> tokens = floor( ... ) = 17,319,720,736,763
> tokens * (1 - 0.20) = 13,855,776,589,410.4
> floor( ... ) = 13,855,776,589,410
> ```
>
> `13,855,776,589,410` is not the **min_tokens_out** he stored, so that tx is not the one he priced.
>
> On the two real buys in part 1 the stored **min_tokens_out** is `12,530,789,183,073` on both, and the fills receive `16,294,914,379,839` and `15,596,779,105,649` tokens. The search compares **min_tokens_out**. Those two fill sizes stay out of it. Mint and signatures: [References](#references).

### · No signal tx

- **max_sol_cost** = `u64::MAX`, or **min_tokens_out** is `0` or `1`. He set no bound. The **definite entry** yields no **slippage**, and the search above stays blank.
- No earlier tx in the 2 slots.
- **max_sol_cost** or **min_tokens_out** is stored, and no earlier tx equals it.
- Two earlier txs equal it.

---

## References

### · Part 1 and part 3. The 8dtx Buy, pool unchanged

Token `71CNvMdcDkv4rzY83SM72QAkHPLGBHnPYrM4nW48pump`.

| Role | Signature |
| --- | --- |
| His buy. **max_sol_cost** = `644,160,000`. **token_amount** = `10,618,468,108,549`. Pool takes `530,172,839` lamports. | `56To4KyGuoqssqtXGSsv5n68eXzDAFrfSFgAhn2Nc5UHY3hUw31ReBefVREh44TtMnnmBVrF7XtihohW5L8SNWm4` |
| **Signal tx**. **curve_sol** = `0.530172839` SOL. | `2zn4BZqbqoEjwYbLRnaUHdSDyGHbFPBw3JdTE4wrntHCu36hz1SYZyTh61qUswG5uSe7DpCJqE6BG1coBwPwnSuF` |
| Other tx in the same 2 slots. **curve_sol** = `0.504036734` SOL. | `xdJAEFYGC9HrmaARyQK2TXwYQBxbERbamHgiVDuduZk8LeXDyia4tAGraZ8qGqqA6XmJYwUsaruPvdj5bxuNkxQ` |

### · Part 1 and part 3. The 8dtx Buy, one tx between

Token `9iff9a1YyKqRnQsuz6W4jJNvz1da2uaPzBdX9Uaapump`.

| Role | Signature |
| --- | --- |
| His buy. **max_sol_cost** = `644,160,000`. **token_amount** = `9,043,932,725,254`. Pool takes `539,868,427` lamports. | `5otC9vpm5R3mYE1E8umkymax8uEouzx2cZhp84W6nn9BGYfcoAKHtir95koZPA5GJuX3yLQXoMMyvqtPVtcsMGA4` |
| **Signal tx**. **curve_sol** = `530,172,839` lamports for that **token_amount**. | `5VG2GzaVVfWRh1xkdvVycN77dgPuRpbJeVzbgewj4ucAtrRk8hECYH6zkk1xL9HD8GimSEq4JRdSyXPx9JGLF7rV` |
| The buy that lands between. | `Ldc71ZFZy8yQj43bgow3H7jAsmW8z3UoC5wKX1W5tafK5qkRzCTeTTXPFnsPsciNL5Yc5y8pLqNWPDE89yragps` |

### · Part 1 and part 3. `spendable_sol_in = 0.500000000` SOL

**min_tokens_out** in the formula block is the formula at `vsol = 30` SOL and `vtok = 1,073,000,000,000,000`. The two buys below store **min_tokens_out** = `12,530,789,183,073`, and each puts `493,827,159` lamports into the pool.

| Role | Signature |
| --- | --- |
| Lands on the **signal tx**. Fill receives `16,294,914,379,839` tokens. Token `6XCU6mKZEeHZXX811renwuE8sqy3gWCMHYarHjgtpump`. | `1P7uf3oN3HssS1bVS5xSqprmrcRPZdoijBhcAh8ANtvH4cNKztJaC9xE81phpzcjmro3mVZBUF9VGXeKcHH731F` |
| **Signal tx** for that buy. | `3bT4Zmvfcr6w4wgbR3h6Drh2HG4sMzqZnNNVnSHx1RYq97VLpjCaNCAYB1K6XZori9HBnNkWbbbxCrcozsKJEryV` |
| One buy lands between. Fill receives `15,596,779,105,649` tokens. Token `9p5MkYGMiY2ERRD1tXMqGPraSZ2PMPoyPFm1Zu6jpump`. | `2pM8dTHcwR8AmMjfgj3SPXshCeLy1eqvPi7RYRYF33dDMNpVWbbs78aRr6iZj567D6qGM5vpnUMHrbRKfycg3uEm` |
| **Signal tx** for that buy. | `CRo8wJsVmqurTVSgrgdx4zvHNHatmq8SoiomNVYQKsnW38imvAhwZxZM2aWdP7r92k9daHaB9yJmttpVJfmzogD` |
| The buy that lands between. | `5xyhUrtEMU3T746Zdbpdixmgz3Hu32thHuuVVifKn8U6SBzejJHsAYjZFKC1J4zmyY43oUKCV2HV4QZ59emoH66K` |

The earlier `nngvws…` buy is the same **spendable_sol_in** on a deeper pool. Its **min_tokens_out** is `4,738,803,122,455`. The fill receives `5,923,503,890,959` tokens. The pool takes `493,827,159` lamports.

| Role | Signature |
| --- | --- |
| His buy. Token `nngvwsDSqGbGQGTJVs3titwhvjfgVd8zVs6ZFAnpump`. | `326vtZHfkNi7cKq6jCwkavFKqthrk5jUKTYqmJ4JnJDnmadSipg47Kxo4Lu33c4xs7WCADXoJVdUvjraoEY7byXH` |
| Earlier tx in the 2 slots. `vsol = 51.557118973` SOL. | `nmGrpQHAmG5QGjG6EKuer9LnQ7jDuT3v6gycFr9n1vA4jn8nm5HthDUAKfzzwTUsesRg8gEaAdo9m7HJAm58SqF` |
