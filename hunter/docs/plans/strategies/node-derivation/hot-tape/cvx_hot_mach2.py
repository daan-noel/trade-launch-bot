"""Hot-tape node, step 20: the independent-machine count as an EVENT, scored on money.

Step 19 put the unpriced axis into this node's event for the first time and found the contrast is
between the two members, not between them and the tape:

  within the coin, the two that pay against the four that do not
      nb5    distinct builds printing in the last 5 s     0.586 HIGH   (median 10 against 7)
      nb20                       in the last 20 s         0.565 HIGH   (22 against 17)
      newb20 builds NEW to this coin in that window       0.558 HIGH   (5 against 3)
      hhi20  concentration of prints across builds        0.443 low    (0.167 against 0.187)
      pro20  professional builds printing                 0.554 HIGH

The two that pay buy into BROAD, many-machine activity; the four that lose buy into narrow,
few-machine activity. That is exactly the term strategy 1.4 names as unpriced - "how many
INDEPENDENT machines are acting" - and it has never been scored for money on this node.

Lift is not money (7.4 law 11), so this run scores it: the count as a standing condition on the
full tape, one position per coin at a time, our own fill, both exit families.

  D  none                                   S 0.2      R one per coin, unlimited
  E  nb5 / nb20 distinct builds over a threshold, alone and with the price state beside it
  P  age >= 60 s . reserve band
  X  cap15 . cap60 . cap300 . trail40 c600
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import os
import time
from collections import defaultdict
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

# (name, nb5 min, nb20 min, hhi20 max, mv60 min, dd min, stall max)
GATES = [
    ("nb5>=8",              8,  0, 1.1, -1e9, -1e9, 1e9),
    ("nb5>=12",            12,  0, 1.1, -1e9, -1e9, 1e9),
    ("nb5>=16",            16,  0, 1.1, -1e9, -1e9, 1e9),
    ("nb20>=25",            0, 25, 1.1, -1e9, -1e9, 1e9),
    ("nb20>=40",            0, 40, 1.1, -1e9, -1e9, 1e9),
    ("nb5>=12 hhi<=.15",   12,  0, 0.15, -1e9, -1e9, 1e9),
    ("nb5>=12 mv60>=10",   12,  0, 1.1, 10.0, -1e9, 1e9),
    ("nb5>=12 mv60>=25 dd>=-25", 12, 0, 1.1, 25.0, -25.0, 1e9),
    ("nb5>=12 st<=20",     12,  0, 1.1, -1e9, -1e9, 20.0),
    ("nb5>=16 hhi<=.15 mv60>=10", 16, 0, 0.15, 10.0, -1e9, 1e9),
    ("nb5<=4 (control)",    0,  0, 1.1, -1e9, -1e9, 1e9),
]
EXITS = (("cap15", 15.0, None), ("cap60", 60.0, None), ("cap300", 300.0, None),
         ("tr40_c600", 600.0, 40.0))


def pct(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return np.where(b > 0, (a / b) ** 2 - 1.0, np.nan) * 100.0


def distinct_window(tm, bu, win):
    """Distinct builds among prints in [tm[i] - win, tm[i]). Two-pointer, O(n)."""
    n = len(tm)
    out = np.zeros(n, dtype=np.int32)
    cnt = defaultdict(int)
    dis = 0
    lo = 0
    hi = 0                                   # prints [lo, hi) are in the window
    for i in range(n):
        while hi < i:
            b = bu[hi]
            if cnt[b] == 0:
                dis += 1
            cnt[b] += 1
            hi += 1
        limit = tm[i] - win
        while lo < hi and tm[lo] < limit:
            b = bu[lo]
            cnt[b] -= 1
            if cnt[b] == 0:
                dis -= 1
            lo += 1
        out[i] = dis
    return out


def hhi_window(tm, bu, win, nb20):
    """Herfindahl of the build mix in the last `win` seconds, from prints 0..i-1."""
    n = len(tm)
    out = np.full(n, np.nan)
    cnt = defaultdict(int)
    ss = 0.0                                  # sum of squares of the counts
    tot = 0
    lo = 0
    hi = 0
    for i in range(n):
        while hi < i:
            b = bu[hi]
            c = cnt[b]
            ss += 2 * c + 1
            cnt[b] = c + 1
            tot += 1
            hi += 1
        limit = tm[i] - win
        while lo < hi and tm[lo] < limit:
            b = bu[lo]
            c = cnt[b]
            ss -= 2 * c - 1
            cnt[b] = c - 1
            tot -= 1
            lo += 1
        if tot > 0:
            out[i] = ss / (tot * tot)
    return out


def state(t, v, age, bu):
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
    nb5 = distinct_window(tm, bu, 5.0)
    nb20 = distinct_window(tm, bu, 20.0)
    hhi = hhi_window(tm, bu, 20.0, nb20)
    return dict(v=vprev, age=age, mv60=pct(vprev, vb(j60)), stall=stall,
                dd=pct(vprev, peak), nb5=nb5.astype(np.float64), nb20=nb20.astype(np.float64),
                hhi=hhi, n60=(i - j60).astype(np.float64))


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
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    nruns = len(T.start)
    lim = int(os.environ.get("HOTM_LIMIT", "0"))
    rng = range(nruns if not lim else min(lim, nruns))
    print("tape %s prints  coins %s  %.2f days  %ds"
          % (f"{T.n:,}", f"{nruns:,}", days, time.time() - t0), flush=True)

    rows = []
    for r in rng:
        a, b = T.start[r], T.end[r]
        if b - a < 60 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; bu = T.build[a:b]
        age = t - c_s[r]
        if age[-1] < 60.0:
            continue
        S = state(t, v, age, bu)
        base = np.isfinite(S["dd"]) & (age >= 60.0) & (S["v"] >= 42.43) & (S["v"] <= 85.0)
        if not base.any():
            continue
        for gi, (gn, n5, n20, hh, mv, dd, st) in enumerate(GATES):
            m = base & (S["nb5"] >= n5) & (S["nb20"] >= n20) & (S["hhi"] <= hh) \
                & (S["mv60"] >= mv) & (S["dd"] >= dd) & (S["stall"] <= st)
            if gn.startswith("nb5<=4"):
                m = base & (S["nb5"] <= 4)
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
                             float(S["nb5"][k]), float(S["nb20"][k]), float(S["hhi"][k]),
                             float(S["mv60"][k]),
                             y["cap15"], y["cap60"], y["cap300"], y["tr40_c600"]))
        if r % 20000 == 0:
            print("  run %s  fires %s  %ds" % (f"{r:,}", f"{len(rows):,}", time.time() - t0),
                  flush=True)

    F = pd.DataFrame(rows, columns=["run", "day", "gi", "v", "age", "nb5", "nb20", "hhi",
                                    "mv60", "y_cap15", "y_cap60", "y_cap300", "y_tr40"])
    F.to_parquet(data_file("cvx_hot_mach2.parquet"), index=False)
    print("\nfires %s over %s coins  %ds"
          % (f"{len(F):,}", f"{F.run.nunique():,}", time.time() - t0), flush=True)

    rows2 = []
    for gi, g in enumerate(GATES):
        s = F[F.gi == gi]
        if len(s) < 400:
            continue
        for ycol, lbl in (("y_cap15", "cap15"), ("y_cap60", "cap60"),
                          ("y_cap300", "cap300"), ("y_tr40", "trail40")):
            dd = s.copy(); dd["y"] = dd[ycol]
            tot = dd.y.sum()
            top = dd.y.nlargest(max(1, int(round(0.01 * len(dd))))).sum()
            c = cell(dd, days=days)
            c["top1"] = round(100 * top / tot, 1) if tot > 0 else np.nan
            c["wo_top1"] = round(tot - top, 2)
            rows2.append(dict(gate=g[0], exit=lbl, **c))
    show(rows2, "the independent-machine count as an event, full tape, age >= 60 s, v 42-85")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
