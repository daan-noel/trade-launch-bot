"""Hot-tape node, step 36: the SECOND event as a public sentence - the capitulation cascade.

Step 35 finds the second paying operator's reachable leg: 49uohd buys 177-360 ms after a public
sell >= 1 SOL, we land ahead of it 96.8 % of the time, and our seat on its picks books +3.50 %/trade
on 8 of 8 days under cap15. Within the coin, the sells it picks are:

  price move over the last 10 s        -10.1 %  against  -1.1 %
  big sells in the last 10 s            2        against  1
  prints in the last 5 s               15        against  9
  gap before the sell                   0.06 s   against  0.24 s
  the seller's profit on this sale     -3.9 %    against  +7.3 %

A losing holder dumps into a falling, busy tape - a capitulation cascade - and the bounce is quick.
It is the other side of E (1.12), where a winning flipper sells into a rising frenzy.

Here: every public sell >= 1 SOL at age >= 60 s on the full tape, the terms added one at a time
(each must lift the book), then the established-coin permission (1.14), each gate with its own
occupancy, under cap15 and the bracket. The member's coin list is split at its first buy on the
coin (the 1.13 lesson) and is a diagnostic only.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import K, fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids
from cvx_hot_exit7 import EXITS, run_exit

B = 0.2
LAG = 0.115
BRACKET = EXITS[0]

# name, mv10 max, sells10 min, n5 min, gap max, seller pnl max, holders min, age min
GATES = [
    ("sell>=1", 1e9, 0, 0, 1e9, 1e9, 0, 60.0),
    ("+ falling (mv10<=-5)", -5.0, 0, 0, 1e9, 1e9, 0, 60.0),
    ("+ cascade (2nd big sell in 10 s)", -5.0, 2, 0, 1e9, 1e9, 0, 60.0),
    ("+ busy (n5>=12)", -5.0, 2, 12, 1e9, 1e9, 0, 60.0),
    ("+ inside a burst (gap<=0.1 s)", -5.0, 2, 12, 0.1, 1e9, 0, 60.0),
    ("+ seller at a loss", -5.0, 2, 12, 0.1, 0.0, 0, 60.0),
    ("cascade + loss, no gap term", -5.0, 2, 12, 1e9, 0.0, 0, 60.0),
    ("full + established coin", -5.0, 2, 12, 0.1, 0.0, 368, 158.0),
    ("no-gap + established coin", -5.0, 2, 12, 1e9, 0.0, 368, 158.0),
    ("falling + cascade + established", -5.0, 2, 0, 1e9, 1e9, 368, 158.0),
    ("sell>=1 + established (control)", 1e9, 0, 0, 1e9, 1e9, 368, 158.0),
]

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 400)


def run(T, c_s, is_node, wm, gates, t_min=-np.inf):
    rows = []
    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 60 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]; wal = T.wallet[a:b]
        tm = np.maximum.accumulate(t)
        age = tm - c_s[r]
        if age[-1] < 60.0:
            continue
        mine = is_node[a:b]
        pub = ~mine
        big = pub & (side == -1) & (sol >= 1.0) & (age >= 60.0) & (t >= t_min)
        if not big.any():
            continue
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
        tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
        mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        gap = np.concatenate(([1e9], np.diff(tm)))
        j3 = np.searchsorted(tm, tm - 3.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        j10 = np.searchsorted(tm, tm - 10.0, side="left")
        cbp = np.concatenate(([0.0], np.cumsum(np.where(pub & (side == 1), sol, 0.0))))
        cb3 = cbp[np.arange(n) + 1] - cbp[j3]
        cbig = np.concatenate(([0], np.cumsum((side == -1) & (sol >= 1.0))))
        vprev = np.concatenate(([np.nan], v[:-1]))
        mv10 = ((vprev / v[np.maximum(j10 - 1, 0)]) ** 2 - 1.0) * 100.0
        sells10 = cbig[np.arange(n)] - cbig[j10]
        n5 = np.arange(n) - j5
        cand = np.nonzero(big)[0]
        cset = set(cand.tolist())
        spnl = np.full(n, np.nan)
        hc = np.zeros(n, dtype=np.int32)
        pos = {}; cost = {}; holders = 0
        for i in range(n):
            hc[i] = holders
            w = wal[i]
            if i in cset:
                p = pos.get(w, 0.0)
                if p > 0 and tokd[i] > 0 and cost.get(w, 0.0) > 0:
                    spnl[i] = ((sol[i] / tokd[i]) / (cost[w] / p) - 1.0) * 100.0
            if mine[i]:
                continue
            p = pos.get(w, 0.0)
            if side[i] == 1:
                q = p + tokd[i]; cost[w] = cost.get(w, 0.0) + sol[i]
            else:
                q = max(p - tokd[i], 0.0)
                if p > 0:
                    cost[w] = cost.get(w, 0.0) * q / p
            holders += int(q > 0) - int(p > 0)
            pos[w] = q
        mb = tm[(wal == wm) & (side == 1)]
        first_m = mb[0] if len(mb) else np.inf
        for gi, (nm, m10, s10, nn5, gmax, pmax, hmin, amin) in enumerate(gates):
            ok = big & (np.nan_to_num(mv10, nan=1e9) <= m10) & (sells10 >= s10) & (n5 >= nn5) \
                & (gap <= gmax) & (hc >= hmin) & (age >= amin)
            if pmax < 1e8:
                ok &= np.nan_to_num(spnl, nan=1e9) <= pmax
            last = -1
            for k in np.nonzero(ok)[0]:
                k = int(k)
                if k <= last:
                    continue
                ei = fill_idx(t, k, LAG)
                jx = max(int(np.searchsorted(t, t[ei] + 15.0, side="right") - 1), ei)
                x15 = fill_idx(t, jx, LAG)
                y15 = net(float(v[ei]), float(v[x15]), B)
                yb, xb, why, _ = run_exit(BRACKET, t, v, side, sol, mv, cb3, ei)
                rows.append((r, gi, int(T.day[a + k]), y15, yb, why, int(len(mb) > 0),
                             int(tm[k] < first_m)))
                last = x15
    return pd.DataFrame(rows, columns=["run", "gate", "day", "y15", "yb", "why", "cm", "before"])


def report(F, days, gates):
    out = []
    for gi, g in enumerate(gates):
        d = F[F.gate == gi]
        if not len(d):
            continue
        for yc, xl in (("y15", "cap15"), ("yb", "bracket")):
            dd = d.assign(y=d[yc])
            s = dd.y.sum()
            top = dd.y.nlargest(max(1, int(round(0.01 * len(dd))))).sum()
            c = cell(dd, days=days)
            c["body"] = round(s - top, 2)
            c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
            c["maxcoin"] = round(100 * dd.groupby("run").y.sum().max() / s, 1) if s > 0 else np.nan
            dl = np.sort(dd.day.unique()); h = len(dl) // 2
            c["half1"] = round(float(dd[dd.day.isin(dl[:h])].y.mean()) / B * 100, 2)
            c["half2"] = round(float(dd[dd.day.isin(dl[h:])].y.mean()) / B * 100, 2)
            c["its_before"] = round(float(dd[(dd.cm == 1) & (dd.before == 1)].y.mean()) / B * 100, 2)
            c["its_after"] = round(float(dd[(dd.cm == 1) & (dd.before == 0)].y.mean()) / B * 100, 2)
            c["other"] = round(float(dd[dd.cm == 0].y.mean()) / B * 100, 2)
            out.append(dict(gate=g[0], exit=xl, **c))
    show(out, "the capitulation cascade, own occupancy per gate  (its_before / its_after / other: "
              "the member's coin list split at its first buy - diagnostic)")


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    inv = {v: k for k, v in lab.items()}
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    is_node = np.isin(T.wallet, list(lab))
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    print("tape %s prints  %ds" % (f"{T.n:,}", time.time() - t0), flush=True)
    F = run(T, c_s, is_node, inv["49uohd"], GATES)
    F.to_parquet(data_file("cvx_hot_ev2.parquet"), index=False)
    print("fires %s  %ds" % (f"{len(F):,}", time.time() - t0), flush=True)
    report(F, days, GATES)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
