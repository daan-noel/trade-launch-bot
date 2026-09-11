"""The exit families. Every branch resolves to a print index; the smallest index wins (law 24).

An exit is a dict built by X(): kind, arm (the take profit, or a trail's arm), give (a trail's
give-back), sl (stop), cap (clock, s from our fill), fade / dump (under-water cuts), be (breakeven
once the peak reached it), and for a scale-out the second half's tp2 / cap2 / be2; hf scales the
take profit by the headroom to the graduation wall.

  bracket   take profit at +arm, stop at -sl, clock at cap - the family rule 1 ships
  scale     half at +arm, the other half to tp2 / a breakeven / the stop, clock cap2
  sellbuy   once up >= arm, close on the first public buy >= 1 SOL or +3 % print
  trail     once up >= arm, close on a give-back from the in-hold peak
  ride      once up >= arm, ride while public buying continues (3 s buy SOL >= quiet)

The trip is read on the print; the fill is the last print landed 115 ms after it (kernel.fill_idx).
A multi-leg exit is priced leg by leg (law 25).

  run_exit(sp, t, v, side, sol, mv, cb3, ei) -> (net SOL, exit fill index, reason, hold s)
  outcomes(S, C, exits) -> every row of candidate table C booked under each exit, before
                           occupancy (book.occupy turns it into fires)
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from kernel import B_DEFAULT as B, FEE, FIX, K, LAG, fill_idx, net

WALL = 115.0   # vsol at graduation: the headroom target is (WALL / vsol)^2 - 1


def X(name, kind="bracket", arm=0.10, give=None, sl=0.25, cap=60.0, fade=False, dump=False,
      be=None, tp2=None, cap2=None, be2=False, hf=None, tpmin=0.05, tpmax=0.40, quiet=0.3):
    return dict(name=name, kind=kind, arm=arm, give=give, sl=sl, cap=cap, fade=fade, dump=dump,
                be=be, tp2=tp2, cap2=cap2, be2=be2, hf=hf, tpmin=tpmin, tpmax=tpmax, quiet=quiet)


def net_split(v0, v1, v2, B=B):
    """Buy B at v0, sell half the bag at v1 and half at v2: three legs priced one by one (law 25)."""
    sb = B / (1.0 + FEE)
    tok = K / v0 - K / (v0 + sb)
    o1 = v1 - K / (K / v1 + tok / 2.0)
    o2 = v2 - K / (K / v2 + tok / 2.0)
    return (o1 + o2) * (1.0 - FEE) - B - 3.0 * FIX


def first_trip(sp, t, v, side, sol, mv, cb3, ei, start, cap, v0, tp, sl, be):
    """First tripping print after `start` inside `cap` from our fill; (index, reason) or (None, 'time')."""
    e = int(np.searchsorted(t, t[ei] + cap, side="right"))
    if e <= start + 1:
        return None, "time", e
    sl_ = slice(start + 1, e)
    pr = (v[sl_] / v0) ** 2 - 1.0
    pk = np.maximum(np.maximum.accumulate(pr), float(((v[ei:start + 1] / v0) ** 2 - 1.0).max()))
    trips = []
    if sl is not None:
        trips.append(("sl", pr <= -sl))
    if be is not None:
        trips.append(("be", (pk >= be) & (pr <= 0.0)))
    kind = sp["kind"]
    if kind in ("bracket", "scale") and tp is not None:
        trips.append(("tp", pr >= tp))
    elif kind == "ride":
        armed = pk >= tp
        trips.append(("tp", armed & ((cb3[sl_] < sp["quiet"]) | (pr <= tp / 2.0))))
    elif kind == "sellbuy":
        bigbuy = (side[sl_] == 1) & ((sol[sl_] >= 1.0) | (mv[sl_] >= 3.0))
        trips.append(("tp", bigbuy & (pk >= tp) & (pr > 0)))
    elif kind == "trail":
        trips.append(("tp", (pk >= tp) & ((1.0 + pr) <= (1.0 + pk) * (1.0 - sp["give"]))))
    if sp["fade"]:
        trips.append(("fade", (cb3[sl_] < 0.3) & (pr < 0) & ((t[sl_] - t[ei]) >= 3.0)))
    if sp["dump"]:
        trips.append(("dump", (side[sl_] == -1) & (sol[sl_] >= 1.0) & (pr <= -0.05)))
    best = None
    for why, m in trips:
        h = np.nonzero(m)[0]
        if len(h) and (best is None or h[0] < best[0]):
            best = (int(h[0]), why)
    if best is None:
        return None, "time", e
    return start + 1 + best[0], best[1], e


def run_exit(sp, t, v, side, sol, mv, cb3, ei):
    """Return (net SOL, last exit fill index, reason, hold s) for one fire."""
    n = len(t)
    v0 = float(v[ei])
    tp = sp["arm"]
    if sp.get("hf") is not None:
        tp = min(max(sp["hf"] * ((WALL / v0) ** 2 - 1.0), sp["tpmin"]), sp["tpmax"])
    j, why, e = first_trip(sp, t, v, side, sol, mv, cb3, ei, ei, sp["cap"], v0, tp,
                           sp["sl"], sp["be"])
    if j is None:
        j = max(min(e - 1, n - 1), ei)
    x = fill_idx(t, j, LAG)
    if sp["kind"] != "scale" or why != "tp":
        return net(v0, float(v[x]), B), x, why, t[x] - t[ei]
    j2, why2, e2 = first_trip(dict(sp, kind="bracket", fade=False, dump=False), t, v, side, sol,
                              mv, cb3, ei, j, sp["cap2"], v0, sp["tp2"],
                              None if sp["be2"] else sp["sl"], 0.0 if sp["be2"] else None)
    if j2 is None:
        j2 = max(min(e2 - 1, n - 1), j)
    x2 = fill_idx(t, j2, LAG)
    return net_split(v0, float(v[x]), float(v[x2])), x2, "tp+" + why2, t[x2] - t[ei]


def outcomes(S, C, exits):
    """Every candidate row (run, k, day, t) booked under each exit, from a fill 115 ms after k.
    Only the bracket families are exact here: the tape-reactive kinds read mv and cb3, which
    this helper passes as zeros - book those through run_exit with a facts.Run instead."""
    T = S.T
    rows = []
    for r, g in C.groupby("run", sort=True):
        a, b = T.start[r], T.end[r]
        t = T.t[a:b]; v = T.v[a:b]; side = T.side[a:b]; sol = T.sol[a:b]
        mv = np.zeros(len(t)); cb3 = np.zeros(len(t))
        for k, day, tk in zip(g.k.to_numpy(), g.day.to_numpy(), g.t.to_numpy()):
            ei = fill_idx(t, int(k), LAG)
            for xi, sp in enumerate(exits):
                y, x, why, hold = run_exit(sp, t, v, side, sol, mv, cb3, ei)
                rows.append((xi, r, int(k), int(day), float(tk), y, x, why, float(hold),
                             float(v[ei]), float(v[x])))
    return pd.DataFrame(rows, columns=["exit", "run", "k", "day", "t", "y", "x", "why", "hold",
                                       "v0", "v1"]).sort_values(["exit", "run", "k"])
