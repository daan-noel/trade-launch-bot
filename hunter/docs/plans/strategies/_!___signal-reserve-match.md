# Reserve match

Names the **signal tx**: the earlier transaction he prices when he builds his buy.

Two pictures. The first is the progress for any new wallet: find the **formula**, recover **`curve_sol`** or a fixed SOL and **`token_amount`**, then match those two against the reserves. The candidates under it are the lines a **definite entry** may keep. The second picture is that progress on 8dtx, a spot `Buy`, and `BuyExactSolIn`. The sheet is the same steps, one column per kept line.

`1.0125` is the pump.fun fee, on top of pool SOL. In lamports the strip is `floor(G * 10000 / 10125)`. SOL amounts are marked ◎. Percents are marked %. Pool SOL is `amount_lamports`. `max_sol_cost` is `max_cost_lamports`. `spendable_sol_in` is `spendable_lamports_in`. Token amounts stay token amounts.

A **definite entry** already has the numbers. The **formula** is the line those entries keep, the one whose number repeats. A **basis** is the input that line starts from. Three bases: the gross SOL **`G`**, **`curve_sol`**, or a fixed SOL **`S`**. The same basis is written onto `Buy` or `BuyExactSolIn`. The instruction chooses the two stored fields. A **crowded entry** runs the kept line. **`vsol`** and **`vtok`** in the match are the reserves before that tx.

---

## Workflow

### Progress

A **definite entry** finds the **formula**. A **crowded entry** recovers **`curve_sol`** or a fixed SOL, and **`token_amount`**, then matches those two.

```
new wallet. each buy, look back 2 slots.
            |
     +------+------+
     |             |
  one tx        several
     |             |
     v             v
 DEFINITE entry           CROWDED entry
 **signal** = that tx     not named yet
 **formula** unknown      a kept **formula** first

definite buys first.
until a **formula** is kept, every crowded buy stays unnamed.

------------------------------------------------------------
1  FIND THE **FORMULA**.  definite buys of this wallet only.
   the signal tx is known.  the formula is not.
------------------------------------------------------------

  known                               unknown
  the one earlier tx                  his **formula**
  it is the **signal**
  **vsol**, **vtok** before that tx

  **Buy**             **token_amount**     **max_sol_cost**
  **BuyExactSolIn**   **spendable_sol_in** **min_tokens_out**

  a **basis** is the input the line starts from.
  gross SOL **G**, **curve_sol**, or a fixed SOL **S**.
  the lines are the candidates below.

  same number on every definite entry     **KEEP**  his line
  a different number each time            **DROP**  next line

------------------------------------------------------------
2  RECOVER THE PAIR.  crowded buys only.
   the kept formula returns these two.
   landed pool SOL and tokens received stay out.
------------------------------------------------------------

  SOL size       **curve_sol**, or a fixed SOL **S**
  token size     **token_amount**

  **Buy**            **token_amount**,    **max_sol_cost**
  **BuyExactSolIn**  **spendable_sol_in**, **min_tokens_out**

------------------------------------------------------------
3  MATCH.  each earlier tx in the 2 slots.
   the pair from step 2 enters this check.
   **vsol**, **vtok** are the reserves before that tx.
------------------------------------------------------------

  **curve_sol** and **token_amount**
    floor( vtok * curve_sol / (vsol + curve_sol) ) = token_amount

  fixed SOL **S** and **token_amount**
    floor( vtok * S / vsol ) = token_amount

  one tx passes the check                         that tx is the **signal**
  two or more pass the check                      unnamed
  earlier txs, none pass                          unnamed
  no earlier tx                                   unnamed
  crowded **Buy**, **max_sol_cost** unlimited     unnamed
  crowded **BuyExactSolIn**, **min_tokens_out**
    is 0 or 1                                     unnamed
  crowded, the size does not repeat              unnamed

  a definite buy stays the signal.
  the one earlier tx needs no formula.
```

### Candidates

A **basis** is the input a line starts from. Three bases. Each is written onto `Buy` or `BuyExactSolIn`. `Buy` stores **`token_amount`** and **`max_sol_cost`**. `BuyExactSolIn` stores **`spendable_sol_in`** and **`min_tokens_out`**. A definite entry keeps the line whose number repeats. The same basis can keep another **slippage**, or another fixed SOL.

