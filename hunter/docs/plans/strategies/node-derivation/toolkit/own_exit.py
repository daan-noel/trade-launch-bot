"""Derive step 3 (section 8.0): the wallet's OWN exit, read from its own sells.

Read before any entry test, for two reasons. His sells show his exit with no guessing, because the
pool is his own positions and nothing in it is selected by hindsight. And every entry test needs an
exit to price it: an entry booked under a placeholder is booked wrong, which is how 136 walked
exits came back red on a member whose real exit is a trail that widens as the run grows.

  families(R, k, hold_p50) every branch this module knows, as a state at each print of the coin:
                           the static abort, the flat and curved trails, keep a share of the gain,
                           and a clock. A branch is a yes/no at every print, so a set of them is an
                           OR and the first print where any is true is the sell
  coverage(S, E, build)    8.0 c, on FIRST CROSSING: per branch, the share of his positions where
                           he is out within `lat` of its first crossing (it names his sell), where
                           he sits through it (it is early), and where it never fires before he is
                           out. Read on both halves of the days. A branch he sits through most of
                           the time is not his exit, however well it describes the exit print
  book(S, E, build, ...)   8.0 d: every set traded on his positions - bought at his fill, sold at
                           the first public print where the set turns true, carried past his own
                           sell when it has not fired - with his own close booked beside it on the
                           same kernel
  table(E, *book(...))     one row a set: its SOL, the fit and test halves, the tail, and how far
                           each of its trades lands from his own close. The set nearest his book is
                           the reading of his exit; a set that earns MORE than he does is reported,
                           never preferred - this step reads his logic, not our money
  at_our_seat(S, E, build, lag)
                           the same sets read from OUR fill and filled `lag` after the deciding
                           print. This is what the frozen set costs at our seat, and it is the
                           starting X of every later book (derive 5.3, 6.2, 10)

The frozen set is chosen on the fit days, frozen in code, then read ONCE on the holdout (8.0 e):
it passes when it books about his SOL and about his top 1 % on days it never saw. Each branch then
carries its plain reason and its inventory X row (8.0 f); a branch with no reason is dropped.
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from kernel import B_DEFAULT as B, LAG, fill_idx, net

from .facts import Run

LAT = 0.3          # he acts about 80 ms behind a print; a sell inside this window is that print's
CAP = 1200.0       # a set that has not fired by his own sell is carried this long after the fill

ABORT_T = (3.0, 5.0, 7.0, 10.0, 15.0, 20.0)      # held at least this many seconds
TRAIL_S = (5.0, 7.0, 10.0, 15.0, 20.0)           # the flat trail's give-back, %
CURVE_P = (0.3, 0.4, 0.5)                        # the curved trail's growth with the run
CURVE_W = (17.0, 19.0, 21.0, 23.0)               # its width at a +50 % best, which sets the scale
CURVE_F = (7.0, 10.0)                            # the floor under it
KEEP_S = (0.3, 0.4, 0.5, 0.6, 0.7)               # the share of the best gain he keeps
KEEP_A = (2.0, 5.0, 10.0, 15.0, 20.0, 30.0)      # the best gain, %, that arms it


def _gain(R, k):
    """Per print: the gain over his fill, the best gain so far, and the give-back from that best -
    all in percent, from the price his own buy left, so his own impact is not counted as a move."""
    idx = np.arange(R.n)
    p = (R.v / float(R.v[k])) ** 2
    bp = np.maximum.accumulate(np.where(idx >= k, p, 0.0))
    up = (p - 1.0) * 100.0
    best = np.maximum((bp - 1.0) * 100.0, 0.0)
    ret = (1.0 - p / np.where(bp > 0, bp, 1.0)) * 100.0
    return up, best, ret


def abort(R, k):
    """Held a time AND still under a line: the cut that fires when nothing follows. The lines are the
    price before his buy, 3 % under it, his own fill, and 5 % over his fill."""
    held = R.tm - R.tm[k]
    vb = float(R.vbef[k]) if R.vbef[k] > 0 else float(R.v[k])
    lines = (("under the price before his buy", (R.v / vb) ** 2 - 1.0, 0.0),
             ("3 % under the price before his buy", (R.v / vb) ** 2 - 1.0, -0.03),
             ("under his fill", (R.v / float(R.v[k])) ** 2 - 1.0, 0.0),
             ("not 5 % over his fill", (R.v / float(R.v[k])) ** 2 - 1.0, 0.05))
    return {"held %g s AND %s" % (t, nm): (held >= t) & (x < off)
            for t in ABORT_T for nm, x, off in lines}


def trail(R, k):
    """A flat give-back from the best price reached in the hold."""
    _, _, ret = _gain(R, k)
    return {"flat trail %g %%" % s: ret >= s for s in TRAIL_S}


def curve(R, k):
    """A trail whose width bends: it grows with the best gain as a power under 1, so it widens fast on
    a small run and flattens on a big one. Named by its width at a +50 % best."""
    _, best, ret = _gain(R, k)
    return {"curved trail p %g, %g %% at a +50 %% best, floor %g %%" % (pw, wd, f):
            ret >= np.maximum(f, wd / 50.0 ** pw * best ** pw)
            for pw in CURVE_P for wd in CURVE_W for f in CURVE_F}


def keep(R, k):
    """Keep a share of the best gain: once the best gain arms it, sell when the gain falls to that
    share of its best. A +60 % best kept at 0.5 sells at +30 %."""
    up, best, _ = _gain(R, k)
    return {"keep %g of the gain, armed at +%g %%" % (s, a): (best >= a) & (up <= s * best)
            for s in KEEP_S for a in KEEP_A}


def clock(R, k, secs):
    return {"clock %g s" % s: (R.tm - R.tm[k]) >= s for s in np.atleast_1d(secs)}


def wall(R, k, v=110.0):
    """Sell before the curve completes: the price jumps at the wall and early holders leave there."""
    return {"vsol >= %g (before the wall)" % v: R.v >= v}


def families(R, k, hold_p50=None):
    """Every branch this module knows, in one dict. A study narrows it; it does not re-spell it."""
    out = {}
    for f in (abort, trail, curve, keep, wall):
        out.update(f(R, k))
    if hold_p50 is not None:
        out.update(clock(R, k, hold_p50))
    return out


def coverage(S, E, build, lat=LAT, folds=True):
    """8.0 c. Every branch scored on FIRST CROSSING over his closed positions.

    `build(R, k)` returns {branch name: state at each print}. For each position the branch's first
    crossing is the earliest PUBLIC print strictly inside the hold where its state is true:

      names      he is out within `lat` after it      - the branch names that sell
      early      he sits through it and sells later   - the branch is not what he does
      never      it does not fire before he is out

    A branch that names a small share is a description of the exit print, not the rule behind it."""
    C = E[E.ks >= 0]
    day = C.day.to_numpy()
    dl = np.sort(np.unique(day)); h = (len(dl) + 1) // 2
    fit = np.isin(day, dl[:h])
    names = None; M = None
    for r, g in C.groupby("run"):
        R = Run(S, int(r))
        for i, k, ks in zip(C.index.get_indexer(g.index), g.k.to_numpy(), g.ks.to_numpy()):
            k, ks = int(k), int(ks)
            if ks <= k + 1:
                continue
            mom = np.nonzero(R.pub[k + 1:ks])[0] + k + 1      # prints he could have left on
            if not len(mom):
                continue
            X = build(R, k)
            if names is None:
                names = list(X); M = np.zeros((len(names), len(C), 3), dtype=bool)
            for j, nm in enumerate(names):
                f = mom[X[nm][mom]]
                if not len(f):
                    M[j, i, 2] = True
                elif R.tm[ks] - R.tm[int(f[0])] <= lat:
                    M[j, i, 0] = True
                else:
                    M[j, i, 1] = True
    if names is None:
        return pd.DataFrame()
    seen = M.any(2)
    rows = []
    for j, nm in enumerate(names):
        ok = seen[j]
        row = dict(branch=nm, positions=int(ok.sum()),
                   names_pct=round(100.0 * M[j, ok, 0].mean(), 1),
                   early_pct=round(100.0 * M[j, ok, 1].mean(), 1),
                   never_pct=round(100.0 * M[j, ok, 2].mean(), 1))
        if folds:
            for lab, m in (("fit", fit), ("test", ~fit)):
                s = ok & m
                row["names_%s" % lab] = round(100.0 * M[j, s, 0].mean(), 1) if s.any() else np.nan
        rows.append(row)
    return pd.DataFrame(rows).sort_values("names_pct", ascending=False)


def book(S, E, build, lag=0.0, entry_lag=None, cap=CAP, wall_d=None, amm_net=None, clip=B):
    """8.0 d. His close and each set's close on every position.

    `build(R, k, ks)` returns the sets as states at each print. Each set is sold at the first PUBLIC
    print where it turns true, filled `lag` after that print; a set that has not fired by his own
    sell is carried to `cap` after the fill or the coin's last print, so a late sell costs what it
    costs. `entry_lag = None` buys at HIS fill (the reserve his own buy left), which is the seat
    8.0 reads: it asks what his rule is, not what ours earns. A number sets our own entry instead,
    `entry_lag` after the print he answered. `wall_d` prices a set carried onto the completing print
    in the migrated pool at (1 - wall_d) of its opening, through `amm_net` (graduation.amm_net)."""
    n = len(E)
    his = np.full(n, np.nan); his_t = np.full(n, np.nan)
    bk = {}; hold = {}
    for r, g in E.groupby("run"):
        R = Run(S, int(r))
        for i, k, ks in zip(E.index.get_indexer(g.index), g.k.to_numpy(), g.ks.to_numpy()):
            k, ks = int(k), int(ks)
            if ks <= k:
                continue
            ek = k if entry_lag is None else int(fill_idx(R.t, k, entry_lag))
            v0 = float(R.vbef[k]) if entry_lag is None else float(R.v[ek])
            his[i] = net(float(R.vbef[k]), float(R.v[ks - 1]), clip)
            his_t[i] = R.tm[ks] - R.tm[k]
            end = int(np.searchsorted(R.tm, R.tm[ek] + cap, side="right"))
            mom = np.nonzero(R.pub[ek + 1:end])[0] + ek + 1
            if not len(mom):
                continue
            for nm, st in build(R, ek, ks).items():
                if nm not in bk:
                    bk[nm] = np.full(n, np.nan); hold[nm] = np.full(n, np.nan)
                f = mom[st[mom]]
                j = int(f[0]) if len(f) else int(mom[-1])
                x = int(fill_idx(R.t, j, lag)) if lag else j
                at_wall = x == R.n - 1 and R.v[x] >= 114.0
                bk[nm][i] = (amm_net(v0, wall_d) if at_wall and wall_d is not None
                             and amm_net is not None else net(v0, float(R.v[x]), clip))
                hold[nm][i] = R.tm[x] - R.tm[ek]
    return his, his_t, bk, hold


def table(E, his, his_t, bk, hold, lat=LAT):
    """One row a set: its SOL, the two halves of the days, the tail, and how far each of its trades
    lands from his own close. Rank by `diff_abs`, never by SOL: the set nearest his book is the
    reading of his exit, and a set that earns more than he does has stopped describing him."""
    day = E.day.to_numpy(); dl = np.sort(np.unique(day)); h = (len(dl) + 1) // 2
    FIT = np.isin(day, dl[:h]); HV = E.pnl.to_numpy() > 0
    ok = np.isfinite(his)
    for nm in bk:
        ok &= np.isfinite(bk[nm])
    rank = pd.Series(np.where(ok, his, -np.inf)).rank(ascending=False, method="first").to_numpy()
    TOP1 = ok & (rank <= max(round(0.01 * ok.sum()), 1))
    TOP5 = ok & (rank <= max(round(0.05 * ok.sum()), 1))

    def row(nm, x, t):
        d = x - his
        return dict(
            exit=nm, sol=round(float(x[ok].sum()), 2), fit=round(float(x[ok & FIT].sum()), 2),
            test=round(float(x[ok & ~FIT].sum()), 2), top1=round(float(x[TOP1].sum()), 2),
            top5=round(float(x[TOP5].sum()), 2), rest=round(float(x[ok & ~TOP5].sum()), 2),
            per_trade=round(float(x[ok].mean()), 4),
            his_winners=round(float(x[ok & HV].sum()), 2),
            his_cuts=round(float(x[ok & ~HV].sum()), 2),
            diff_abs=round(float(np.abs(d[ok]).mean()), 4),
            diff_abs_fit=round(float(np.abs(d[ok & FIT]).mean()), 4),
            diff_abs_test=round(float(np.abs(d[ok & ~FIT]).mean()), 4),
            corr=round(float(np.corrcoef(x[ok], his[ok])[0, 1]), 3),
            before_him=round(100.0 * float((t[ok] < his_t[ok] - lat).mean()), 1),
            with_him=round(100.0 * float((np.abs(t[ok] - his_t[ok]) <= lat).mean()), 1),
            after_him=round(100.0 * float((t[ok] > his_t[ok] + lat).mean()), 1),
            hold_p50=round(float(np.median(t[ok])), 1))
    rows = [row("his own exit", his, his_t)] + [row(nm, bk[nm], hold[nm]) for nm in bk]
    return pd.DataFrame(rows), dl[:h], dl[h:]


def at_our_seat(S, E, build, lag=LAG, **kw):
    """The frozen set read from OUR fill: we buy `lag` after the print he answered and sell `lag`
    after the print that decides the exit. Every later book uses this, never the zero-lag version -
    the difference between the two is what the seat costs, and it is reported, not hidden."""
    return book(S, E, build, lag=lag, entry_lag=lag, **kw)
