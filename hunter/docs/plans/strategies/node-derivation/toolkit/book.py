"""A variant is a set of one-sided cuts over a candidate table; the ledger judges it.

  mask(C, spec)          spec = {column: (">=" or "<=", value)}; a None value is no cut
  occupy(C, m, ...)      one position per coin at a time, re-entry after the exit fill, applied
                         AFTER the mask (a cut candidate frees the coin for a later one, as a live
                         rule would). Optional R terms: cool_sl (no re-entry for N s after a
                         stop-out), max_per_coin
  fires(C, spec, ...)    mask + occupy
  Occupier(run, k, xs)   occupy() for many masks and exits on one table (no R terms), for the
                         ladder: keep(m, e) -> the row indices occupy(C, m) keeps under exit e
  ledger(F, days)        the book: tickets a day, %/trade, SOL, SOL a day, days positive, worst
                         day, body (net without the top 1 % tickets), top 1 % share, biggest
                         coin's share, the two halves of the days, stop-out rate, win rate, and
                         the capped-gain test
  capped(F)              every gain capped at the median take-profit ticket: does the book rest
                         on its gaps?
  reprice(F, b)          each ticket at clip b from its entry and exit reserves (an upper bound:
                         a replay cannot price our buy moving the next prints)
  save / load            a candidate table and its day count under data/
"""
from __future__ import annotations

import json

import numpy as np
import pandas as pd

from kernel import B_DEFAULT as B, net
from .paths import data


def save(C, days, prefix, name):
    C.to_parquet(data("%s_%s.parquet" % (prefix, name)), index=False)
    data("%s_%s.json" % (prefix, name)).write_text(json.dumps({"days": days}))


def load(prefix, name):
    C = pd.read_parquet(data("%s_%s.parquet" % (prefix, name)))
    days = json.loads(data("%s_%s.json" % (prefix, name)).read_text())["days"]
    return C.sort_values(["run", "k"]).reset_index(drop=True), days


def mask(C, spec):
    m = np.ones(len(C), dtype=bool)
    for col, (op, val) in spec.items():
        if val is None:
            continue
        x = C[col].to_numpy()
        m &= (x >= val) if op == ">=" else (x <= val)
    return m


def occupy(C, m, cool_sl=0.0, max_per_coin=None):
    run = C.run.to_numpy(); k = C.k.to_numpy(); x = C.x.to_numpy(); t = C.t.to_numpy()
    why = C.why.to_numpy(); hold = C.hold.to_numpy()
    keep = np.zeros(len(C), dtype=bool)
    cur = -1; last = -1; t_ok = -np.inf; cnt = 0
    for i in np.nonzero(m)[0]:
        if run[i] != cur:
            cur = run[i]; last = -1; t_ok = -np.inf; cnt = 0
        if k[i] <= last or t[i] < t_ok:
            continue
        if max_per_coin is not None and cnt >= max_per_coin:
            continue
        keep[i] = True; last = x[i]; cnt += 1
        if why[i] == "sl" and cool_sl > 0:
            t_ok = t[i] + hold[i] + cool_sl
    return C[keep]


class Occupier:
    """occupy() without R terms, vectorised: rows sorted by (run, k), xs[e] each row's exit fill
    index under exit e. A kept row's next position is the first masked row of its coin after
    its exit fill; the chains are walked for every coin at once."""

    def __init__(self, run, k, xs):
        run = np.asarray(run, dtype=np.int64); k = np.asarray(k, dtype=np.int64)
        self.n = n = len(run); self.run = run; self.ar = np.arange(n)
        key = run * (1 << 32) + k
        self.q = [np.maximum(np.searchsorted(key, run * (1 << 32) + np.asarray(x, dtype=np.int64),
                                             side="right"), self.ar + 1) for x in xs]
        self.cs = np.flatnonzero(np.r_[True, run[1:] != run[:-1]]) if n else np.array([], np.int64)

    def keep(self, m, e):
        n = self.n
        nm = np.full(n + 1, n, dtype=np.int64)
        nm[:n] = np.minimum.accumulate(np.where(m, self.ar, n)[::-1])[::-1]
        cur = nm[self.cs]; ok = cur < n
        cur = cur[ok]; cur = cur[self.run[cur] == self.run[self.cs[ok]]]
        out = []; q = self.q[e]
        while cur.size:
            out.append(cur)
            nx = nm[q[cur]]; ok = nx < n
            src = cur[ok]; nx = nx[ok]
            cur = nx[self.run[nx] == self.run[src]]
        return np.sort(np.concatenate(out)) if out else np.array([], dtype=np.int64)


def fires(C, spec, **r):
    return occupy(C, mask(C, spec), **r)


def capped(F, yc="y"):
    tp = F[F.why == "tp"][yc]
    if not len(tp):
        return dict(cap_sol=np.nan, cap_top1=np.nan)
    y = np.minimum(F[yc].to_numpy(), float(tp.median()))
    s = float(y.sum()); top = float(np.sort(y)[::-1][:max(1, int(round(0.01 * len(y))))].sum())
    return dict(cap_sol=round(s, 2), cap_top1=round(100 * top / s, 1) if s > 0 else np.nan)


def ledger(F, days, b=B, yc="y"):
    if len(F) == 0:
        return dict(n=0, nday=0.0, pct=np.nan, sol=0.0, solday=0.0, pos="0/0")
    y = F[yc]
    s = float(y.sum())
    ag = F.groupby("day")[yc].sum()
    top = float(y.nlargest(max(1, int(round(0.01 * len(F))))).sum())
    dl = np.sort(F.day.unique()); h = (len(dl) + 1) // 2
    return dict(
        n=len(F), nday=round(len(F) / days, 1), pct=round(100 * float(y.mean()) / b, 2),
        sol=round(s, 2), solday=round(s / days, 3),
        pos="%d/%d" % (int((ag > 0).sum()), ag.size), worst=round(float(ag.min()), 2),
        body=round(s - top, 2), top1=round(100 * top / s, 1) if s > 0 else np.nan,
        maxcoin=round(100 * float(F.groupby("run")[yc].sum().max()) / s, 1) if s > 0 else np.nan,
        h1=round(100 * float(F[F.day.isin(dl[:h])][yc].mean()) / b, 2),
        h2=round(100 * float(F[F.day.isin(dl[h:])][yc].mean()) / b, 2),
        sl=round(100 * float((F.why == "sl").mean()), 1), win=round(100 * float((y > 0).mean()), 1),
        **capped(F, yc))


def reprice(F, b):
    return np.array([net(v0, v1, b) for v0, v1 in zip(F.v0.to_numpy(), F.v1.to_numpy())])
