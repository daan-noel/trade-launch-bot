"""Hot-tape node, step 12: the exit SHAPE, searched instead of guessed.

Step 11 measured the convexity the six wallets enter and killed three assumptions at once:

  * The move is a CONTINUATION process, not a mean-reverting one. At 20 s in, a position already
    up 25 % has median future max +107 % and reaches +50 % more 83 % of the time; one that is down
    10 % has median future max -2 %. Every take-profit family is therefore selling the wrong half.
  * Any stop or trail tighter than about 30 % is a winner-killer. Among entries whose peak exceeds
    +100 %, the drawdown BEFORE that peak is -15 % at the median and -31 % at p25.
  * Everything collapses in the end - median future MIN is -40 to -60 % in every bucket - so the
    exit cannot be "hold and see". The whole game is between those two walls.

And one family came out positive where 27 others did not: take half off at +10 %, trail the rest
25 %. It beats the same trail without the scale-out by 4.6 points. That is a shape, not a tuned
number, so this run searches the shape properly:

  1  THE LADDER GRID. First leg level x first leg fraction x runner trail x the DEAD CUT that
     applies only while no leg has fired. Searched on a random half of the episodes, verified on
     the other half - a split, so a tuned winner cannot be reported as a measurement.
  2  THREE-LEG LADDERS, because a convex payoff wants more than one rung.
  3  FLOW TELLS. On a constant-product curve windowed flow IS the price path, so "the buying has
     stopped" is expressible without a price threshold at all: net flow over the last W seconds
     turns negative, or the buy share does. This is the closest thing to an exit EVENT rather than
     an exit level, and it is the one family a level-based grid cannot contain.
  4  ROBUSTNESS on the finalists: per day, leave-one-day-out, and the book with its top 1 % of
     trades removed. A family that only survives with its best 1 % is a lottery, not an exit.

Their entries are the instrument (7.4 law 20) - the seat is ours, 115 ms behind their print, and
the result here is a SHAPE to carry to a real event, never a shippable sentence on its own.
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

B = 0.2
LAG = 0.115
CAP = 600.0
SEED = 20260910

pd.set_option("display.width", 460)
pd.set_option("display.max_rows", 600)
pd.set_option("display.max_columns", 44)

TP1 = (5.0, 10.0, 15.0, 20.0, 30.0)
F1 = (0.34, 0.5)
TRAIL = (20.0, 25.0, 30.0, 40.0)
DEAD = ("none", "stop25", "stop35", "cap15", "cap30", "cap60")
MODES = ("armed", "entry")


def net_multi(v0, sells, B=B):
    """Curve arithmetic of kernel.net, one sell leg at a time; every leg pays its own fixed cost."""
    s = B / (1.0 + FEE)
    tok = K / v0 - K / (v0 + s)
    out = 0.0
    legs = 0
    for v1, f in sells:
        ti = tok * f
        if ti <= 0:
            continue
        out += (v1 - K / (K / v1 + ti)) * (1.0 - FEE)
        legs += 1
    return out - B - FIX * (1.0 + max(legs, 1))


def prep(t, v, k, cap=CAP, lag=LAG):
    """The window an exit sees: fill index, entry reserve, price path, running peak, times."""
    n = len(t)
    ei = fill_idx(t, k, lag)
    if ei + 1 >= n:
        return None
    t0 = t[ei]
    m = int(np.searchsorted(t[ei + 1:], t0 + cap, side="right"))
    if m <= 0:
        return None
    va = v[ei + 1:ei + 1 + m]
    pr = (va / v[ei]) ** 2
    return dict(ei=ei, v0=float(v[ei]), t0=t0, pr=pr, tt=t[ei + 1:ei + 1 + m] - t0,
                peak=np.maximum.accumulate(np.maximum(pr, 1.0)), m=m)


def _first(mask):
    h = np.nonzero(mask)[0]
    return int(h[0]) if len(h) else None


def ladder(t, v, W, legs, trail, dead, mode="armed"):
    """Sell each rung as its level is first met, ride the remainder on a trail.

    EVERY exit here is decided in INDEX ORDER, which is the only thing that makes it causal. The
    dead cut is a deadline the position has to beat: if the first rung is not met before the cut
    lands, the cut fires and the rungs never happen. Deciding the cut against the whole window
    instead - "did this position EVER reach the rung" - is a lookahead, and a strong one: it keeps
    exactly the trades that were going to work.

    mode: "armed" - the trail arms at the first rung, and before that the only protection is the
                    dead cut.
          "entry" - the trail is live from the fill and competes with the rungs by index.
    """
    ei, v0, pr, peak, tt = W["ei"], W["v0"], W["pr"], W["peak"], W["tt"]
    big = len(pr) + 1
    if dead == "none":
        d = big
    elif dead.startswith("stop"):
        d = _first(pr <= 1.0 - float(dead[4:]) / 100.0)
        d = big if d is None else d
    else:
        d = _first(tt >= float(dead[3:]))
        d = big if d is None else d
    tj = _first(pr <= peak * (1.0 - trail / 100.0))
    tj_entry = big if tj is None else tj
    rung = []
    for lv, fr in legs:
        j = _first(pr >= 1.0 + lv / 100.0)
        rung.append((big if j is None else j, fr))
    r1 = rung[0][0]

    pre = min(d, tj_entry) if mode == "entry" else d
    if r1 >= pre:
        if pre >= big:
            return net(v0, float(v[ei + W["m"]]), B), "cap"
        x = fill_idx(t, ei + 1 + pre, LAG)
        return net(v0, float(v[x]), B), ("dead" if pre == d else "trail")

    if mode == "armed":
        hit = pr <= peak * (1.0 - trail / 100.0)
        hit = hit.copy()
        hit[:r1 + 1] = False
        close = _first(hit)
        close = big if close is None else close
    else:
        close = tj_entry

    sells = []
    rest = 1.0
    for j, fr in rung:
        if j >= close or j >= big:
            break
        f = min(fr, rest)
        if f <= 1e-9:
            break
        sells.append((float(v[fill_idx(t, ei + 1 + j, LAG)]), f))
        rest -= f
    if rest > 1e-9:
        if close >= big:
            sells.append((float(v[ei + W["m"]]), rest))
            why = "cap"
        else:
            sells.append((float(v[fill_idx(t, ei + 1 + close, LAG)]), rest))
            why = "trail"
    else:
        why = "ladder"
    return net_multi(v0, sells), why


def tell_exit(t, v, W, tell, arm, trail, dead):
    """Exit on a FLOW tell: the first print after the fill where the tape stops buying.

    `arm` delays the tell until the position is up that percent - the tape goes quiet constantly
    and an unarmed tell is a random clock. `trail` and `dead` are the fallbacks if it never fires.
    """
    ei, v0, pr, peak, tt = W["ei"], W["v0"], W["pr"], W["peak"], W["tt"]
    tl = tell[ei + 1:ei + 1 + W["m"]].copy()
    if arm > 0:
        a = np.nonzero(peak >= 1.0 + arm / 100.0)[0]
        if len(a):
            tl[:int(a[0])] = False
        else:
            tl[:] = False
    cands = []
    hj = np.nonzero(tl)[0]
    if len(hj):
        cands.append((int(hj[0]), "tell"))
    h = np.nonzero(pr <= peak * (1.0 - trail / 100.0))[0]
    if len(h):
        cands.append((int(h[0]), "trail"))
    if dead.startswith("stop"):
        h = np.nonzero(pr <= 1.0 - float(dead[4:]) / 100.0)[0]
        if len(h):
            cands.append((int(h[0]), "dead"))
    if cands:
        j, why = min(cands)
        x = fill_idx(t, ei + 1 + j, LAG)
        return net(v0, float(v[x]), B), why
    return net(v0, float(v[ei + W["m"]]), B), "cap"


def cumwin(t, x, w):
    """Sum of x over the (t-w, t] window, ending strictly at each print."""
    c = np.concatenate(([0.0], np.cumsum(x)))
    j = np.searchsorted(t, t - w, side="left")
    return c[np.arange(len(t)) + 1] - c[j]


def robust(d, days, name):
    ag = d.groupby("day").y.sum()
    tot = float(d.y.sum())
    top1 = float(d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum())
    loo = min(float(tot - ag.loc[i]) for i in ag.index)
    return dict(exit=name, n=len(d), sol=round(tot, 1),
                pct=round(float(d.y.mean()) / B * 100, 2),
                pos="%d/%d" % (int((ag > 0).sum()), ag.size),
                worst=round(float(ag.min()), 1), win=round(100 * float((d.y > 0).mean()), 1),
                top1=round(100 * top1 / tot, 1) if tot else None,
                ex_top1=round(100 * (tot - top1) / len(d) / B, 2),
                loo_worst=round(loo, 1))


def main() -> None:
    t0 = time.time()
    E = pd.read_parquet(data_file("cvx_hot_exit_eps.parquet"))
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    print("episodes %s  %ds" % (f"{len(E):,}", time.time() - t0), flush=True)

    # ---- 0. the ceiling, priced on the REAL entry reserve ---------------------------------
    HZ = [30, 60, 120, 300, 600, 1800]
    rows = []
    for H in HZ:
        c = "mfe%d" % H
        v1 = E.v0.to_numpy() * np.sqrt(1.0 + np.maximum(E[c].to_numpy(), -0.99))
        y = np.array([net(a, b, B) for a, b in zip(E.v0.to_numpy(), v1)])
        d = pd.DataFrame(dict(y=y, day=E.day.to_numpy()))
        ag = d.groupby("day").y.sum()
        rows.append(dict(horizon="%d s" % H, mfe_p50=round(100 * float(E[c].median()), 2),
                         mfe_p90=round(100 * float(E[c].quantile(0.90)), 2),
                         perfect_pct=round(100 * float(y.mean()) / B, 2),
                         pos="%d/%d" % (int((ag > 0).sum()), ag.size)))
    show(rows, "PERFECT-EXIT CEILING per horizon, their entries at our seat (real entry reserve)")

    # ---- windows, built once per episode ---------------------------------------------------
    byrun = {}
    for i, r in enumerate(E.run.values):
        byrun.setdefault(int(r), []).append(i)
    rng = np.random.default_rng(SEED)
    half = rng.random(len(E)) < 0.5
    W = {}
    TELLS = {}
    for r, idxs in byrun.items():
        a, b = T.start[r], T.end[r]
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        bs = np.where(side == 1, sol, 0.0); ss = np.where(side == -1, sol, 0.0)
        tells = {}
        for w in (3.0, 10.0):
            bw = cumwin(t, bs, w); sw = cumwin(t, ss, w)
            tells["net%d" % int(w)] = (bw - sw) < 0.0
            tells["bs%d" % int(w)] = (bw + sw > 0) & (bw < 0.5 * (bw + sw))
        tells["dry3"] = cumwin(t, (side == 1).astype(float), 3.0) < 1.0
        TELLS[r] = tells
        for i in idxs:
            k = int(E.ki.values[i])
            if k >= len(t) - 1:
                continue
            w = prep(t, v, k)
            if w is not None:
                W[i] = (r, w)
    print("windows %s  %ds" % (f"{len(W):,}", time.time() - t0), flush=True)

    day = E.day.to_numpy()

    def run_family(fn, keys, subset):
        """Book one exit definition over a subset of episodes; returns the frame."""
        out = []
        for i in subset:
            if i not in W:
                continue
            r, w = W[i]
            a, b = T.start[r], T.end[r]
            y, why = fn(T.t[a:b], T.v[a:b], w, r)
            out.append((y, int(day[i]), why, int(E.run.values[i])))
        return pd.DataFrame(out, columns=["y", "day", "reason", "run"])

    # ---- 1. the ladder grid, fitted on half ------------------------------------------------
    fit_all = np.nonzero(half)[0]
    hold = np.nonzero(~half)[0]
    # 432 shapes x 47k episodes is hours of python; the grid searches a random 15k of the fit
    # half and every survivor is then re-read on the WHOLE hold half.
    fit = rng.choice(fit_all, size=min(15000, len(fit_all)), replace=False)
    print("\n=== 1. LADDER GRID  (grid sample %s of the fit half, hold %s)  %ds"
          % (f"{len(fit):,}", f"{len(hold):,}", time.time() - t0), flush=True)
    grid = []
    for tp in TP1:
        for f1 in F1:
            for tr in TRAIL:
                for dd in DEAD:
                    for md in MODES:
                        grid.append((tp, f1, tr, dd, md))
    res = []
    for gi, (tp, f1, tr, dd, md) in enumerate(grid):
        legs = [(tp, f1)]
        d = run_family(lambda t, v, w, r, L=legs, TR=tr, DD=dd, M=md:
                       ladder(t, v, w, L, TR, DD, M), None, fit)
        res.append(dict(tp=tp, f1=f1, trail=tr, dead=dd, mode=md, n=len(d),
                        pct=round(float(d.y.mean()) / B * 100, 2),
                        sol=round(float(d.y.sum()), 1),
                        pos="%d/%d" % (int((d.groupby("day").y.sum() > 0).sum()),
                                       d.day.nunique())))
        if gi % 30 == 0:
            print("  grid %d/%d  %ds" % (gi, len(grid), time.time() - t0), flush=True)
    R = pd.DataFrame(res).sort_values("pct", ascending=False)
    show(R.head(25).to_dict("records"), "ladder grid, FIT half, top 25 by percent per trade")
    show(R.tail(8).to_dict("records"), "ladder grid, FIT half, the worst 8")

    print("\n=== 1b. the same top shapes on the HOLD half")
    out = []
    for rec in R.head(8).to_dict("records"):
        d = run_family(lambda t, v, w, r, L=[(rec["tp"], rec["f1"])], TR=rec["trail"],
                       DD=rec["dead"], M=rec["mode"]: ladder(t, v, w, L, TR, DD, M), None, hold)
        out.append(robust(d, days, "tp%g f%g tr%g %s %s"
                          % (rec["tp"], rec["f1"], rec["trail"], rec["dead"], rec["mode"])))
    show(out, "HOLD half, the shapes the fit half chose")

    # ---- 2. three-leg ladders --------------------------------------------------------------
    print("\n=== 2. THREE-LEG LADDERS on the HOLD half  %ds" % (time.time() - t0), flush=True)
    L3 = {
        "10/30/100 .34each tr30 stop35": ([(10.0, 0.34), (30.0, 0.33), (100.0, 0.33)], 30.0,
                                          "stop35", "entry"),
        "10/30/100 .34each tr30 cap30": ([(10.0, 0.34), (30.0, 0.33), (100.0, 0.33)], 30.0,
                                         "cap30", "armed"),
        "8/25/75 .34each tr25 entry": ([(8.0, 0.34), (25.0, 0.33), (75.0, 0.33)], 25.0,
                                       "stop35", "entry"),
        "10/40 .5/.25 tr30 entry": ([(10.0, 0.5), (40.0, 0.25)], 30.0, "stop35", "entry"),
        "10/50 .5/.25 tr40 entry": ([(10.0, 0.5), (50.0, 0.25)], 40.0, "stop35", "entry"),
        "15/60 .5/.25 tr30 entry": ([(15.0, 0.5), (60.0, 0.25)], 30.0, "stop35", "entry"),
        "step 11 winner, entry trail": ([(10.0, 0.5)], 25.0, "none", "entry"),
        "step 11 winner, armed trail": ([(10.0, 0.5)], 25.0, "none", "armed"),
        "one clip, no rung, tr25 entry": ([(1e9, 1.0)], 25.0, "none", "entry"),
    }
    out = []
    for nm, (legs, tr, dd, md) in L3.items():
        d = run_family(lambda t, v, w, r, L=legs, TR=tr, DD=dd, M=md:
                       ladder(t, v, w, L, TR, DD, M), None, hold)
        out.append(robust(d, days, nm))
    show(out, "multi-rung ladders, HOLD half")

    # ---- 3. flow tells ---------------------------------------------------------------------
    print("\n=== 3. FLOW TELLS on the HOLD half  %ds" % (time.time() - t0), flush=True)
    out = []
    for tk in ("net3", "net10", "bs3", "bs10", "dry3"):
        for arm in (0.0, 5.0, 10.0, 25.0):
            d = run_family(
                lambda t, v, w, r, TK=tk, A=arm: tell_exit(t, v, w, TELLS[r][TK], A, 40.0,
                                                           "stop35"),
                None, hold)
            out.append(robust(d, days, "%s arm%g tr40 stop35" % (tk, arm)))
    show(sorted(out, key=lambda x: -x["pct"]), "flow tells, HOLD half")

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
