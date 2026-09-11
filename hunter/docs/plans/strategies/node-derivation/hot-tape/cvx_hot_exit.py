"""Hot-tape node, step 11: THE EXIT SLOT, measured against the convexity they actually enter.

WHY THIS RUN EXISTS. Every exit this node has been booked under came from a guessed grid - a
clock, a trail, an abort - chosen before anything was known about the shape of the move being
exited. Measured, that grid leaves 26 points on the table: the model's fires book -6.67 %/trade
under cap60 while selling each one at its own 120 s peak books +19.26 %. An exit family has to be
FITTED to the excursion distribution, and the distribution has to be measured first.

THE BASIS. The convexities in question are the ones THEY enter, so the entry here is their own
buy and the seat is ours: the last print landed 115 ms after their print. That removes the entry
question from the exit question entirely - whatever their trigger turns out to be, the shape of
what follows it is what an exit must eat. Their wallets are the instrument (7.4 law 20): a
ceiling and a shape, never a term in a sentence.

WHAT IS MEASURED, in order:

  1  EPISODES. Position tracked to the token, exactly as cvx_replay3: an episode opens when their
     position leaves zero and closes when it returns. So `their exit` is a real decision, not the
     next sell print, and a re-entry trader is not scored on his worst subset.
  2  GEOMETRY from our fill. MFE, time to peak, and - the number the guessed grids never had -
     the MAE **before** the peak. A trail that is knocked out at -15 % before a +50 % run is not
     a trail, it is a stop wearing one, and that is the most likely reason every trail failed.
  3  CONTINUATION. Given the position is at +g % after t seconds, what is the distribution of the
     max still ahead? This is the object that decides take-profit against trail: momentum says
     ride, reversion says take. Nothing about an exit can be chosen before this table.
  4  THEIR OWN CAPTURE. What fraction of the available peak they actually take. If their exit is
     the edge they are claimed to have, it shows up here as a capture ratio a mechanical rule
     cannot reach.
  5  THE GRID, fitted. Clocks, take-profits, trails, armed trails, aborts and SCALE-OUTS. The MFE
     distribution is extremely convex (p50 4 %, p90 53 %, p99 203 %), and a single exit point on a
     convex payoff is provably not the best shape - taking the base off early and riding the tail
     is. The kernel prices one clip in and one clip out, so the scale-out legs are priced here by
     the same curve arithmetic, each leg paying its own fee, its own fixed cost and its own impact.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import FEE, FIX, K, book, fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids

B = 0.2
LAG = 0.115
HOR = 1800.0              # geometry horizon: t_pk p90 is 468 s, so 600 s truncates it
HZ = (30.0, 60.0, 120.0, 300.0, 600.0, 1800.0)   # ceiling reported at every horizon
CLOSE_EPS = 0.02          # position under 2 % of the episode peak counts as closed
CHECK = (5.0, 10.0, 20.0, 40.0, 80.0, 160.0)

pd.set_option("display.width", 440)
pd.set_option("display.max_rows", 500)
pd.set_option("display.max_columns", 44)


def net_multi(v0, sells, B=B):
    """Buy B at v0, sell the bag in legs. sells = [(reserve_at_sell, fraction_of_bag), ...].

    Same curve arithmetic as kernel.net, one leg at a time: each sell pays 125 bps, the fixed
    0.000225 SOL, and its own impact on the virtual reserve. A three-leg exit therefore costs
    two extra fixed legs (0.22 % of a 0.2 SOL clip) - the scale-out has to beat that, not ignore it.
    """
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


def sim_scale(t, v, k, tp1, f1, trail, cap, arm=None, lag=LAG):
    """Sell f1 of the bag at the first print >= +tp1 %, ride the rest on a trail, cap the rest.

    The trail's peak runs from the fill for the whole hold - taking the first leg off does not
    reset it. Every leg fills at the last print landed lag seconds after its own trigger.
    """
    n = len(t)
    ei = fill_idx(t, k, lag)
    v0 = v[ei]
    t0 = t[ei]
    if ei + 1 >= n:
        return net_multi(v0, [(v0, 1.0)]), ei, ei, "flat"
    ta = t[ei + 1:]
    m = int(np.searchsorted(ta, t0 + cap, side="right"))
    if m <= 0:
        return net_multi(v0, [(v[ei], 1.0)]), ei, ei, "cap"
    va = v[ei + 1:ei + 1 + m]
    pr = (va / v0) ** 2
    peak = np.maximum.accumulate(np.maximum(pr, 1.0))
    sells = []
    j1 = None
    if f1 > 0:
        h = np.nonzero(pr >= 1.0 + tp1 / 100.0)[0]
        if len(h):
            j1 = ei + 1 + int(h[0])
            x1 = fill_idx(t, j1, lag)
            sells.append((v[x1], f1))
    rest = 1.0 - (f1 if j1 is not None else 0.0)
    hit = pr <= peak * (1.0 - trail / 100.0)
    if arm is not None:
        aidx = np.nonzero(peak >= 1.0 + arm / 100.0)[0]
        if len(aidx):
            hit = hit.copy()
            hit[:int(aidx[0])] = False
        else:
            hit = np.zeros_like(hit)
    if j1 is not None:
        hit = hit.copy()
        hit[:j1 - ei] = False          # the runner's trail starts once the first leg is off
    hj = np.nonzero(hit)[0]
    if len(hj):
        j2 = ei + 1 + int(hj[0])
        x2 = fill_idx(t, j2, lag)
        reason = "trail"
    else:
        x2 = max(ei, int(np.searchsorted(t, t0 + cap + lag, side="right") - 1))
        reason = "cap"
    sells.append((v[x2], rest))
    return net_multi(v0, sells), ei, x2, reason


EXITS = {
    "cap15": dict(cap=15.0), "cap30": dict(cap=30.0), "cap60": dict(cap=60.0),
    "cap120": dict(cap=120.0), "cap300": dict(cap=300.0),
    "tp5_c120": dict(tp=5.0, cap=120.0), "tp10_c120": dict(tp=10.0, cap=120.0),
    "tp15_c120": dict(tp=15.0, cap=120.0), "tp25_c300": dict(tp=25.0, cap=300.0),
    "tp50_c600": dict(tp=50.0, cap=600.0),
    "tr15_c600": dict(trail=15.0, cap=600.0), "tr25_c600": dict(trail=25.0, cap=600.0),
    "tr35_c600": dict(trail=35.0, cap=600.0), "tr50_c600": dict(trail=50.0, cap=600.0),
    "arm10_tr25": dict(arm=10.0, trail=25.0, cap=600.0),
    "arm10_tr40": dict(arm=10.0, trail=40.0, cap=600.0),
    "arm25_tr40": dict(arm=25.0, trail=40.0, cap=600.0),
    "arm10_tr40_s20": dict(arm=10.0, trail=40.0, stop=20.0, cap=600.0),
    "shipped": dict(arm=21.0, trail=36.0, stop=43.75, cap=1200.0),
    "ab10_tr40": dict(abort=(10.0, 0.0), arm=10.0, trail=40.0, cap=600.0),
    "ab20g5_tr40": dict(abort=(20.0, 5.0), arm=10.0, trail=40.0, cap=600.0),
}
SCALES = {
    "half5_tr40": (5.0, 0.5, 40.0, 600.0, None),
    "half10_tr40": (10.0, 0.5, 40.0, 600.0, None),
    "half10_tr25": (10.0, 0.5, 25.0, 600.0, None),
    "half20_tr40": (20.0, 0.5, 40.0, 600.0, None),
    "two3rd10_tr50": (10.0, 2.0 / 3.0, 50.0, 600.0, None),
    "three4th10_tr60": (10.0, 0.75, 60.0, 600.0, None),
    "half10_arm25tr40": (10.0, 0.5, 40.0, 600.0, 25.0),
}


def qs(x, ps=(10, 25, 50, 75, 90, 95, 99)):
    x = np.asarray(x, float)
    x = x[np.isfinite(x)]
    if not x.size:
        return "n/a"
    return "  ".join("%8.2f" % p for p in np.percentile(x, ps))


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    is_node = np.isin(T.wallet, NODE)
    runs = np.unique(T.run_of[np.nonzero(is_node)[0]])
    print("tape %s prints  hot-tape wallets %d  coins %s  %ds"
          % (f"{T.n:,}", len(lab), f"{len(runs):,}", time.time() - t0), flush=True)

    # ---- 1. episodes, position tracked ---------------------------------------------------
    eps = []
    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]; wal = T.wallet[a:b]
        t = T.t[a:b]
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        for w in np.unique(wal[np.isin(wal, NODE)]):
            w = int(w)
            p = np.nonzero(wal == w)[0]
            pos = 0.0; peak = 0.0; ki = None; s_in = 0.0; s_out = 0.0; nbuy = 0
            for k in p:
                k = int(k)
                if side[k] == 1:
                    tk = K / vbef[k] - K / v[k]
                    if ki is None:
                        ki = k; s_in = 0.0; s_out = 0.0; peak = 0.0; nbuy = 0
                    pos += tk; s_in += float(sol[k]); peak = max(peak, pos); nbuy += 1
                else:
                    if ki is None:
                        continue
                    pos -= K / v[k] - K / vbef[k]
                    s_out += float(sol[k])
                    if pos <= CLOSE_EPS * peak:
                        eps.append((lab[w], r, int(T.day[a + ki]), False, ki, k, nbuy,
                                    s_in, s_out, float(vbef[ki]), float(vbef[k])))
                        ki = None; pos = 0.0
            if ki is not None:
                eps.append((lab[w], r, int(T.day[a + ki]), True, ki, len(t) - 1, nbuy,
                            s_in, s_out, float(vbef[ki]), float(v[-1])))
        if r % 4000 == 0:
            print("  episodes run %d  %s  %ds" % (r, f"{len(eps):,}", time.time() - t0),
                  flush=True)

    E = pd.DataFrame(eps, columns=["w", "run", "day", "bag", "ki", "ko", "nbuy",
                                   "s_in", "s_out", "v_in", "v_out"])
    E = E[E.s_in > 0].reset_index(drop=True)
    print("\nepisodes %s   bags %.2f %%   median clip %.3f SOL   median buys/episode %.0f  %ds"
          % (f"{len(E):,}", 100 * float(E.bag.mean()), float(E.s_in.median()),
             float(E.nbuy.median()), time.time() - t0), flush=True)

    # ---- 2. geometry from OUR fill, and 3. continuation samples ---------------------------
    geo = []
    cont = []
    byrun = {}
    for i, r in enumerate(E.run.values):
        byrun.setdefault(int(r), []).append(i)
    for r, idxs in byrun.items():
        a, b = T.start[r], T.end[r]
        t = T.t[a:b]; v = T.v[a:b]
        nb = is_node[a:b] & (T.side[a:b] == 1)
        for i in idxs:
            ki = int(E.ki.values[i]); ko = int(E.ko.values[i])
            ei = fill_idx(t, ki, LAG)
            v0 = v[ei]; t0r = t[ei]
            j = int(np.searchsorted(t, t0r + HOR, side="right"))
            if j > ei + 1:
                pr = (v[ei + 1:j] / v0) ** 2
                tt = t[ei + 1:j] - t0r
            else:
                pr = np.array([1.0]); tt = np.array([0.0])
            pk = int(np.argmax(pr))
            mfe = float(pr[pk] - 1.0)
            t_pk = float(tt[pk])
            mae_pre = float(pr[:pk + 1].min() - 1.0)
            mae_all = float(pr.min() - 1.0)
            end = float(pr[-1] - 1.0)
            xi = fill_idx(t, ko, LAG)
            their_mv = float((v[xi] / v0) ** 2 - 1.0)
            their_t = float(t[xi] - t0r)
            m_to = tt <= max(their_t, 0.0)
            mfe_to_exit = float(pr[m_to].max() - 1.0) if m_to.any() else 0.0
            arrive = bool(nb[ei + 1:j].any()) if j > ei + 1 else False
            hz = []
            for H in HZ:
                mh = tt <= H
                hz.append(float(pr[mh].max() - 1.0) if mh.any() else 0.0)
            geo.append((mfe, t_pk, mae_pre, mae_all, end, their_mv, their_t,
                        mfe_to_exit, arrive, float(v0)) + tuple(hz))
            for c in CHECK:
                m = tt <= c
                if not m.any() or tt[-1] < c:
                    continue
                g = float(pr[m][-1] - 1.0)
                fut = pr[~m]
                cont.append((c, g, float(fut.max() - 1.0) if fut.size else g,
                             float(fut.min() - 1.0) if fut.size else g))
    G = pd.DataFrame(geo, columns=["mfe", "t_pk", "mae_pre", "mae_all", "end", "their_mv",
                                   "their_t", "mfe_to_exit", "arrive", "v0"]
                     + ["mfe%d" % int(H) for H in HZ])
    E = pd.concat([E, G], axis=1)
    C = pd.DataFrame(cont, columns=["at_s", "g", "fut_max", "fut_min"]).dropna()
    print("geometry done  %ds" % (time.time() - t0), flush=True)

    P = (10, 25, 50, 75, 90, 95, 99)
    hdr = "            " + "  ".join("%8s" % ("p%d" % p) for p in P)
    print("\n=== 2. THE SHAPE OF WHAT THEY ENTER  (our fill 115 ms after their buy, %.0f s horizon)"
          % HOR)
    print(hdr)
    print("  MFE %%     %s" % qs(100 * E.mfe, P))
    print("  MAE-pre   %s   <- drawdown BEFORE the peak: what a trail must survive"
          % qs(100 * E.mae_pre, P))
    print("  MAE all   %s" % qs(100 * E.mae_all, P))
    print("  end %%     %s   <- where price sits at the horizon" % qs(100 * E.end, P))
    print("  t_pk s    %s" % qs(E.t_pk, P))

    print("\n=== 2b. MAE-before-peak BY MFE bucket   (a stop tighter than this kills the winner)")
    bk = pd.cut(100 * E.mfe, [-100, 0, 5, 10, 25, 50, 100, 1e6])
    out = []
    for lv, g in E.groupby(bk, observed=True):
        out.append(dict(mfe_bucket=str(lv), n=len(g),
                        share=round(100 * len(g) / len(E), 1),
                        mae_pre_p10=round(100 * float(g.mae_pre.quantile(0.10)), 1),
                        mae_pre_p25=round(100 * float(g.mae_pre.quantile(0.25)), 1),
                        mae_pre_p50=round(100 * float(g.mae_pre.median()), 1),
                        t_pk_p50=round(float(g.t_pk.median()), 1),
                        t_pk_p90=round(float(g.t_pk.quantile(0.90)), 1)))
    show(out, "drawdown before the peak, by how big the peak got")

    print("\n=== 3. CONTINUATION: at +g %% after t seconds, what is still ahead?")
    for c in CHECK:
        d = C[C.at_s == c]
        if len(d) < 500:
            continue
        gb = pd.cut(d.g * 100, [-100, -10, -3, 0, 3, 10, 25, 1e6])
        rows = []
        for lv, g in d.groupby(gb, observed=True):
            rows.append(dict(now=str(lv), n=len(g),
                             fut_max_p50=round(100 * float(g.fut_max.median()), 1),
                             fut_max_p90=round(100 * float(g.fut_max.quantile(0.90)), 1),
                             p_ge10=round(100 * float((g.fut_max >= 0.10).mean()), 1),
                             p_ge25=round(100 * float((g.fut_max >= 0.25).mean()), 1),
                             p_ge50=round(100 * float((g.fut_max >= 0.50).mean()), 1),
                             fut_min_p50=round(100 * float(g.fut_min.median()), 1)))
        show(rows, "at t = %.0f s, future max/min over the rest of the %.0f s window" % (c, HOR))

    print("\n=== 4. WHAT THEY THEMSELVES CAPTURE")
    ok = E[~E.bag].copy()
    ok["cap_ratio"] = np.where(ok.mfe_to_exit > 0.005, ok.their_mv / ok.mfe_to_exit, np.nan)
    print("  closed episodes %s   their hold to exit p50 %.1f s  p90 %.1f s"
          % (f"{len(ok):,}", float(ok.their_t.median()), float(ok.their_t.quantile(0.9))))
    print("  their exit move on our basis %%    %s" % qs(100 * ok.their_mv, P))
    print("  peak available BEFORE they left %%  %s" % qs(100 * ok.mfe_to_exit, P))
    print("  capture ratio (their exit / peak they saw)  %s"
          % qs(ok.cap_ratio.clip(-2, 2), P))
    print("  they leave before the %.0f s peak in %.1f %% of episodes; their exit is within"
          " 10 %% of the window peak in %.1f %%"
          % (HOR, 100 * float((ok.their_t < ok.t_pk).mean()),
             100 * float((ok.their_mv >= 0.9 * ok.mfe).mean())))

    # ---- 5. the grid --------------------------------------------------------------------
    print("\n=== 5. THE GRID, on their entries at our seat  %ds" % (time.time() - t0), flush=True)
    rows = {x: [] for x in list(EXITS) + list(SCALES)}
    perf = []
    done = 0
    for r, idxs in byrun.items():
        a, b = T.start[r], T.end[r]
        t = T.t[a:b]; v = T.v[a:b]; sl = T.slot[a:b]
        for i in idxs:
            k = int(E.ki.values[i])
            if k >= len(t) - 1:
                continue
            d = int(E.day.values[i]); ar = bool(E.arrive.values[i])
            for xn, spec in EXITS.items():
                y, ei, xi, why = book(t, v, sl, k, spec, lag=LAG, B=B)
                rows[xn].append((y, r, d, why, ar))
            for xn, (tp1, f1, tr, cap, arm) in SCALES.items():
                y, ei, xi, why = sim_scale(t, v, k, tp1, f1, tr, cap, arm)
                rows[xn].append((y, r, d, why, ar))
            ei = fill_idx(t, k, LAG)
            j = int(np.searchsorted(t, t[ei] + HOR, side="right"))
            seg = v[ei:max(j, ei + 1)]
            perf.append((net(float(v[ei]), float(seg.max()), B), r, d))
        done += 1
        if done % 3000 == 0:
            print("  booked coins %d  %ds" % (done, time.time() - t0), flush=True)
    out = []
    for xn in list(EXITS) + list(SCALES):
        d = pd.DataFrame(rows[xn], columns=["y", "run", "day", "reason", "arrive"])
        c = cell(d, days=days)
        tot = float(d.y.sum())
        top1 = float(d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum())
        out.append(dict(exit=xn, n=c["n"], sol=c["sol"], pct=c["pct"], pos=c["pos"],
                        worst=c["worst"], win=c["win"],
                        top1=round(100 * top1 / tot, 1) if tot else None,
                        arrive_pct=round(float(d[d.arrive].y.mean()) / B * 100, 2),
                        none_pct=round(float(d[~d.arrive].y.mean()) / B * 100, 2)))
    show(sorted(out, key=lambda x: -x["pct"]),
         "exit families on THEIR entries at our seat, 0.2 SOL, lag_115 both legs")

    PF = pd.DataFrame(perf, columns=["y", "run", "day"])
    print("\n=== 6. THE CEILING, by how long we are allowed to hold")
    rows = []
    for H in HZ:
        c = "mfe%d" % int(H)
        pk = np.sqrt(1.0 + np.maximum(E[c].to_numpy(), -0.99))
        y = np.array([net(1.0, float(x), B) for x in pk])
        rows.append(dict(horizon="%.0f s" % H,
                         mfe_p50=round(100 * float(E[c].median()), 2),
                         mfe_p90=round(100 * float(E[c].quantile(0.90)), 2),
                         perfect_pct=round(100 * float(y.mean()) / B, 2)))
    show(rows, "perfect-exit ceiling per horizon, on their entries at our seat")
    print("\n  PERFECT EXIT (sell at each entry's own %.0f s peak): %+.2f %%/trade, %.2f SOL, %s"
          % (HOR, 100 * float(PF.y.mean()) / B, float(PF.y.sum()),
             cell(PF, days=days)["pos"]))
    print("  THEIRS on their own money: %+.2f %% on spend"
          % (100 * float(ok.s_out.sum() * 0.9875 - ok.s_in.sum() * 1.0125) / float(ok.s_in.sum())))

    E.to_parquet(data_file("cvx_hot_exit_eps.parquet"), index=False)
    C.to_parquet(data_file("cvx_hot_exit_cont.parquet"), index=False)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
