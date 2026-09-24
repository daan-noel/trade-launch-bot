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
  control(S, w, E, frame)  derive 5.2's control: the same read on RANDOM public buys of the same
                           coins, in the same frame, at the same seat. Peak leftover is the best
                           price inside a hold, so it is positive on most prints of any class; a
                           pass no higher than this control says nothing about the class. Pass it
                           to leftover_summary / veto, which flag a pass without one as thin
  race_split(S, F, lag)    derive 5.4: how many public prints land inside our lag after each fire,
                           and the same fire booked at 0 ms and at our seat, split into still /
                           1 / 2+ windows. Answered fires that carry the book at 0 ms and lose at
                           our seat are those bots' own buying landing before we do: move the
                           event, do not re-spell it
  leftover_summary(L, C)   acted, acted BEHIND it (our fill after its buy: its own fill is in
                           the price, so no copied fill is counted - the row the veto reads) and
                           ignored: medians, shares, and the within-coin excess (acted minus
                           ignored per coin, weighted by acted tickets, a coin bootstrap p5..p95)
  veto(L, n_decisions)     derive 5.2's verdict on a leftover table, the ONE reader: a study
                           script calls it and never re-spells the lines. kill when the behind
                           row's median peak leftover <= 0 or missed >= 50 %, or a race (acted
                           lag p50 <= 50 ms and ahead < 10 %); a corner when the class covers
                           < 10 % of its decisions (derive 5.1); else PASS, flagged thin under
                           THIN_PEAK. Cost is reported and decides nothing
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


def control(S, w, E, horizons=None, n_per=4, seed=20260918, b=B, frame=None, max_trig=0.3):
    """Derive 5.2's control: the same leftover read on RANDOM public buys of the same coins, in the
    same frame, at the same seat.

    Peak leftover is the best price reached inside a hold, so it is positive on most prints of any
    class: a pass that is no higher than this control says nothing about the class. `frame(R)`
    returns the mask a study fires inside (its age band, its t_min); prints inside `max_trig` before
    one of his buys are left out, so the control cannot contain his own triggers.

    The table it returns has the columns of `leftover` with `acted = -1`, so passing it to
    `leftover_summary` or `veto` adds the control row and nothing else changes."""
    hz = tuple(horizons) if horizons is not None else \
        tuple(float(x) for x in np.nanquantile(E.held.to_numpy(), (0.1, 0.5, 0.9)))
    rng = np.random.default_rng(seed)
    rows = []
    for r in np.unique(E.run.to_numpy()):
        R = Run(S, int(r))
        m = R.pub & (R.side == 1) & (R.tm >= S.t_min) & (R.tm < S.t_max)
        if frame is not None:
            m &= frame(R)
        bt = R.tm[(R.wal == w) & (R.side == 1)]
        cand = np.nonzero(m)[0]
        if len(bt) and len(cand):
            q = np.searchsorted(bt, R.tm[cand], side="left")
            near = (q < len(bt)) & (bt[np.minimum(q, len(bt) - 1)] - R.tm[cand] <= max_trig)
            cand = cand[~near]
        if not len(cand):
            continue
        if len(cand) > n_per:
            cand = np.sort(rng.choice(cand, size=n_per, replace=False))
        for bj in cand:
            ei, cost, peaks, up, null = _ticket(R, int(bj), hz, b)
            rows.append((int(r), int(R.day[bj]), int(bj), -1, np.nan, np.nan, np.nan, cost,
                         *peaks, up, null))
    cols = ["run", "day", "k", "acted", "dt", "ahead", "missed", "cost"] + \
        ["peak%d" % i for i in range(len(hz))] + ["up", "null"]
    C = pd.DataFrame(rows, columns=cols)
    C.attrs["horizons"] = hz
    return C


RACE_BINS = ("still (no print answers it)", "1 print answers", "2+ prints answer")


