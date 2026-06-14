
### Candidate A — "survivor continuation" (selective)

```
p_entry_min_age_secs     = 30      # skip the spike (winner peaked at 41s, not at launch)
p_entry_min_liquidity_sol= 15      # winner ~29 SOL; flat losers were 3–7 → this cut is the real filter
p_entry_min_alive_sol    = 3       # require ongoing volume in the 10s window
p_entry_max_cohort_held  = 0.40    # launch cohort must have sold down
p_entry_min_organic_sol  = 2       # outside demand, not cohort wash
p_exit_take_profit       = 70      # winner ran 3.3×; bank a real scalp, don't be greedy
p_exit_stop_loss         = 20      # winner only dipped to 0.85 (-15%); 20% keeps it, cuts the bleeders
p_exit_trailing_stop_pct = 20
p_exit_stall_secs        = 25      # winners moved fast; flat tokens bleed when held
p_exit_time_stop_secs    = 90
p_exit_liquidity_drop_pct= 35
p_exit_cohort_ratio      = 0.10    # E5 rug-dump bail
p_max_concurrent_tokens  = 2
buy_amount               = small   # paper / tiny size
```

### Candidate B — same gates, `p_entry_min_liquidity_sol = 22`

Even more selective; in this data it isolates the EXht/4eDK (20+ SOL) class and skips the
marginal 10-SOL tokens. Fewer entries, higher hit-quality — but also fewer samples, so
it's *more* overfit, not less.
