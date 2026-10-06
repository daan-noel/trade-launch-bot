# Finding the signal transaction

His buy freezes `token_amount` and `max_sol_cost` at the moment he decides. He computed both from one earlier print, the signal.

```
+-------------------------------+-------------------------------+
| PART 1                        | PART 2                        |
| Reserve match                 | Instruction pick              |
| names the TRANSACTION         | names the INSTRUCTION SHAPE   |
|                               |                               |
| 1A  one buy before him        | drop racers                   |
|     -> read his slippage set  | drop a shape that returns     |
| 1B  many prints before him    | after him                     |
|     -> find the signal        | take the closest print        |
+-------------------------------+-------------------------------+
```

The window for both parts is the 2 slots before his entry, and the 1 slot after it. A print is before him when `(slot, tx_index)` is strictly earlier than his buy.

## Formulas

Three formulas. Nothing else.

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

`vsol` and `vtok` are the virtual reserves a print left behind (`reserve_lamports`, `reserve_token`). His filled SOL can differ from `curve_sol` when other prints land before him. `token_amount` and `max_sol_cost` stay the numbers he set at the signal. A wallet on another fee uses that fee's multiplier in place of `1.0125`.

## Part 1. Reserve match

Names the transaction. His ceiling has to equal that print's quote.

### 1A. Definite entry: read his slippage set

One other buy sits in the 2 slots before him, and that buy is the print immediately before his entry. Each such entry yields one slippage. A wallet can use more than one.

```
+--------------------------------------------------+
| 1A  DEFINITE                                     |
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
        repeat on every definite entry
                      |
                      v
        the values that repeat are his slippage set
        one value, or several
```

Trust a reading when that prior buy also follows at least 5 seconds with no trade on the mint. Round each reading to the setting it hits within 1 lamport. On 8dtx the set is one value, 20%, read on 133 entries. Another wallet's set can be several values, for example 15% and 25%. 1B tries every value in the set.

Worked entry, mint `ie8FuxvKNFcHwpBxHKCSUpsNCLgacLPDZEYsvubynCQ`. Quiet for 305 seconds, one other wallet buys, his buy lands in that same slot.

```
QUOTE      curve_sol    = 0.593299784 SOL
fee        0.593299784 * 1.0125 = 0.600716031 SOL
CEILING    0.600716031 * 1.20   = 0.720859238 SOL
```

### 1B. Crowded entry: find the signal

Several prints sit in the 2 slots before him. His slippage set is already known from 1A. Each value in the set is a separate ceiling.

```
+--------------------------------------------------+
| 1B  CROWDED                                      |
| several prints in the 2 slots before him         |
+--------------------------------------------------+
        his max_sol_cost, his token_amount
        each slippage in the set from 1A
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
        many matches -> Part 2 picks among them
```

On the worked entry the ceiling `0.720859238` is already on his `Buy`, and his set is `{20%}`. Divide by `1.215` and the buy size he decided is `0.593299784` SOL. The signal is the print whose reserves turn his `token_amount` into that SOL. With a set of several values, the same ceiling is divided once per value, and each result is tested the same way.

## Part 2. Instruction pick

Names the instruction shape. Use it when the reserves do not decide, or when several prints match in Part 1 (including matches on different slippages).

```
+--------------------------------------------------+
| 2  INSTRUCTION PICK                              |
| other wallets' buys, 2 slots before him          |
+--------------------------------------------------+
                      |
                      v
        RACER = the instruction list contains
                "System Program: AdvanceNonceAccount"
                or "System Program: CreateAccountWithSeed"
                      |
                      v
        a non-racer buy exists before him?
            yes -> drop every racer
            no  -> the racers stay
                      |
                      v
        drop a candidate whose exact instruction list
        also appears on a buy in the 1 slot after him
                      |
                      v
        of what remains, take the closest print before him
        (latest slot, then latest tx_index)
                      |
                      v
        he lands about 50-100 ms after the signal,
        so the nearest survivor is the signal
```

The exact list is the ordered instruction labels, not a template grain. When both parts run, Part 1 decides which print he priced. Part 2 decides which shapes belong in the target list, and it breaks a tie Part 1 leaves open.
