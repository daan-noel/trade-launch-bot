"""Hot-tape node, step 16: the same split, at BOTH seats and inside a fixed reserve band.

Step 15 (cvx_hot_sep.py) found the separation of their winners from their losers is dominated by
`v` - the reserve at the decision. That is the reachability trap, not prediction: `price = vsol^2/K`
with the curve floored at 30, so a +100 % move needs reserve under 81 and a -50 % is impossible
below 42.43 (7.4 law 21). Every term has to be read INSIDE a band or it is measuring headroom.

Three things this adds:

  1  THE RACE SEAT. Every hot-tape number so far is a FOLLOW reading - we react to their print, so
     we pay its impact and lose its slot, and that costs -5.59 pp (1.4). A rule that decides on a
     LEVEL rather than on their print is not at that seat. The gradient of a term is what matters,
     but only the RACE column says whether a slice is actually positive for a rule anchored on
     state.

  2  THE RESERVE BAND held fixed on every slice.

  3  THE PER-WALLET SPLIT (7.4 law 27). `8fStGV` is a different animal from the other five and the
     pooled book hides it.

The candidate terms come from step 15's own ranking and from the August 64hP study:
     ep_idx     round trips already taken on this coin
     own_dep    price now against their own previous exit on this coin
     stall      seconds since the coin last made a new high
     SUSTAINED  n60_z high with wake_z low  - a busy MINUTE, not a busy two seconds. Step 15 says
                this is the winners' signature and 1.6 says `wake_z` (lift 4.5) is what identifies
                their moment at all, so if this holds, the feature that finds their moment is the
                feature that finds their LOSERS.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import FEE, FIX, K, fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids

B = 0.2
LAG = 0.115

pd.set_option("display.width", 470)
pd.set_option("display.max_rows", 700)
pd.set_option("display.max_columns", 50)

BANDS = [("<42.4", 0.0, 42.43), ("42-50", 42.43, 50.0), ("50-60", 50.0, 60.0),
         ("60-70", 60.0, 70.0), ("70-85", 70.0, 85.0), (">85", 85.0, 1e9)]
SPECS = {"cap15": 15.0, "cap60": 60.0, "cap300": 300.0}


def clock_book(t, v, s, v0, cap):
    """Buy at v0 with the forward path starting at index s; sell at the state cap seconds later."""
    j = int(np.searchsorted(t, t[s] + cap, side="right") - 1)
    j = max(j, s)
    x = fill_idx(t, j, LAG)
    return net(v0, float(v[x]), B)


def trail_book(t, v, s, v0, trail, cap):
    n = len(t)
    e = int(np.searchsorted(t, t[s] + cap, side="right"))
    if e <= s + 1:
        return clock_book(t, v, s, v0, cap)
    pr = (v[s + 1:e] / v0) ** 2
    peak = np.maximum.accumulate(np.maximum(pr, 1.0))
    hit = np.nonzero(pr <= peak * (1.0 - trail / 100.0))[0]
    if len(hit):
        x = fill_idx(t, s + 1 + int(hit[0]), LAG)
        return net(v0, float(v[x]), B)
    return clock_book(t, v, s, v0, cap)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    D = pd.read_parquet(data_file("cvx_hot_sep.parquet"))
    print("episodes %s  coins %s  %ds" % (f"{len(D):,}", f"{D.run.nunique():,}",
                                          time.time() - t0), flush=True)

    byrun = {}
    for i, r in enumerate(D.run.values):
        byrun.setdefault(int(r), []).append(i)
    kki = D.ki.values.astype(int)
    out = np.full((len(D), 8), np.nan)
    for r, idxs in byrun.items():
        a, b = T.start[r], T.end[r]
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        for i in idxs:
            k = int(kki[i])
            if k >= len(t):
                continue
            # FOLLOW: our fill is the last print landed 115 ms after their print
            ef = fill_idx(t, k, LAG)
            # RACE: we are sequenced BEFORE their print, so we meet the reserve they met
            vr = float(vbef[k])
            out[i] = (clock_book(t, v, ef, float(v[ef]), 15.0),
                      clock_book(t, v, ef, float(v[ef]), 60.0),
                      clock_book(t, v, ef, float(v[ef]), 300.0),
                      trail_book(t, v, ef, float(v[ef]), 40.0, 600.0),
                      clock_book(t, v, k, vr, 15.0),
                      clock_book(t, v, k, vr, 60.0),
                      clock_book(t, v, k, vr, 300.0),
                      trail_book(t, v, k, vr, 40.0, 600.0))
    for j, c in enumerate(["f15", "f60", "f300", "ftr40", "r15", "r60", "r300", "rtr40"]):
        D[c] = out[:, j]
    D = D[np.isfinite(D.f60)].copy()
    print("booked both seats  %s rows  %ds" % (f"{len(D):,}", time.time() - t0), flush=True)

    D["band"] = pd.cut(D.v, [0, 42.43, 50, 60, 70, 85, 1e9],
                       labels=[x[0] for x in BANDS]).astype(str)
    D["sustained"] = (D.n60_z >= 0.5) & (D.wake_z <= 0.0)
    D["spike"] = D.wake_z >= 1.0

    def blk(d, name, ycol):
        dd = d.copy()
        dd["y"] = dd[ycol]
        return dict(slice=name, **cell(dd, days=days))

    # ---- 1. the two seats, whole node and per wallet --------------------------------------
    rows = []
    for ycol, lbl in (("f60", "FOLLOW cap60"), ("ftr40", "FOLLOW trail40"),
                      ("r60", "RACE cap60"), ("rtr40", "RACE trail40")):
        rows.append(blk(D, lbl, ycol))
    show(rows, "1. THE WHOLE NODE AT BOTH SEATS")

    rows = []
    for w, g in D.groupby("w"):
        for ycol, lbl in (("f60", "FOLLOW cap60"), ("r60", "RACE cap60")):
            rows.append(dict(w=w, **blk(g, lbl, ycol)))
    show(rows, "2. PER WALLET (7.4 law 27) - the node is not one animal")

    # ---- 3. every term, inside a fixed reserve band ---------------------------------------
    def term_bins(d):
        yield "all", d
        for lo, hi, nm in ((0, 0, "ep 0"), (1, 3, "ep 1-3"), (4, 7, "ep 4-7"),
                           (8, 999, "ep 8+")):
            yield nm, d[(d.ep_idx >= lo) & (d.ep_idx <= hi)]
        dd = d[d.ep_idx > 0]
        for lo, hi, nm in ((-1e9, -30, "own_dep < -30"), (-30, -15, "own_dep -30..-15"),
                           (-15, -5, "own_dep -15..-5"), (-5, 5, "own_dep -5..5"),
                           (5, 1e9, "own_dep > +5")):
            yield nm, dd[(dd.own_dep > lo) & (dd.own_dep <= hi)]
        for lo, hi, nm in ((0, 5, "stall 0-5"), (5, 20, "stall 5-20"), (20, 60, "stall 20-60"),
                           (60, 300, "stall 60-300"), (300, 1e9, "stall > 300")):
            yield nm, d[(d.stall > lo) & (d.stall <= hi)]
        yield "SUSTAINED n60_z>=.5 & wake_z<=0", d[d.sustained]
        yield "SPIKE wake_z>=1", d[d.spike]
        yield "neither", d[~d.sustained & ~d.spike]

    for band, lo, hi in BANDS:
        d = D[(D.v > lo) & (D.v <= hi)]
        if len(d) < 3000:
            continue
        rows = []
        for nm, s in term_bins(d):
            if len(s) < 400:
                continue
            r = dict(slice=nm, n=len(s))
            for ycol, lbl in (("f60", "F60"), ("ftr40", "Ftr40"), ("r60", "R60"),
                              ("rtr40", "Rtr40")):
                ss = s.copy(); ss["y"] = ss[ycol]
                c = cell(ss, days=days)
                r[lbl] = c["pct"]
                r[lbl + "d"] = c["pos"]
            r["nday"] = round(len(s) / days, 1)
            rows.append(r)
        show(rows, "3. RESERVE BAND %s   n=%s   (%%/trade and days positive)"
             % (band, f"{len(d):,}"))

    # ---- 4. the sustained/spike contrast, whole tape, both seats ---------------------------
    rows = []
    for nm, s in (("SUSTAINED", D[D.sustained]), ("SPIKE", D[D.spike]),
                  ("neither", D[~D.sustained & ~D.spike])):
        for ycol, lbl in (("f60", "FOLLOW cap60"), ("r60", "RACE cap60"),
                          ("r300", "RACE cap300")):
            rows.append(dict(exit=lbl, **blk(s, nm, ycol)))
    show(rows, "4. SUSTAINED MINUTE vs TWO-SECOND SPIKE")

    print("\n  mfe60 p50 by slice: sustained %.1f %%  spike %.1f %%  neither %.1f %%"
          % (100 * D.mfe60[D.sustained].median(), 100 * D.mfe60[D.spike].median(),
             100 * D.mfe60[~D.sustained & ~D.spike].median()))

    D.to_parquet(data_file("cvx_hot_sep2.parquet"), index=False)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