```
**Spend**   input the gross SOL **G**.  **curve_sol** is **G** after the fee.
  curve_sol    = floor( G * 10000 / 10125 )
  token_amount = floor( vtok * curve_sol / (vsol + curve_sol) )

  **Buy**              8dtx keeps this.  G = ◎0.5368.  slippage 20%.
    max_sol_cost = G * (1 + slippage)

  **BuyExactSolIn**    worked wallet.  G = ◎0.50.  slippage 20%.
    spendable_sol_in = G
    min_tokens_out   = floor( token_amount * (1 - slippage) )

**Curve**   input **curve_sol**.
  token_amount = floor( vtok * curve_sol / (vsol + curve_sol) )

  **Buy**
    max_sol_cost = ceil( curve_sol * 1.0125 * (1 + slippage) )

  **BuyExactSolIn**
    spendable_sol_in = ceil( curve_sol * 1.0125 * (1 + slippage) )
    min_tokens_out   = floor( token_amount * (1 - slippage) )

**Spot**    input a fixed SOL **S**.
  token_amount = floor( vtok * S / vsol )

  **Buy**              8fSt keeps this.  S = ◎0.30.
    max_sol_cost = ◎0.33

  **BuyExactSolIn**
    min_tokens_out   = floor( token_amount * (1 - slippage) )
    spendable_sol_in is a fixed SOL
```

### Worked buys

