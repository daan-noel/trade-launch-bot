"""Hot-tape node, step 35: the SECOND event - which triggers the other paying operator acts on.

AbQcLH and 49uohd share one 15-wallet recipe set: one operator, two legs. Their reactions
(`cvx_hot_trig.py`, excess intensity over the coin's own print rate):

  AbQcLH   a BURST START (first public buy after >= 0.4 s of silence) at 25-50 ms, lift 9.9
  49uohd   the partner's print at 25-50 ms (lift 6.4), and a public SELL >= 1 SOL / a -2 % print
           at 150-300 ms (lift 3.3-3.7)

Step 22 books AbQcLH's burst starts at our seat at -0.80 %/trade: at a 69 ms median lag it is
ahead of our 115 ms two times in three, and a burst start is a BUY, so the price leaves during
our lag. 49uohd's dip-buy is slower than our fill. This run does, for both triggers, what step 24
did for the frenzy-absorbed sell - on the same coin, the triggers the member acts on against the
ones it ignores - and books our seat on its picks under the bracket (1.13) and under cap15.

  trigger A   public burst start, case = AbQcLH buys <= 150 ms after it
  trigger B   public sell >= 1 SOL, case = 49uohd buys 100-500 ms after it
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from kernel import K, fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import ROUTERS, node_ids
from cvx_hot_which import strat_rank
from cvx_hot_exit7 import EXITS, run_exit

B = 0.2
LAG = 0.115
BURST_GAP = 0.4
BRACKET = EXITS[0]
TRIG = {"A": ("AbQcLH", 0.0, 0.150), "B": ("49uohd", 0.100, 0.500)}
FEATS = ["size", "gap", "mv10", "mv60", "stall", "dd", "nb5", "n5", "buys2", "sells10", "age",
         "v", "holders", "b_fresh", "b_router", "b_pro", "b_first", "s_hold", "s_pnl"]

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 400)


def pctv(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return float(((a / b) ** 2 - 1.0) * 100.0)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    inv = {v: k for k, v in lab.items()}
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router_all = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()
    nb = len(T.builds)
    cnt = np.bincount(T.build, minlength=nb).astype(np.int64)
    wpb = pd.DataFrame({"b": T.build, "w": T.wallet}).drop_duplicates()
    nwal = np.bincount(wpb.b.to_numpy(), minlength=nb).astype(np.int64)
    is_op_b = (cnt >= 200) & (nwal <= 50)
    first_t = pd.Series(T.t).groupby(T.wallet).transform("min").to_numpy()
    fresh_all = (T.t - first_t) < 600.0
    is_node = np.isin(T.wallet, NODE)
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    wid = {g: inv[m] for g, (m, _, _) in TRIG.items()}
    runs = np.unique(T.run_of[np.isin(T.wallet, list(wid.values()))])
    print("tape %s prints  operator coins %s  %ds" % (f"{T.n:,}", f"{len(runs):,}",
                                                       time.time() - t0), flush=True)

    rows = []
    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 30 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]; bu = T.build[a:b]; rt = router_all[a:b]; fresh = fresh_all[a:b]
        mine = is_node[a:b]; pub = ~mine
        tm = np.maximum.accumulate(t)
        age = tm - c_s[r]
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
        tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
        mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        gap = np.concatenate(([1e9], np.diff(tm)))
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j3 = np.searchsorted(tm, tm - 3.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        j10 = np.searchsorted(tm, tm - 10.0, side="left")
        j60 = np.searchsorted(tm, tm - 60.0, side="left")
        cbp = np.concatenate(([0.0], np.cumsum(np.where(pub & (side == 1), sol, 0.0))))
        cb3 = cbp[np.arange(n) + 1] - cbp[j3]
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        trig = {"A": np.nonzero(pub & (side == 1) & (gap >= BURST_GAP) & (age >= 60.0))[0],
                "B": np.nonzero(pub & (side == -1) & (sol >= 1.0) & (age >= 60.0))[0]}
        buyt = {g: tm[(wal == wid[g]) & (side == 1)] for g in TRIG}
        want = {}
        for g in TRIG:
            for k in trig[g]:
                want.setdefault(int(k), []).append(g)
        if not want:
            continue
        pos = {}; cost = {}; lastbuy = {}; seen = set(); holders = 0
        for i in range(n):
            gs = want.get(i)
            if gs:
                w = wal[i]
                p = pos.get(w, 0.0)
                avgc = cost.get(w, 0.0) / p if p > 0 else np.nan
                tk = tokd[i]
                pnow = sol[i] / tk if tk > 0 else np.nan
                s_pnl = (pnow / avgc - 1.0) * 100.0 if (side[i] == -1 and np.isfinite(avgc)
                                                        and avgc > 0) else np.nan
                lb = lastbuy.get(w)
                ei = fill_idx(t, i, LAG)
                y_b, _, why, _ = run_exit(BRACKET, t, v, side, sol, mv, cb3, ei)
                jx = max(int(np.searchsorted(t, t[ei] + 15.0, side="right") - 1), ei)
                y15 = net(float(v[ei]), float(v[fill_idx(t, jx, LAG)]), B)
                f = (float(sol[i]), float(min(gap[i], 60.0)),
                     pctv(v[i - 1], v[max(int(j10[i]) - 1, 0)]) if i else np.nan,
                     pctv(v[i - 1], v[max(int(j60[i]) - 1, 0)]) if i else np.nan,
                     float(stall[i]), pctv(v[i - 1], rmax[i - 1]) if i else np.nan,
                     float(len(np.unique(bu[j5[i]:i]))), float(i - j5[i]),
                     float(sol[j2[i]:i][side[j2[i]:i] == 1].sum()),
                     float(((side[j10[i]:i] == -1) & (sol[j10[i]:i] >= 1.0)).sum()),
                     float(age[i]), float(vbef[i]), float(holders),
                     float(fresh[i]), float(rt[i]), float(is_op_b[bu[i]]),
                     float(w not in seen),
                     tm[i] - lb if (side[i] == -1 and lb is not None) else np.nan, s_pnl)
                for g in gs:
                    _, lo, hi = TRIG[g]
                    bt = buyt[g]
                    q = int(np.searchsorted(bt, tm[i] + lo, side="left"))
                    hit = int(q < len(bt) and bt[q] - tm[i] <= hi)
                    lag = float(bt[q] - tm[i]) if q < len(bt) else np.nan
                    ahead = int(hit and t[ei] < bt[q])
                    rows.append((g, r, int(T.day[a + i]), hit, ahead, lag, y15, y_b, why) + f)
            if mine[i]:
                continue
            w = wal[i]
            seen.add(w)
            p = pos.get(w, 0.0)
            if side[i] == 1:
                q_ = p + tokd[i]
                cost[w] = cost.get(w, 0.0) + sol[i]
                lastbuy[w] = tm[i]
            else:
                q_ = max(p - tokd[i], 0.0)
                if p > 0:
                    cost[w] = cost.get(w, 0.0) * q_ / p
            holders += int(q_ > 0) - int(p > 0)
            pos[w] = q_

    D = pd.DataFrame(rows, columns=["g", "run", "day", "hit", "ahead", "lag", "y15", "yb", "why"]
                     + FEATS)
    D.to_parquet(data_file("cvx_hot_which2.parquet"), index=False)
    print("triggers %s  %ds" % (f"{len(D):,}", time.time() - t0), flush=True)

    for g, (m, lo, hi) in TRIG.items():
        d = D[D.g == g]
        C = d[d.hit == 1]; Kc = d[(d.hit == 0) & d.run.isin(set(C.run))]
        print("\n######## trigger %s (%s): %s triggers on its coins, it acts on %s (%.1f %%), lag "
              "p25/50/75 %s ms, we land ahead %.1f %%"
              % (g, m, f"{len(d):,}", f"{len(C):,}", 100 * len(C) / max(len(d), 1),
                 np.round(1000 * C.lag.quantile([.25, .5, .75]).values).astype(int).tolist(),
                 100 * C.ahead.mean()))
        rk = strat_rank(C, Kc, FEATS)
        print("=== within-coin rank, picked against ignored (0.5 = nothing)")
        print(rk.to_string(index=False))
        print("=== medians"); print(d.groupby("hit")[FEATS].median().T.round(3).to_string())
        out = []
        for nm, s in (("every trigger on its coins", d), ("the ones it acts on", C),
                      ("  - we land ahead", C[C.ahead == 1]), ("  - we land behind", C[C.ahead == 0]),
                      ("the ones it ignores", d[d.hit == 0])):
            for yc, xl in (("y15", "cap15"), ("yb", "bracket")):
                out.append(dict(slice=nm, exit=xl, **cell(s.assign(y=s[yc]), days=days)))
        show(out, "our seat, 115 ms after the trigger, trigger %s" % g)


if __name__ == "__main__":
    main()
