# Instruction pick

Reference only. Kept for later. [Reserve match](_!___signal-reserve-match.md) names the print the pages use. This method does not run, and a reserve match that names several prints stays unnamed.

Names the instruction shape. Its window is the 2 slots before his entry, and the 1 slot after it. A print is before him when `(slot, tx_index)` is strictly earlier than his buy.

```
+--------------------------------------------------+
| INSTRUCTION PICK                                 |
| other wallets' buys, 2 slots before him          |
+--------------------------------------------------+
                      |
                      v
        RACER = the instruction list contains
                "System Program: AdvanceNonceAccount"
                or "System Program: CreateAccountWithSeed"
                      |
                      v
        one plain shape before him?
            yes -> that shape is the signal
                   even when it also buys after him
            no  -> drop a plain shape whose exact list
                   also appears on a buy in the slot after
                      |
                      v
        a plain shape remains?
            yes -> take the closest plain shape
                   the racers stay out
            no  -> take the closest racer
                      |
                      v
        closest = latest slot, then latest tx_index
                      |
                      v
        he lands about 50-100 ms after the signal,
        so the nearest one kept is the signal
```

The exact list is the ordered instruction labels, not a template grain. Closest is the latest slot, then the latest tx_index. The 50-100 ms line is why that nearest survivor is the one kept. It is not a cutoff.
