"""A member's own decisions, and where our seat lands against them.

  episodes(S, wallet_id)   one row per position the wallet opens on a coin: the first buy while
                           flat (k, the decision print) to the sell that takes it to <= 2 % of its
                           peak size (ks). pnl / peak off the curve from the reserve its first buy
                           left; held in s. A position still open at the tape's end has ks = -1
  seat_book(S, E, caps)    each episode booked at two seats under a clock:
                             FOLLOW  our fill is the last print landed 115 ms after its print
                             RACE    we are sequenced BEFORE its print and meet the reserve it met
                           A member that pays only at RACE is reacting to something we must
                           reach before it; one that pays at FOLLOW can be followed
  reaction(S, E, trigger, max_trig)
                           for each episode: the latest trigger print within max_trig s before its
                           buy, its lag from that print (dt), and whether our fill on that trigger
                           (115 ms later) lands AHEAD of its buy; booked at that seat (cap 15 / 60)
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from kernel import B_DEFAULT as B, LAG, fill_idx, net
from .facts import Run


def _pct(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return ((a / b) ** 2 - 1.0) * 100.0


def episodes(S, wallet_id):
    T = S.T
    rows = []
    for r in np.unique(T.run_of[T.wallet == wallet_id]):
        r = int(r)
        if T.end[r] - T.start[r] < 30 or not np.isfinite(S.c_s[r]):
            continue
        R = Run(S, r)
        p = 0.0; peak = 0.0; e0 = -1
        for s in np.nonzero(R.wal == wallet_id)[0]:
            s = int(s)
            if R.side[s] == 1:
                if p <= 0.0:
                    e0 = s; peak = 0.0
                p += R.tokd[s]; peak = max(peak, p)
                continue
            if p <= 0.0 or e0 < 0:
                continue
            p = max(p - R.tokd[s], 0.0)
            if p > 0.02 * peak:
                continue
            p = 0.0
            v0 = float(R.v[e0])
            rows.append((r, e0, s, int(R.day[e0]), float(R.age[e0]), float(R.v[e0]),
                         float(_pct(R.v[s - 1], v0)), float(_pct(R.v[e0:s].max(), v0)),
                         float(R.tm[s] - R.tm[e0])))
            e0 = -1
        if e0 >= 0 and p > 0.0:
            rows.append((r, e0, -1, int(R.day[e0]), float(R.age[e0]), float(R.v[e0]),
                         np.nan, np.nan, np.nan))
    return pd.DataFrame(rows, columns=["run", "k", "ks", "day", "age", "v", "pnl", "peak", "held"])


def _clock(t, v, s, v0, cap):
    j = max(int(np.searchsorted(t, t[s] + cap, side="right") - 1), s)
    return net(v0, float(v[fill_idx(t, j, LAG)]), B)


def seat_book(S, E, caps=(15.0, 60.0)):
    T = S.T
    out = np.full((len(E), 2 * len(caps)), np.nan)
    for r, g in E.groupby("run"):
        a, b = T.start[r], T.end[r]
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        for i, k in zip(E.index.get_indexer(g.index), g.k.to_numpy()):
            k = int(k)
            ef = fill_idx(t, k, LAG)
            out[i] = [_clock(t, v, ef, float(v[ef]), c) for c in caps] + \
                     [_clock(t, v, k, float(vbef[k]), c) for c in caps]
    cols = ["follow%d" % c for c in caps] + ["race%d" % c for c in caps]
    return E.assign(**{c: out[:, j] for j, c in enumerate(cols)})


def reaction(S, E, trigger, max_trig=1.0):
    T = S.T
    out = np.full((len(E), 6), np.nan)
    for r, g in E.groupby("run"):
        R = Run(S, int(r))
        bidx = np.nonzero(trigger(R))[0]
        if not len(bidx):
            continue
        for i, k in zip(E.index.get_indexer(g.index), g.k.to_numpy()):
            k = int(k)
            p = int(np.searchsorted(bidx, k, side="left")) - 1
            if p < 0:
                continue
            bj = int(bidx[p])
            dt = float(R.tm[k] - R.tm[bj])
            if dt > max_trig:
                continue
            ei = fill_idx(R.t, bj, LAG)
            out[i] = (dt, bj, float(ei < k), float(R.tm[ei] - R.tm[bj]),
                      _clock(R.t, R.v, ei, float(R.v[ei]), 15.0),
                      _clock(R.t, R.v, ei, float(R.v[ei]), 60.0))
    cols = ["dt", "trig_k", "ahead", "our_lag", "y15", "y60"]
    return E.assign(**{c: out[:, j] for j, c in enumerate(cols)})
