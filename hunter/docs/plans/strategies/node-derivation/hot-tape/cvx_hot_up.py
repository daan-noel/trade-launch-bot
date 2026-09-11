"""Hot-tape node, step 18: THE UP-MOVE EVENT, derived from the two members that pay at our seat.

WHERE THIS COMES FROM. Splitting the node by member (7.4 law 27) separates two animals:

  wallet    FOLLOW cap60   RACE cap60   clip   hold   stall   dd_peak   mv60   n60   age
  8fStGV      +0.54 %       +2.43 %     0.30   10 s    4.6 s   -16.7 %   high   213   95 s
  AbQcLH      -1.12 %       +2.00 %     0.49   19 s   49.6 s   -32.9 %          103  189 s
  the other four            -0.5..-1.5 %                                         ~85  350 s

The two that pay buy a coin that is UP - median mv60 +22.3 % against +9.8 % for the other four -
that made a new high seconds ago (stall 24 s against 63 s), on a busy tape (n60 141 against 106,
sol60 73 against 45 SOL), only a little off its peak (dd_peak -24.7 % against -32.0 %), and at the
moment the last five seconds turn net SELL (net5 -0.57 against +0.49). That is a **pullback inside
a live up-move**, not the deep dip-buy the pooled study described - and the pooled study read the
opposite sign on dd_peak precisely because the four deep-dip members outnumber the two by 8 to 1.

So the event this run scores is the user's own description of the node, spelled in public tape
state with no wallet in it:

  E   the coin is up m % over the last 60 s, made a new high within s seconds, has given back no
      more than g % from that high, and the tape is busy - and (optionally) the last few seconds
      have turned down. Fire on any print at which that state holds, one position per coin at a
      time, so the standing condition is a STATE that has been true for seconds rather than a
      reaction to a print (7.2).

  P   reserve band (headroom) . age band
  X   cap15 . cap60 . cap300 . trail40 c600, side by side
  R   one position per coin, unlimited re-entry        S 0.2
  seat  fill = last print landed by the fire + 115 ms, both legs

WHAT WOULD KILL IT. The same thing that killed the level rule in step 17: a door or a feature that
is not knowable at the fire. Every term here is built from prints 0..i-1 on that coin, and the
`live60` door of that run is deliberately NOT used - it is only knowable at age 60 s and 15 % of
its fires were younger than that, which is where all of its money was.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import os
import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape

B = 0.2
LAG = 0.115

pd.set_option("display.width", 480)
pd.set_option("display.max_rows", 900)
pd.set_option("display.max_columns", 60)

# (name, mv60 min, stall max, dd_peak min, n60 min, require last-5s pullback)
GATES = [
    ("m10",              10.0, 1e9, -1e9,  0, False),
    ("m25",              25.0, 1e9, -1e9,  0, False),
    ("m25 st20",         25.0, 20.0, -1e9,  0, False),
    ("m25 st20 dd25",    25.0, 20.0, -25.0,  0, False),
    ("m25 st20 dd25 n50", 25.0, 20.0, -25.0, 50, False),
    ("m25 st20 dd25 n150", 25.0, 20.0, -25.0, 150, False),
    ("m25 st20 dd25 n50 pull", 25.0, 20.0, -25.0, 50, True),
    ("m50 st20 dd25 n50", 50.0, 20.0, -25.0, 50, False),
    ("m10 st20 dd10 n50", 10.0, 20.0, -10.0, 50, False),
    ("m25 st5 dd15 n150", 25.0, 5.0, -15.0, 150, False),
    ("m25 st60 dd40 n50", 25.0, 60.0, -40.0, 50, False),
    ("m0 st20 dd25 n50", 0.0, 20.0, -25.0, 50, False),
]
EXITS = (("cap15", 15.0, None), ("cap60", 60.0, None), ("cap300", 300.0, None),
         ("tr40_c600", 600.0, 40.0))


def pct(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return np.where(b > 0, (a / b) ** 2 - 1.0, np.nan) * 100.0


def state(t, v, age):
    """Every term from prints 0..i-1 and the reserve v[i-1]. Nothing at or after print i."""
    n = len(t)
    i = np.arange(n)
    vprev = np.concatenate(([np.nan], v[:-1]))
    tm = np.maximum.accumulate(t)
    j5 = np.searchsorted(tm, tm - 5.0, side="left")
    j60 = np.searchsorted(tm, tm - 60.0, side="left")
    vb = lambda jj: vprev[np.maximum(jj, 1)]

    rmax = np.maximum.accumulate(v)
    newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
    t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
    stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
    peak = np.concatenate(([np.nan], rmax[:-1]))

    cnt = np.arange(n, dtype=np.float64)
    n60 = cnt[i] - j60.astype(np.float64)
    return dict(v=vprev, age=age, mv5=pct(vprev, vb(j5)), mv60=pct(vprev, vb(j60)),
                stall=stall, dd=pct(vprev, peak), n60=n60)


def book_from(t, v, k, cap, trail=None):
    ei = fill_idx(t, k, LAG)
    v0 = float(v[ei])
    n = len(t)
    e = int(np.searchsorted(t, t[ei] + cap, side="right"))
    if trail is not None and e > ei + 1:
        pr = (v[ei + 1:e] / v0) ** 2
        pk = np.maximum.accumulate(np.maximum(pr, 1.0))
        hit = np.nonzero(pr <= pk * (1.0 - trail / 100.0))[0]
        if len(hit):
            x = fill_idx(t, ei + 1 + int(hit[0]), LAG)
            return net(v0, float(v[x]), B), x
    j = max(min(e - 1, n - 1), ei)
    x = fill_idx(t, j, LAG)
    return net(v0, float(v[x]), B), x


def main() -> None:
    t0 = time.time()
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    nruns = len(T.start)
    lim = int(os.environ.get("HOTUP_LIMIT", "0"))
    rng = range(nruns if not lim else min(lim, nruns))
    print("tape %s prints  coins %s  %.2f days  %ds"
          % (f"{T.n:,}", f"{nruns:,}", days, time.time() - t0), flush=True)

    rows = []
    for r in rng:
        a, b = T.start[r], T.end[r]
        if b - a < 40 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]
        age = t - c_s[r]
        S = state(t, v, age)
        ok0 = np.isfinite(S["mv60"]) & np.isfinite(S["dd"]) & (age >= 60.0)
        if not ok0.any():
            continue
        for gi, (gname, mv, st, dd, nn, pull) in enumerate(GATES):
            m = ok0 & (S["mv60"] >= mv) & (S["stall"] <= st) & (S["dd"] >= dd) & (S["n60"] >= nn)
            if pull:
                m &= S["mv5"] <= 0.0
            cand = np.nonzero(m)[0]
            if not len(cand):
                continue
            last = -1
            for k in cand:
                k = int(k)
                if k <= last:
                    continue
                y = {}
                xi = 0
                for nm, cap, tr in EXITS:
                    yy, xx = book_from(t, v, k, cap, tr)
                    y[nm] = yy
                    if nm == "cap60":
                        xi = xx
                last = xi
                rows.append((r, int(T.day[a + k]), gi, float(S["v"][k]), float(age[k]),
                             float(S["mv60"][k]), float(S["stall"][k]), float(S["dd"][k]),
                             float(S["n60"][k]),
                             y["cap15"], y["cap60"], y["cap300"], y["tr40_c600"]))
        if r % 20000 == 0:
            print("  run %s  fires %s  %ds" % (f"{r:,}", f"{len(rows):,}", time.time() - t0),
                  flush=True)

    F = pd.DataFrame(rows, columns=["run", "day", "gi", "v", "age", "mv60", "stall", "dd",
                                    "n60", "y_cap15", "y_cap60", "y_cap300", "y_tr40"])
    F.to_parquet(data_file("cvx_hot_up.parquet"), index=False)
    print("\nfires %s over %s coins  %ds"
          % (f"{len(F):,}", f"{F.run.nunique():,}", time.time() - t0), flush=True)

    def rep(d, **kw):
        out = []
        for ycol, lbl in (("y_cap15", "cap15"), ("y_cap60", "cap60"),
                          ("y_cap300", "cap300"), ("y_tr40", "trail40")):
            dd = d.copy(); dd["y"] = dd[ycol]
            s = dd.y.sum()
            top = dd.y.nlargest(max(1, int(round(0.01 * len(dd))))).sum()
            c = cell(dd, days=days)
            c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
            c["wo_top1"] = round(s - top, 2)
            out.append(dict(exit=lbl, **kw, **c))
        return out

    rows2 = []
    for gi, g in enumerate(GATES):
        s = F[F.gi == gi]
        if len(s) < 400:
            continue
        rows2 += rep(s, gate=g[0])
    show(rows2, "1. THE UP-MOVE EVENT, full tape, age >= 60 s, no door")

    print("\n=== 2. the best gates inside a reserve band")
    rows3 = []
    for gi, g in enumerate(GATES):
        s = F[F.gi == gi]
        if len(s) < 400:
            continue
        for nm, m in (("v 42-85", (s.v >= 42.43) & (s.v <= 85)),
                      ("v <= 60", s.v <= 60), ("v 60-85", (s.v > 60) & (s.v <= 85)),
                      ("v > 85", s.v > 85)):
            ss = s[m]
            if len(ss) < 400:
                continue
            for ycol, lbl in (("y_cap15", "cap15"), ("y_cap60", "cap60")):
                dd = ss.copy(); dd["y"] = dd[ycol]
                rows3.append(dict(gate=g[0], band=nm, exit=lbl, **cell(dd, days=days)))
    show(rows3, "2. reserve band")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
