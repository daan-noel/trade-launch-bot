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
                           (115 ms later) lands AHEAD of its buy; booked at that seat (cap 15 / 60).
                           A 5.3 diagnostic (dt, ahead, a clock), never the 5.2 veto
  leftover(S, w, E, trigger, max_trig, horizons)
                           derive 5.2, leftover existence. Acted tickets: the latest trigger print
                           <= max_trig s before each of its decisions. Ignored tickets: trigger
                           prints on the same coins it does not buy within max_trig s (up to
                           n_ctrl per acted ticket per coin). Per ticket, from our fill 115 ms
                           after the trigger print:
                             cost     % price move from the trigger print to our fill
                             missed   our fill lands at or after its closing sell (acted only)
                             peak_h   best net % of any exit decided inside h s, filled 115 ms
                                      later, both legs' cost paid (a ceiling, law 23)
                             up       the path reaches break-even (+cost) before the mirror
                                      loss (-cost, log-symmetric in price) inside the last
                                      horizon: 1 / 0, NaN when it reaches neither. A driftless
                                      price reads 1 / (r + 1), about 0.49, whatever its
                                      volatility (`null` per ticket)
                           horizons default to its hold p10 / p50 / p90
  leftover_summary(L)      acted, acted BEHIND it (our fill after its buy: its own fill is in
                           the price, so no copied fill is counted - the row the veto reads) and
                           ignored: medians, shares, and the within-coin excess (acted minus
                           ignored per coin, weighted by acted tickets, a coin bootstrap p5..p95)
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from kernel import B_DEFAULT as B, FEE, FIX, K, LAG, fill_idx, net
from .facts import Run


