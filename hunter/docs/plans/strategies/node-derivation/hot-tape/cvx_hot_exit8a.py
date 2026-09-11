"""Hot-tape node, step 39a: 8fStGV's exit hazard, read on the pool rule 1 selects.

Rule 1's bracket (take profit +10 %, stop -25 %, 60 s) was read off 8fStGV's closing sells on ALL
its holds (step 29), before the permission existed. Law 26: an exit is read at the actor's hold, on
the pool the sentence selects. Here its holds are split at entry by rule 1's permission (age >=
158 s and >= 368 public wallets holding) and by whether the entry is rule 1's kind (8fStGV buys
<= 300 ms after a public sell >= 1 SOL), and for each pool:

  hazard   the chance its next print is the closing sell, by profit since entry x time held
  shape    profit at the close, time held, the in-hold peak, and how the close splits into
           take-profit (>= +8 %), stop (<= -20 %) and in between

Profit is off the curve: (v / v_entry)^2 - 1, v_entry the reserve after its first buy.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import K
from cvx import DAY0
from tape import Tape
from cvx_hot_event import node_ids

PNL_E = np.array([-1e9, -40, -30, -25, -20, -10, 0, 5, 8, 10, 12, 15, 20, 30, 1e9])
PNL_L = ["<-40", "-40..-30", "-30..-25", "-25..-20", "-20..-10", "-10..0", "0..5", "5..8", "8..10",
         "10..12", "12..15", "15..20", "20..30", ">30"]
HLD_E = np.array([0, 5, 10, 20, 30, 45, 60, 90, 1e9])
HLD_L = ["0-5s", "5-10s", "10-20s", "20-30s", "30-45s", "45-60s", "60-90s", ">90s"]

pd.set_option("display.width", 400)
pd.set_option("display.max_rows", 200)


def pctv(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return ((a / b) ** 2 - 1.0) * 100.0


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    inv = {v: k for k, v in lab.items()}
    w8 = inv["8fStGV"]
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    is_node = np.isin(T.wallet, list(lab))
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    runs = np.unique(T.run_of[T.wallet == w8])
    HZ = {}
    eps = []
    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 30 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]; wal = T.wallet[a:b]
        tm = np.maximum.accumulate(t)
        age = tm - c_s[r]
        mine = is_node[a:b]; pub = ~mine
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
        tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
        hc = np.zeros(n, dtype=np.int32); pos = {}; holders = 0
        for i in range(n):
            hc[i] = holders
            if mine[i]:
                continue
            w = wal[i]; p = pos.get(w, 0.0)
            q = p + tokd[i] if side[i] == 1 else max(p - tokd[i], 0.0)
            holders += int(q > 0) - int(p > 0); pos[w] = q
        bigsell_t = tm[pub & (side == -1) & (sol >= 1.0)]
        idx = np.nonzero(wal == w8)[0]
        p8 = 0.0; peak8 = 0.0; e0 = -1
        for s in idx:
            s = int(s)
            if side[s] == 1:
                if p8 <= 0.0:
                    e0 = s; peak8 = 0.0
                p8 += tokd[s]; peak8 = max(peak8, p8)
                continue
            if p8 <= 0.0 or e0 < 0:
                continue
            p8 = max(p8 - tokd[s], 0.0)
            if p8 > 0.02 * peak8:
                continue
            p8 = 0.0
            v0 = float(v[e0])
            q = int(np.searchsorted(bigsell_t, tm[e0], side="right"))
            e_kind = int(q > 0 and tm[e0] - bigsell_t[q - 1] <= 0.300)
            pool = ("P" if (hc[e0] >= 368 and age[e0] >= 158.0) else "notP") + \
                   (" E-kind" if e_kind else " other")
            hold = np.nonzero(pub[e0 + 1:s])[0] + e0 + 1
            pk = float(pctv(v[e0:s].max(), v0)) if s > e0 else 0.0
            pnl = float(pctv(v[s - 1], v0))
            eps.append((pool, r, pnl, float(tm[s] - tm[e0]), pk, e_kind))
            H = HZ.setdefault(pool, np.zeros((2, len(PNL_L), len(HLD_L))))
            pi = np.searchsorted(PNL_E, pnl, side="right") - 1
            hi = np.searchsorted(HLD_E, tm[s] - tm[e0], side="right") - 1
            if 0 <= pi < len(PNL_L) and 0 <= hi < len(HLD_L):
                H[0, pi, hi] += 1
            if len(hold):
                vp = v[hold - 1]
                pr = pctv(vp, v0); hl = tm[hold] - tm[e0]
                pi = np.searchsorted(PNL_E, pr, side="right") - 1
                hi = np.searchsorted(HLD_E, hl, side="right") - 1
                ok = (pi >= 0) & (pi < len(PNL_L)) & (hi >= 0) & (hi < len(HLD_L))
                np.add.at(H[1], (pi[ok], hi[ok]), 1.0)
    E = pd.DataFrame(eps, columns=["pool", "run", "pnl", "held", "peak", "ekind"])
    print("8fStGV closed holds %s  %ds" % (f"{len(E):,}", time.time() - t0))
    print("\n=== the shape of its closes, by pool")
    for pool, g in E.groupby("pool"):
        print("  %-14s n %5d  close pnl p25/50/75 %s  held p25/50/75 %s s  peak p50 %.1f  "
              "tp(>=+8) %.1f %%  stop(<=-20) %.1f %%  between %.1f %%"
              % (pool, len(g), np.round(g.pnl.quantile([.25, .5, .75]).values, 1),
                 np.round(g.held.quantile([.25, .5, .75]).values, 1), g.peak.median(),
                 100 * (g.pnl >= 8).mean(), 100 * (g.pnl <= -20).mean(),
                 100 * ((g.pnl > -20) & (g.pnl < 8)).mean()))
    for pool in sorted(HZ):
        num, den = HZ[pool]
        with np.errstate(divide="ignore", invalid="ignore"):
            hz = np.where(den >= 30, 100.0 * num / (num + den), np.nan)
        print("\n=== HAZARD, pool %s: %% of in-hold public prints at which it closes next" % pool)
        print(pd.DataFrame(hz, index=PNL_L, columns=HLD_L).round(2).to_string())
        print("    closes per cell")
        print(pd.DataFrame(num, index=PNL_L, columns=HLD_L).astype(int).to_string())
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