def race_split(S, F, lag=LAG, cap=15.0, b=B):
    """Derive 5.4's first read: WHO lands inside our lag, and what the fire is worth without them.

    For every fire, count the public prints that land in the `lag` after it and the price move they
    make, then book the same fire twice: at the fire's own reserve (0 ms, a ceiling) and at our seat.
    Read the three rows against each other:

      the answered fires carry the book at 0 ms and lose at our seat  -> the money is those bots'
        own buying and it lands before we do. Do not re-spell this event; move it (5.4)
      the still fires lose even at 0 ms                                -> nobody was coming; the
        event is not naming a decision at all

    `F` needs `run` and `k`. Returns one row a bin, and the per-fire table beside it."""
    T = S.T
    out = np.full((len(F), 4), np.nan)
    for r, g in F.groupby("run"):
        a, e = T.start[int(r)], T.end[int(r)]
        t = T.t[a:e]; v = T.v[a:e]
        pub = ~S.is_node[a:e]
        for i, k in zip(F.index.get_indexer(g.index), g.k.to_numpy()):
            k = int(k)
            j = int(np.searchsorted(t, t[k] + lag, side="right"))
            n83 = int(pub[k + 1:j].sum()) if j > k + 1 else 0
            ei = fill_idx(t, k, lag)
            out[i] = (n83, ((float(v[ei]) / float(v[k])) ** 2 - 1.0) * 100.0,
                      _clock(t, v, k, float(v[k]), cap), _clock(t, v, ei, float(v[ei]), cap))
    D = F.assign(n_in_lag=out[:, 0], move_in_lag=out[:, 1], y0=out[:, 2], y_seat=out[:, 3])
    bin_ = np.where(D.n_in_lag == 0, 0, np.where(D.n_in_lag == 1, 1, 2))
    rows = []
    for i, nm in enumerate(RACE_BINS):
        d = D[bin_ == i]
        if d.empty:
            continue
        rows.append(dict(window=nm, n=len(d), share=round(100.0 * len(d) / len(D), 1),
                         move_in_lag_p50=round(float(d.move_in_lag.median()), 2),
                         pct_at_0ms=round(100.0 * float(d.y0.mean()) / b, 2),
                         pct_at_seat=round(100.0 * float(d.y_seat.mean()) / b, 2),
                         sol_at_0ms=round(float(d.y0.sum()), 2),
                         sol_at_seat=round(float(d.y_seat.sum()), 2)))
    return pd.DataFrame(rows), D


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


def leftover_summary(L, C=None):
    """`C` is `control(...)`, the random-print baseline. Pass it: a peak leftover no higher than the
    control's says nothing about the class (derive 5.2)."""
    hz = L.attrs.get("horizons", ())
    mid = "peak%d" % (len(hz) // 2)
    rows = [("acted", L[L.acted == 1]), ("behind", L[(L.acted == 1) & (L.ahead == 0)]),
            ("ignored", L[L.acted == 0])]
    if C is not None and len(C):
        rows.append(("control", C))
    out = {}
    for nm, d in rows:
        u = d.up.dropna()
        a = nm not in ("ignored", "control")
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


# A pass whose behind-row peak leftover is under this (net %, both legs paid) is flagged thin:
# a flag, never a kill. Uncalibrated - the anchors of evidence 1.27 put noise at about +1 %
# (sssssw +1.01 %) and a paying trigger at +7.46 % (rule 1); the calibrated line is open work.
THIN_PEAK = 2.0


def veto(L, n_decisions, C=None):
    """Derive 5.2 on a `leftover` table: the verdict every study script books. `n_decisions` is the
    member's decisions the class could cover (its episodes), for the 5.1 corner line. `C` is the
    random-print control: without it, or with a peak no higher than it, a PASS is flagged **thin**,
    because the best price inside a hold is positive on most prints of any class. Thin is a flag and
    never a kill."""
    Tb, _ = leftover_summary(L, C)
    A = L[L.acted == 1]
    Bh = L[(L.acted == 1) & (L.ahead == 0)]
    hz = [c for c in Tb.columns if c.startswith("peak_p50_h")]
    mid = hz[len(hz) // 2]
    b = Tb.loc["behind"]
    peak = float(b[mid]) if len(Bh) else np.nan
    why = []
    if not peak > 0.0:
        why.append("peak<=0")
    if not (len(Bh) and float(b.missed) < 50.0):
        why.append("missed>=50")
    if len(A) and float(A.dt.median()) <= 0.050 and float(A.ahead.mean()) < 0.10:
        why.append("race")
    cover = 100.0 * len(A) / max(n_decisions, 1)
    verdict = "kill" if why else ("corner" if cover < 10.0 else "PASS")
    ctrl = float(Tb.loc["control", mid]) if "control" in Tb.index else np.nan
    over_ctrl = np.nan if not np.isfinite(ctrl) else round(peak - ctrl, 2)
    thin = bool(verdict == "PASS" and (peak < THIN_PEAK or not np.isfinite(ctrl)
                                       or peak <= ctrl))
    return dict(
        acted=len(A), cover=round(cover, 1), behind=len(Bh),
        acted_dt_ms=round(1000 * float(A.dt.median()), 0) if len(A) else np.nan,
        acted_ahead=round(100 * float(A.ahead.mean()), 1) if len(A) else np.nan,
        cost=b.cost_p50, cost_ge2=b.cost_ge2,
        peak_p10h=b[hz[0]], peak=round(peak, 2), peak_p90h=b[hz[-1]],
        missed=b.missed, ign_peak=Tb.loc["ignored", mid],
        ctrl_peak=round(ctrl, 2) if np.isfinite(ctrl) else "unread",
        over_control=over_ctrl,
        verdict=verdict, why=",".join(why), thin=thin)