def _pct(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return ((a / b) ** 2 - 1.0) * 100.0


def episodes(S, wallet_id):
    T = S.T
    rows = []
    for r in np.unique(T.run_of[T.wallet == wallet_id]):
        r = int(r)
        if not np.isfinite(S.c_s[r]):   # no print-count floor: it reads the coin's future
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


def _break_even(v0, b=B):
    """The reserve at which selling the bag bought with b at v0 returns b (net 0): the root of
    kernel.net(v0, v1) = 0 in v1, closed form."""
    a = (K / v0 - K / (v0 + b / (1.0 + FEE))) / K
    c = b + 2.0 * FIX
    q = 1.0 - FEE
    return (c * a + np.sqrt((c * a) ** 2 + 4.0 * q * a * c)) / (2.0 * q * a)


def _ticket(R, bj, hz, b):
    t, v = R.t, R.v
    ei = fill_idx(t, bj, LAG)
    v0 = float(v[ei])
    cost = ((v0 / float(v[bj])) ** 2 - 1.0) * 100.0
    peaks = []
    hi = ei
    for h in hz:
        hi = max(int(np.searchsorted(t, t[ei] + h, side="right") - 1), ei)
        if hi > ei:
            j = np.arange(ei + 1, hi + 1)
            x = np.maximum(j, np.searchsorted(t, t[j] + LAG, side="right") - 1)
            vmax = float(v[x].max())
        else:
            vmax = v0
        peaks.append(100.0 * net(v0, max(vmax, v0), b) / b)
    up = np.nan
    vu = _break_even(v0, b); vd = v0 * v0 / vu
    null = 1.0 / ((vu / v0) ** 2 + 1.0)
    if hi > ei:
        path = v[ei + 1:hi + 1]
        iu = np.nonzero(path >= vu)[0]; id_ = np.nonzero(path <= vd)[0]
        if len(iu) or len(id_):
            up = float(len(iu) > 0 and (not len(id_) or iu[0] < id_[0]))
    return ei, cost, peaks, up, null


def leftover(S, w, E, trigger, max_trig=0.3, horizons=None, n_ctrl=4, seed=20260911, b=B):
    hz = tuple(horizons) if horizons is not None else         tuple(float(x) for x in np.nanquantile(E.held.to_numpy(), (0.1, 0.5, 0.9)))
    rng = np.random.default_rng(seed)
    rows = []
    for r, g in E.groupby("run"):
        R = Run(S, int(r))
        tj = np.nonzero(trigger(R))[0]
        if not len(tj):
            continue
        bt = R.tm[(R.wal == w) & (R.side == 1)]
        acted = {}
        for k, ks in zip(g.k.to_numpy(), g.ks.to_numpy()):
            k = int(k)
            p = int(np.searchsorted(tj, k, side="left")) - 1
            if p < 0 or R.tm[k] - R.tm[tj[p]] > max_trig:
                continue
            acted.setdefault(int(tj[p]), (k, int(ks)))
        if not acted:
            continue
        q = np.searchsorted(bt, R.tm[tj], side="left")
        hit = (q < len(bt)) & (bt[np.minimum(q, len(bt) - 1)] - R.tm[tj] <= max_trig)
        ign = tj[~hit & ~np.isin(tj, list(acted))]
        if len(ign) > n_ctrl * len(acted):
            ign = np.sort(rng.choice(ign, size=n_ctrl * len(acted), replace=False))
        for bj, (k, ks) in acted.items():
            ei, cost, peaks, up, null = _ticket(R, bj, hz, b)
            rows.append((int(r), int(R.day[bj]), bj, 1, float(R.tm[k] - R.tm[bj]), float(ei < k),
                         float(ks >= 0 and ei >= ks), cost, *peaks, up, null))
        for bj in ign:
            ei, cost, peaks, up, null = _ticket(R, int(bj), hz, b)
            rows.append((int(r), int(R.day[bj]), int(bj), 0, np.nan, np.nan, np.nan, cost,
                         *peaks, up, null))
    cols = ["run", "day", "k", "acted", "dt", "ahead", "missed", "cost"] +         ["peak%d" % i for i in range(len(hz))] + ["up", "null"]
    L = pd.DataFrame(rows, columns=cols)
    L.attrs["horizons"] = hz
    return L


def _coin_excess(L, col, n_boot=2000, seed=20260911):
    d = L[np.isfinite(L[col])]
    g = d.groupby(["run", "acted"])[col].mean().unstack()
    n = d[d.acted == 1].groupby("run").size()
    g = g.dropna()
    if g.empty or 1 not in g or 0 not in g:
        return np.nan, np.nan, np.nan
    diff = (g[1] - g[0]).to_numpy(); wt = n.reindex(g.index).to_numpy().astype(float)
    est = float((diff * wt).sum() / wt.sum())
    rng = np.random.default_rng(seed)
    ix = rng.integers(0, len(diff), size=(n_boot, len(diff)))
    bs = (diff[ix] * wt[ix]).sum(1) / wt[ix].sum(1)
    return est, float(np.quantile(bs, 0.05)), float(np.quantile(bs, 0.95))


def leftover_summary(L):
    hz = L.attrs.get("horizons", ())
    mid = "peak%d" % (len(hz) // 2)
    out = {}
    for nm, d in (("acted", L[L.acted == 1]), ("behind", L[(L.acted == 1) & (L.ahead == 0)]),
                  ("ignored", L[L.acted == 0])):
        u = d.up.dropna()
        a = nm != "ignored"
        out[nm] = dict(
            n=len(d), coins=d.run.nunique(),
            dt_p50_ms=round(1000 * float(d.dt.median()), 0) if a else np.nan,
            ahead=round(100 * float(d.ahead.mean()), 1) if a else np.nan,
            missed=round(100 * float(d.missed.mean()), 1) if a else np.nan,
            cost_p50=round(float(d.cost.median()), 2),
            cost_ge2=round(100 * float((d.cost >= 2.0).mean()), 1),
            **{"peak_p50_h%d" % i: round(float(d["peak%d" % i].median()), 2)
               for i in range(len(hz))},
            covers=round(100 * float((d[mid] > 0).mean()), 1),
            up=round(100 * float(u.mean()), 1) if len(u) else np.nan,
            null=round(100 * float(d.null[d.up.notna()].mean()), 1) if len(u) else np.nan,
            unresolved=round(100 * float(d.up.isna().mean()), 1))
    T = pd.DataFrame(out).T
    ex = {c: _coin_excess(L, c) for c in (mid, "up", "cost")}
    X = pd.DataFrame({c: dict(excess=round(e * (100 if c == "up" else 1), 2),
                              p5=round(lo * (100 if c == "up" else 1), 2),
                              p95=round(hi * (100 if c == "up" else 1), 2))
                      for c, (e, lo, hi) in ex.items()}).T
    return T, X
