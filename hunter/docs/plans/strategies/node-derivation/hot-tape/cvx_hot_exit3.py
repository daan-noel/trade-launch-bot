"""Hot-tape node, step 13: is the exit slot empty, or is the SEAT eating it?

Step 12 searched 480 exit shapes on their own entries and every one is negative - best -2.38 %
per trade, 0/8 days, against a perfect-exit ceiling of +13 % at 30 s and +63 % at 1800 s. Two
readings survive that, and they have opposite consequences:

  A  THE EXIT IS EMPTY. The convexity is real but only an oracle can reach it, so no exit family
     can pay the toll and the 26-point gap is unharvestable. X is closed and the money, if any,
     is in selection.
  B  THE SEAT ATE IT. Step 12 entered 115 ms BEHIND their print, so it paid their impact and the
     lag on every ticket. Evidence 1.4 prices that at -5.59 pp. Their own book is only +0.70 % on
     spend, so a -5.59 pp seat is more than enough to bury every exit no matter how good.

This run separates them by pricing the SAME shapes at both seats on the same episodes:

  FOLLOW_115  our fill is the last print landed 115 ms after their print. We are behind them.
  RACE        our buy is sequenced BEFORE their print at the same decision moment - entry reserve
              is v_before(their print), which is exactly the state their own decision met. The
              sell leg stays honest: a normal reaction, filled 115 ms after its own trigger.

RACE is not a free lunch and is not shippable by itself - it assumes we decide at the same instant
they do, which is the entry question, not the exit question. It is here as the CEILING ON THE SEAT:
if no shape is positive even at RACE, the exit slot is empty for this node and no amount of speed
fixes it. If shapes turn positive at RACE, then X is alive and everything depends on reaching
their decision point, which is where E goes next.

Their entries are the instrument (7.4 law 20).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import fill_idx, net
from cvx import DAY0, show
from tape import Tape
from cvx_hot_exit2 import CAP, LAG, B, ladder, robust, tell_exit, cumwin


pd.set_option("display.width", 460)
pd.set_option("display.max_rows", 600)
pd.set_option("display.max_columns", 44)

# the shapes worth re-reading: the best of the 480, the extremes of the axis, and the plain
# clocks that any of them has to beat.
SHAPES = {
    "cap15 flat": ([(1e9, 1.0)], 99.0, "cap15", "armed"),
    "cap30 flat": ([(1e9, 1.0)], 99.0, "cap30", "armed"),
    "cap60 flat": ([(1e9, 1.0)], 99.0, "cap60", "armed"),
    "tp30 f.5 tr30 cap15": ([(30.0, 0.5)], 30.0, "cap15", "entry"),
    "tp15 f.5 tr30 cap15": ([(15.0, 0.5)], 30.0, "cap15", "entry"),
    "tp15 f.5 tr40 cap15": ([(15.0, 0.5)], 40.0, "cap15", "entry"),
    "tp15 f.5 tr25 cap15": ([(15.0, 0.5)], 25.0, "cap15", "entry"),
    "tp30 f.5 tr30 cap30": ([(30.0, 0.5)], 30.0, "cap30", "entry"),
    "tp10 f.5 tr25 stop35": ([(10.0, 0.5)], 25.0, "stop35", "entry"),
    "tp10 f.5 tr25 none": ([(10.0, 0.5)], 25.0, "none", "entry"),
    "tr25 only": ([(1e9, 1.0)], 25.0, "none", "entry"),
    "tr40 only": ([(1e9, 1.0)], 40.0, "none", "entry"),
    "10/30/100 tr30 cap30": ([(10.0, 0.34), (30.0, 0.33), (100.0, 0.33)], 30.0, "cap30", "entry"),
}


def prep_seat(t, v, sol, side, k, seat, cap=CAP):
    """The window an exit sees, at one of the two seats.

    FOLLOW: entry index is the last print landed 115 ms after theirs; entry reserve is v there.
    RACE:   entry reserve is v_before(their print) - the state their own decision met - and the
            path starts at their print, which lands on top of us.
    """
    n = len(t)
    if seat == "follow":
        ei = fill_idx(t, k, LAG)
        v0 = float(v[ei])
        s = ei + 1
    else:
        ei = int(k)
        v0 = float(v[k] - sol[k] * (1.0 if side[k] == 1 else -1.0))
        s = ei
    if s >= n:
        return None
    t0 = t[ei]
    m = int(np.searchsorted(t[s:], t0 + cap, side="right"))
    if m <= 0:
        return None
    pr = (v[s:s + m] / v0) ** 2
    return dict(ei=s - 1, v0=v0, t0=t0, pr=pr, tt=t[s:s + m] - t0,
                peak=np.maximum.accumulate(np.maximum(pr, 1.0)), m=m)


def main() -> None:
    t0 = time.time()
    E = pd.read_parquet(data_file("cvx_hot_exit_eps.parquet"))
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    byrun = {}
    for i, r in enumerate(E.run.values):
        byrun.setdefault(int(r), []).append(i)
    print("episodes %s  coins %s  %ds"
          % (f"{len(E):,}", f"{len(byrun):,}", time.time() - t0), flush=True)

    day = E.day.to_numpy()
    for seat in ("follow", "race"):
        books = {nm: [] for nm in SHAPES}
        ceil = []
        for r, idxs in byrun.items():
            a, b = T.start[r], T.end[r]
            t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
            for i in idxs:
                k = int(E.ki.values[i])
                if k >= len(t) - 1:
                    continue
                W = prep_seat(t, v, sol, side, k, seat)
                if W is None:
                    continue
                for nm, (legs, tr, dd, md) in SHAPES.items():
                    y, why = ladder(t, v, W, legs, tr, dd, md)
                    books[nm].append((y, int(day[i]), why))
                ceil.append((net(W["v0"], float(W["pr"].max() ** 0.5 * W["v0"]), B),
                             int(day[i]), "peak"))
        out = []
        for nm in SHAPES:
            d = pd.DataFrame(books[nm], columns=["y", "day", "reason"])
            out.append(robust(d, days, nm))
        d = pd.DataFrame(ceil, columns=["y", "day", "reason"])
        out.append(robust(d, days, "PERFECT EXIT (ceiling, %.0f s)" % CAP))
        show(sorted(out, key=lambda x: -x["pct"]),
             "SEAT = %s   exit shapes on their entries, 0.2 SOL, %.0f s window" % (seat.upper(), CAP))
        print("  %ds" % (time.time() - t0), flush=True)

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