```
his buy. look back 2 slots.
            |
     +------+------+
     |             |
  one tx        several
     |             |
     v             v
 DEFINITE entry       CROWDED entry
 that tx is the signal    formula already known
 formula is unknown   run it, then match

------------------------------------------------------------
0  FIND THE FORMULA.  a **definite entry**.
   numbers are on the buy.  the formula is not.
   each trader keeps his own.  another trader, another line.
------------------------------------------------------------

  KNOWN                               UNKNOWN
  the one earlier tx                  his **basis**
  **vsol**, **vtok** before that tx   the number that repeats

  **Buy**             **token_amount**     **max_sol_cost**
  **BuyExactSolIn**   **spendable_sol_in** **min_tokens_out**

  on a definite constant-product fill, pool SOL = **curve_sol**
  on a spot fill, pool SOL stays off **S**

  same number on every definite entry     **KEEP**  his formula
  a different number each time            **DROP**  try the next line

------------------------------------------------------------
1  DEFINITE  8dtx.  basis **Spend**.  on **Buy**.
   the repeating integer is **G**.
   **curve_sol** is **G** after the fee.
------------------------------------------------------------

  READ
    **token_amount**          10,618,468,108,549
    **max_sol_cost**          ◎0.64416
    **pool SOL**              ◎0.530172839   = **curve_sol**

  **CANDIDATE**  his gross. the same **G** on every definite buy.
    **G**            = 536,800,000 lamports   = ◎0.5368
    **slippage**     = 20%
    **max_sol_cost** = G * (1 + slippage)
                     = 536,800,000 * 120 / 100
                     = 644,160,000             = ◎0.64416
    **KEEP** for 8dtx

  **CANDIDATE**  his **curve_sol** and **token_amount**.
    **curve_sol**    = floor( G * 10000 / 10125 )
                     = floor( 536,800,000 * 10000 / 10125 )
                     = 530,172,839             = ◎0.530172839
    **token_amount** = floor( vtok * curve_sol / (vsol + curve_sol) )
    it matches his definite buys.  **KEEP** for 8dtx
    8fSt keeps a different token formula

------------------------------------------------------------
2  CROWDED  8dtx.  run that kept line.
------------------------------------------------------------

  READ
    **token_amount**          9,043,932,725,254    stays
    **max_sol_cost**          ◎0.64416
    **pool SOL**              ◎0.539868427         leave out

  SOLVE
    **G**         = 644,160,000 * 100 / 120
                  = 536,800,000             = ◎0.5368
    **curve_sol** = floor( 536,800,000 * 10000 / 10125 )
                  = 530,172,839             = ◎0.530172839

  MATCH   reserves before the tx
    floor( vtok * curve_sol / (vsol + curve_sol) ) = **token_amount**
    **signal**    pool SOL ◎0.530172839    **KEEP**
    other         pool SOL ◎0.504036734    **DROP**
    one keeper. that tx is the signal

------------------------------------------------------------
3  DEFINITE  BuyExactSolIn.  basis **Spend**.  ◎30 pool.
   **token_amount** = tokens received.
   the **formula** down to **min_tokens_out** is what is found.
------------------------------------------------------------

  READ
    **curve_sol**             ◎0.493827160
                              = floor( 500,000,000 * 10000 / 10125 )
    **token_amount**          17,376,518,201,528   = tokens received
    **min_tokens_out**        13,901,214,561,222

  **CANDIDATE**
    **min_tokens_out** = floor( token_amount * (1 - **slippage**) )
    **slippage** = 1 - 13,901,214,561,222 / 17,376,518,201,528  =  20%
    next definite buy of this wallet gives the same **slippage**
    **KEEP** for him.  another wallet can keep another **slippage**

------------------------------------------------------------
4  CROWDED  same wallet.  run that kept formula backwards.
------------------------------------------------------------

  READ
    **min_tokens_out**        12,530,789,183,073
    tokens received           16,294,914,379,839   leave out
                              15,596,779,105,649   leave out

  SOLVE
    floor( token_amount * (1 - 20%) ) = 12,530,789,183,073
    token_amount = 15,663,486,478,842

  MATCH   reserves before the tx
    floor( vtok * curve_sol / (vsol + curve_sol) ) = token_amount
    one pass. that tx is the signal

------------------------------------------------------------
5  SPOT  8fSt.  basis **Spot**.
   try lines.  keep only what repeats.
------------------------------------------------------------

  TRY   **token_amount** = floor( vtok * curve_sol / (vsol + curve_sol) )
        changes from buy to buy                 **DROP**

  TRY   **token_amount** = floor( vtok * S / vsol )
        every **definite entry**   S = ◎0.30    **KEEP**

  TRY   a cap line that solves S from **max_sol_cost**
        **max_sol_cost** = ◎0.33 on every buy
        it does not yield S                     **DROP**
        pool SOL stays above ◎0.30

  CROWDED   S is ◎0.30
        keep the tx where
          floor( vtok * ◎0.30 / vsol ) = **token_amount**
        pool SOL stays out

------------------------------------------------------------
6  NAME
------------------------------------------------------------

  one tx passes                                   that tx is the **signal**
  two or more pass                                unnamed
  earlier txs, none pass                          unnamed
  no earlier tx                                   unnamed
  crowded **Buy**, **max_sol_cost** unlimited     unnamed
  crowded **BuyExactSolIn**, **min_tokens_out**
    is 0 or 1                                     unnamed
  crowded, the size does not repeat              unnamed

  a definite buy stays the signal.
  the one earlier tx needs no formula.
```

---

## Sheet

