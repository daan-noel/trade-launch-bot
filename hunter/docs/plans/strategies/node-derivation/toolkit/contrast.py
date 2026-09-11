"""Within-coin contrast: the prints a wallet acts on against the ones it ignores, on the SAME coin.

Everything about the coin - its narrative, its channel, its creator, the hour - is constant
inside one coin, so a difference that survives is an on-chain state at the moment. Each acted
print is ranked among the same coin's ignored prints, per fact; the mean rank over all acted
prints is 0.50 when the fact says nothing, above 0.50 when the wallet prefers HIGH values.

  strat_rank(C, Kc, cols)       C acted rows, Kc ignored rows; both carry `run` (the coin).
                                Returns feature, n, pctile, dev (|pctile - 0.5|), dir
  label_acted(C, S, wallet_id, window=0.5)
                                adds `act` to a candidate table: the wallet bought <= window s
                                after the candidate print (the diagnostic that splits C / Kc)
"""
from __future__ import annotations

import numpy as np
import pandas as pd


def strat_rank(C, Kc, cols):
    num = np.zeros(len(cols)); den = np.zeros(len(cols))
    cg = {r: g[cols].to_numpy() for r, g in C.groupby("run", sort=False)}
    kg = {r: g[cols].to_numpy() for r, g in Kc.groupby("run", sort=False)}
    for r, cm in cg.items():
        km = kg.get(r)
        if km is None:
            continue
        for j in range(len(cols)):
            kv = km[:, j]; kv = kv[np.isfinite(kv)]
            if len(kv) < 2:
                continue
            cv = cm[:, j]; cv = cv[np.isfinite(cv)]
            if not len(cv):
                continue
            kv = np.sort(kv)
            lo = np.searchsorted(kv, cv, side="left")
            hi = np.searchsorted(kv, cv, side="right")
            num[j] += float(((lo + hi) / 2.0 / len(kv)).sum()); den[j] += len(cv)
    out = []
    for j, c in enumerate(cols):
        if den[j]:
            mp = num[j] / den[j]
            out.append(dict(feature=c, n=int(den[j]), pctile=round(mp, 4),
                            dev=round(abs(mp - 0.5), 4), dir="HIGH" if mp > 0.5 else "low"))
    return pd.DataFrame(out).sort_values("dev", ascending=False)


def label_acted(C, S, wallet_id, window=0.5):
    T = S.T
    act = np.zeros(len(C), dtype=np.int8)
    for r, g in C.groupby("run", sort=False):
        a, b = T.start[r], T.end[r]
        tm = np.maximum.accumulate(T.t[a:b])
        bt = tm[(T.wallet[a:b] == wallet_id) & (T.side[a:b] == 1)]
        if not len(bt):
            continue
        tk = tm[g.k.to_numpy()]
        q = np.searchsorted(bt, tk, side="right")
        ok = q < len(bt)
        hit = np.zeros(len(g), dtype=bool)
        hit[ok] = bt[q[ok]] - tk[ok] <= window
        act[C.index.get_indexer(g.index)] = hit
    return C.assign(act=act)
