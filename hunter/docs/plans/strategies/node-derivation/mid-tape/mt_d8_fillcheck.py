"""Audit: does the fill model price the state our real buys met?

The model (kernel.fill_idx, and the engine's LagMs rule): our buy meets the state left by the last
print landed by trigger + lag. Our real copy-rule buys since 09-01 (strategy_positions, mode real)
say where we actually landed: our buy is on the lake by its signature, the trigger is the print in
target_slot nearest target_time. The state our buy met is the print just before ours in chain
order (slot, tx_index, leg_index).

  gap %   spot our buy met / spot the model prices - 1 (positive = we paid more than the model)
  read at lag = 83 ms (the measured seat p50) and at each fill's own measured lag

  python mt_d8_fillcheck.py
"""
from __future__ import annotations

import _paths  # noqa: F401
from _paths import HUNTER, LAKE

import json

import numpy as np
import pandas as pd
import psycopg2
import pyarrow.compute as pc
import pyarrow.parquet as pq

COLS = ["mint", "slot", "tx_index", "leg_index", "block_time", "vsol", "vtok", "is_buy", "sol_amount",
        "tx_signature", "wallet", "venue"]
MAX_WAIT_SLOTS = 3


def positions():
    url = next(l.split("=", 1)[1].strip().strip('"') for l in
               (HUNTER / ".env").read_text(encoding="utf-8").splitlines() if l.startswith("DATABASE_URL="))
    c = psycopg2.connect(url)
    cur = c.cursor()
    cur.execute("""select id::text, mint_address, target_slot, extract(epoch from target_time)*1e6,
                          extract(epoch from entry_time)*1e6, entry_slot, entry_tx_signatures, wallet
                   from strategy_positions where mode = 'real' and entry_time >= '2026-09-01'""")
    rows = cur.fetchall()
    c.close()
    return pd.DataFrame(rows, columns=["id", "mint", "tslot", "t_target", "t_entry", "eslot", "sigs", "wallet"])


def prints(mints):
    out = []
    for d in sorted((LAKE / "trades").glob("dt=2026-09-*")):
        t = pq.read_table(str(d / "data.parquet"), columns=COLS,
                          filters=[("mint", "in", list(mints))])
        if t.num_rows:
            out.append(t.to_pandas())
    P = pd.concat(out, ignore_index=True)
    P = P[(P.venue == "curve") & P.vsol.notna() & (P.vsol > 0)]
    return P.sort_values(["mint", "slot", "tx_index", "leg_index"]).reset_index(drop=True)


def fill_at(t, s, f, lag_us):
    """The engine's LagMs exit rule: the last print landed by t[f] + lag, in f's slot or the next
    observed slot when it is at most 3 slots on; none -> f."""
    dl = t[f] + lag_us
    nxt = next((s[i] for i in range(f + 1, len(s)) if s[i] > s[f]), None)
    win = {s[f]} | ({nxt} if nxt is not None and nxt <= s[f] + MAX_WAIT_SLOTS else set())
    last = f
    i = f + 1
    while i < len(s) and s[i] in win:
        if t[i] <= dl:
            last = i
        i += 1
    return last


def main():
    D = positions()
    D["sig"] = [(json.loads(x) if isinstance(x, str) else x)[0] for x in D.sigs]
    P = prints(set(D.mint))
    rows = []
    for _, p in D.iterrows():
        g = P[P.mint == p.mint].reset_index(drop=True)
        if not len(g):
            rows.append(dict(id=p.id, why="mint not on the lake")); continue
        me = np.nonzero(g.tx_signature.to_numpy() == p.sig)[0]
        if not len(me):
            rows.append(dict(id=p.id, why="our buy not on the lake")); continue
        m0 = int(me[0])
        t_ours = int(g.block_time.iat[m0]); s_ours = int(g.slot.iat[m0])
        # the tape a backtest sees has no buy of ours: drop it; m = its place in what is left
        g = g[g.tx_signature != p.sig].reset_index(drop=True)
        m = m0
        t = g.block_time.to_numpy().astype(np.int64); s = g.slot.to_numpy()
        spot = (g.vsol / g.vtok).to_numpy()
        cand = np.nonzero((s == p.tslot) & (np.arange(len(g)) < m))[0]
        if not len(cand):
            rows.append(dict(id=p.id, why="no trigger print before ours in target_slot")); continue
        f = int(cand[np.argmin(np.abs(t[cand] - p.t_target))])
        met = m - 1
        lag_own = int(t_ours - t[f])
        x83 = fill_at(t, s, f, 83_000)
        xown = fill_at(t, s, f, max(lag_own, 0))
        rows.append(dict(id=p.id, why="ok", lag_ms=lag_own / 1000, same_slot=int(s_ours == s[f]),
                         d_slot=int(s_ours - s[f]), between=int(m - f - 1),
                         gap83=100 * (spot[met] / spot[x83] - 1), gapown=100 * (spot[met] / spot[xown] - 1),
                         trig_move=100 * (spot[met] / spot[f] - 1),
                         model_after_ours=int(x83 >= m)))
    R = pd.DataFrame(rows)
    print(R.why.value_counts().to_string())
    ok = R[R.why == "ok"]
    print("\nreal buys matched: %d" % len(ok))
    print("lake-clock lag trigger -> our print, ms: p10/25/50/75/90 %s" %
          np.round(ok.lag_ms.quantile([.1, .25, .5, .75, .9]).values, 0))
    print("our print in the trigger's slot: %.1f %%   prints between trigger and ours: p50 %d  p90 %d" % (
        100 * ok.same_slot.mean(), ok.between.median(), ok.between.quantile(.9)))
    for c, lab in (("gap83", "model at 83 ms"), ("gapown", "model at each fill's own lag")):
        x = ok[c]
        print("%-30s gap %%: mean %+.2f  median %+.2f  p10 %+.2f  p90 %+.2f  worse than model %.1f %%  "
              "|gap| < 0.1 %% %.1f %%" % (lab, x.mean(), x.median(), x.quantile(.1), x.quantile(.9),
                                        100 * (x > 0.1).mean(), 100 * (x.abs() < 0.1).mean()))
    print("the model's fill print lands at or after our own buy (83 ms): %.1f %%" % (100 * ok.model_after_ours.mean()))
    print("spot our buy met vs the trigger print's spot: mean %+.2f  median %+.2f" % (ok.trig_move.mean(),
                                                                                        ok.trig_move.median()))
    R.to_csv(_paths.DATA / "mt_d8_fillcheck.csv", index=False)


if __name__ == "__main__":
    main()