| | Spend `Buy`, 8dtx | Spot `Buy`, 8fSt | Spend `BuyExactSolIn` |
| --- | --- | --- | --- |
| **1. Definite entry.** One earlier tx is the **signal**. Both results are known. The **formula** is not. | **`token_amount`** and **`max_sol_cost`** are in the ix. Pool SOL on this fill equals **`curve_sol`**. **`G`** is not in the ix. | **`token_amount`** is in the ix. Pool SOL is above `S` on every fill, so it is not the size. | **`spendable_sol_in`** is **`G`**. **`curve_sol`** = `floor(G * 10000 / 10125)` = pool SOL. Tokens received = **`token_amount`** at the signal. **`min_tokens_out`** is in the ix. |
| **Formula a definite entry finds** | **Spend.** **`G`** = `◎0.5368`. **`slippage`** = 20%. **`max_sol_cost`** = `G * (1 + slippage)` = `◎0.64416`. **`curve_sol`** = `floor(G * 10000 / 10125)` = `◎0.530172839`. **`token_amount`** = `floor(vtok * curve_sol / (vsol + curve_sol))`. The repeating integer is **`G`**. **`curve_sol`** is **`G`** after the fee. | **Spot.** **`token_amount`** = `floor(vtok * S / vsol)`. Every **definite entry** gives `S` = `◎0.30`. **`max_sol_cost`** = `◎0.33`. | **Spend.** **`G`** = `◎0.50`. **`curve_sol`** = `floor(G * 10000 / 10125)`. **`min_tokens_out`** = `floor(token_amount * (1 - slippage))`, and **`slippage`** = `1 - min_tokens_out / token_amount`. This wallet keeps 20%. **Curve** and **Spot** are the other two bases. |
| **2. Crowded entry.** Recover the number from the **signal** moment. | **`token_amount`** stays the ix value. **`G`** = `max_sol_cost * 100 / 120` = `◎0.5368`. **`curve_sol`** = `floor(G * 10000 / 10125)`. Landed pool SOL stays out. | `S` is already `◎0.30`. The cap does not recover it. | **`curve_sol`** is known from the spend. The signal-moment **`token_amount`** is derived from the stored **`min_tokens_out`** by the **formula** row 1 found. Tokens received stay out. |
| **3. Which earlier tx** in the 2 slots. **`vsol`**, **`vtok`** are the reserves before that tx. | `floor(vtok * curve_sol / (vsol + curve_sol))` = **`token_amount`**. The keeper put `◎0.530172839` into the pool. The other tx put `◎0.504036734`. | `floor(vtok * ◎0.30 / vsol)` = **`token_amount`**. Pool SOL stays out. | **`curve_sol`** is known. The **`token_amount`** in the check is the one derived from **`min_tokens_out`** by the found **formula**. Tokens received stay out. `floor(vtok * curve_sol / (vsol + curve_sol))` = **`token_amount`**. |

8dtx, one buy between. **`G`** = `◎0.5368` and **`slippage`** = 20% give **`max_sol_cost`** `◎0.64416` and **`curve_sol`** `◎0.530172839`. The **signal** quoted that **`curve_sol`** for the stored **`token_amount`**. The other tx put `◎0.504036734` into the pool. Landed pool SOL `◎0.539868427` stays out.

`BuyExactSolIn`, the found **formula** run backwards. On the ◎30 pool **`token_amount`** is `17,376,518,201,528` and **`min_tokens_out`** is `13,901,214,561,222`, so those **definite** numbers find **`slippage`** = 20% for that wallet. A **crowded entry** from the same wallet stores **`min_tokens_out`** = `12,530,789,183,073`. The signal-moment **`token_amount`** is what that same found **formula** returns for that stored min: `floor(token_amount * (1 - 20%))` = `12,530,789,183,073` gives **`token_amount`** `15,663,486,478,842`. The fill receives `16,294,914,379,839` or `15,596,779,105,649`. Those stay out. The match uses `15,663,486,478,842` and **`curve_sol`**.

A crowded buy stays unnamed when **`max_sol_cost`** is unlimited, when **`min_tokens_out`** is 0 or 1, when the size does not repeat, when no earlier tx passes, when two or more pass, or when the 2 slots hold no earlier tx. A definite buy stays named. The one earlier tx is the signal.

The pages run the constant-product test. A spot wallet is named on a **definite entry**. A **crowded** spot buy stays unnamed on the pages. [Instruction pick](_!___signal-ix-pick.md) does not run.

---

## References

### · 8dtx Buy, pool unchanged

Token `71CNvMdcDkv4rzY83SM72QAkHPLGBHnPYrM4nW48pump`.

| Role | Signature |
| --- | --- |
| His buy. **G** = `◎0.5368`. **max_sol_cost** = `◎0.64416`. **token_amount** = `10,618,468,108,549`. Pool takes `◎0.530172839`. | `56To4KyGuoqssqtXGSsv5n68eXzDAFrfSFgAhn2Nc5UHY3hUw31ReBefVREh44TtMnnmBVrF7XtihohW5L8SNWm4` |
| **Signal tx**. **curve_sol** = `◎0.530172839`. | `2zn4BZqbqoEjwYbLRnaUHdSDyGHbFPBw3JdTE4wrntHCu36hz1SYZyTh61qUswG5uSe7DpCJqE6BG1coBwPwnSuF` |
| Other tx in the same 2 slots. **curve_sol** = `◎0.504036734`. | `xdJAEFYGC9HrmaARyQK2TXwYQBxbERbamHgiVDuduZk8LeXDyia4tAGraZ8qGqqA6XmJYwUsaruPvdj5bxuNkxQ` |

