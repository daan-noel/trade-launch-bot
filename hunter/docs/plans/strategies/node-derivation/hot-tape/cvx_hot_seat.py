"""Hot-tape node, step 22: the seat we ACTUALLY get if we fire on their trigger.

Step 21 finds the trigger: a BURST START - a public buy print that opens a run after a quiet gap -
and both halves of the node react to it, at different latencies. That turns the seat question from
a hypothetical into an arithmetic one:

    fire on the same burst-start print they react to, fill 115 ms later, and see where we land.

  ahead of their print   we get the RACE seat, and 1.11's +2.27 %/trade is reachable
  same slot              PEER: the ordering is the leader's coin flip
  behind their print     FOLLOW, and 1.11 measures that at -0.66 %/trade

This is the number the whole node depends on, and nothing in 1.5 to 1.11 has measured it: 1.5's
attempt was a waiting time and was retracted in 1.9, and 1.11's RACE column is a ceiling that
assumes the seat rather than earning it.

Reported per wallet, because the two halves of the node differ in exactly this.

  D none   E the burst-start print preceding their buy   P none
  X cap15 . cap60      R one per coin   S 0.2
  seat  our fire is the burst-start print; our fill is the last print landed 115 ms after it
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids

B = 0.2
LAG = 0.115
BURST_GAP = 0.4
MAX_TRIG = 1.0            # a burst start further back than this is not their trigger

pd.set_option("display.width", 480)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 40)


def clock(t, v, ei, cap):
    j = max(int(np.searchsorted(t, t[ei] + cap, side="right") - 1), ei)
    x = fill_idx(t, j, LAG)
    return net(float(v[ei]), float(v[x]), B)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    is_node = np.isin(T.wallet, NODE)
    E = pd.read_parquet(data_file("cvx_hot_sep.parquet"))
    print("episodes %s  coins %s  %ds" % (f"{len(E):,}", f"{E.run.nunique():,}",
                                          time.time() - t0), flush=True)

    byrun = {}
    for i, r in enumerate(E.run.values):
        byrun.setdefault(int(r), []).append(i)
    kki = E.ki.values.astype(int)
    out = np.full((len(E), 9), np.nan)
    for r, idxs in byrun.items():
        a, b = T.start[r], T.end[r]
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        slot = T.slot[a:b]
        mine = is_node[a:b]
        tm = np.maximum.accumulate(t)
        gap = np.concatenate(([1e9], np.diff(tm)))
        bs = (~mine) & (side == 1) & (gap >= BURST_GAP)
        bidx = np.nonzero(bs)[0]
        if not len(bidx):
            continue
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        for i in idxs:
            k = int(kki[i])
            if k >= len(t):
                continue
            p = int(np.searchsorted(bidx, k, side="left")) - 1
            if p < 0:
                continue
            bj = int(bidx[p])
            dt = float(tm[k] - tm[bj])
            if dt > MAX_TRIG:
                continue
            ei = fill_idx(t, bj, LAG)          # where WE land firing on that burst start
            out[i] = (dt, bj, ei, float(ei < k), float(slot[ei] == slot[k]),
                      float(tm[ei] - tm[bj]),
                      clock(t, v, ei, 15.0), clock(t, v, ei, 60.0),
                      net(float(vbef[k]), float(v[fill_idx(t, max(int(np.searchsorted(
                          t, t[k] + 15.0, side="right") - 1), k), LAG)]), B))
    cols = ["dt", "bj", "ei", "ahead", "sameslot", "our_lag", "y15", "y60", "race15"]
    for j, c in enumerate(cols):
        E[c] = out[:, j]
    D = E[np.isfinite(E.y15)].copy()
    print("episodes with a burst start within %.1f s: %s of %s (%.1f %%)  %ds"
          % (MAX_TRIG, f"{len(D):,}", f"{len(E):,}", 100 * len(D) / len(E), time.time() - t0),
          flush=True)

    PAY = ("8fStGV", "AbQcLH", "49uohd")
    D["grp"] = np.where(D.w.isin(PAY), "pay", "rest")

    print("\n=== 1. WHERE WE LAND, firing on the burst-start print they react to")
    rows = []
    for lbl, d in [("all six", D)] + [(w, D[D.w == w]) for w in sorted(D.w.unique())]:
        rows.append(dict(who=lbl, n=len(d),
                         their_lag_p50_ms=round(1000 * d.dt.median()),
                         their_lag_p25=round(1000 * d.dt.quantile(.25)),
                         their_lag_p75=round(1000 * d.dt.quantile(.75)),
                         we_land_ahead=round(100 * d.ahead.mean(), 1),
                         same_slot=round(100 * d.sameslot.mean(), 1)))
    print(pd.DataFrame(rows).to_string(index=False), flush=True)

    print("\n=== 2. THE BOOK at that seat, against the RACE ceiling of 1.11")
    rows = []
    for lbl, d in [("all six", D), ("the three that pay", D[D.grp == "pay"]),
                   ("the other three", D[D.grp == "rest"])] \
            + [(w, D[D.w == w]) for w in sorted(D.w.unique())]:
        for ycol, nm in (("y15", "burst-start seat, cap15"), ("y60", "burst-start seat, cap60"),
                         ("race15", "RACE ceiling, cap15")):
            dd = d.copy(); dd["y"] = dd[ycol]
            s = dd.y.sum()
            top = dd.y.nlargest(max(1, int(round(0.01 * len(dd))))).sum()
            c = cell(dd, days=days)
            c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
            c["body"] = round(s - top, 2)
            rows.append(dict(who=lbl, seat=nm, **c))
    show(rows, "2. the book")

    print("\n=== 3. does winning the race decide the money?")
    rows = []
    for lbl, d in (("the three that pay", D[D.grp == "pay"]),
                   ("the other three", D[D.grp == "rest"])):
        for nm, m in (("we land AHEAD of them", d.ahead == 1),
                      ("we land behind them", d.ahead == 0)):
            dd = d[m].copy(); dd["y"] = dd.y15
            rows.append(dict(who=lbl, slice=nm, **cell(dd, days=days)))
    show(rows, "3. ahead against behind, cap15")

    D.to_parquet(data_file("cvx_hot_seat.parquet"), index=False)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