### · 8dtx Buy, one tx between

Token `9iff9a1YyKqRnQsuz6W4jJNvz1da2uaPzBdX9Uaapump`.

| Role | Signature |
| --- | --- |
| His buy. **G** = `◎0.5368`. **max_sol_cost** = `◎0.64416`. **token_amount** = `9,043,932,725,254`. Pool takes `◎0.539868427`. | `5otC9vpm5R3mYE1E8umkymax8uEouzx2cZhp84W6nn9BGYfcoAKHtir95koZPA5GJuX3yLQXoMMyvqtPVtcsMGA4` |
| **Signal tx**. **curve_sol** = `◎0.530172839` for that **token_amount**. | `5VG2GzaVVfWRh1xkdvVycN77dgPuRpbJeVzbgewj4ucAtrRk8hECYH6zkk1xL9HD8GimSEq4JRdSyXPx9JGLF7rV` |
| The buy that lands between. | `Ldc71ZFZy8yQj43bgow3H7jAsmW8z3UoC5wKX1W5tafK5qkRzCTeTTXPFnsPsciNL5Yc5y8pLqNWPDE89yragps` |

### · BuyExactSolIn, `spendable_sol_in = ◎0.500000000`

The arithmetic pool is `vsol = ◎30`, `vtok = 1,073,000,000,000,000`: `curve_sol` = `◎0.493827160`, tokens = `17,376,518,201,528`, `min_tokens_out` = `13,901,214,561,222`. The two buys below store `min_tokens_out` = `12,530,789,183,073` and each put `◎0.493827159` into the pool.

| Role | Signature |
| --- | --- |
| Lands on the **signal tx**. Fill receives `16,294,914,379,839` tokens. Token `6XCU6mKZEeHZXX811renwuE8sqy3gWCMHYarHjgtpump`. | `1P7uf3oN3HssS1bVS5xSqprmrcRPZdoijBhcAh8ANtvH4cNKztJaC9xE81phpzcjmro3mVZBUF9VGXeKcHH731F` |
| **Signal tx** for that buy. | `3bT4Zmvfcr6w4wgbR3h6Drh2HG4sMzqZnNNVnSHx1RYq97VLpjCaNCAYB1K6XZori9HBnNkWbbbxCrcozsKJEryV` |
| One buy lands between. Fill receives `15,596,779,105,649` tokens. Token `9p5MkYGMiY2ERRD1tXMqGPraSZ2PMPoyPFm1Zu6jpump`. | `2pM8dTHcwR8AmMjfgj3SPXshCeLy1eqvPi7RYRYF33dDMNpVWbbs78aRr6iZj567D6qGM5vpnUMHrbRKfycg3uEm` |
| **Signal tx** for that buy. | `CRo8wJsVmqurTVSgrgdx4zvHNHatmq8SoiomNVYQKsnW38imvAhwZxZM2aWdP7r92k9daHaB9yJmttpVJfmzogD` |
| The buy that lands between. | `5xyhUrtEMU3T746Zdbpdixmgz3Hu32thHuuVVifKn8U6SBzejJHsAYjZFKC1J4zmyY43oUKCV2HV4QZ59emoH66K` |

The earlier `nngvws` buy is the same spend on a deeper pool. Its `min_tokens_out` is `4,738,803,122,455`. The fill receives `5,923,503,890,959` tokens. The pool takes `◎0.493827159`.

| Role | Signature |
| --- | --- |
| His buy. Token `nngvwsDSqGbGQGTJVs3titwhvjfgVd8zVs6ZFAnpump`. | `326vtZHfkNi7cKq6jCwkavFKqthrk5jUKTYqmJ4JnJDnmadSipg47Kxo4Lu33c4xs7WCADXoJVdUvjraoEY7byXH` |
| Earlier tx in the 2 slots. `vsol = ◎51.557118973`. | `nmGrpQHAmG5QGjG6EKuer9LnQ7jDuT3v6gycFr9n1vA4jn8nm5HthDUAKfzzwTUsesRg8gEaAdo9m7HJAm58SqF` |
