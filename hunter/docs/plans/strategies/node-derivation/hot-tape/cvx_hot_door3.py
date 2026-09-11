"""Hot-tape node, step 28: the DOOR for the frenzy-absorbed sell, from the UNPRICED side.

Step 26 searched D with size, busyness and prior-frenzy COUNTS and reached -0.27 %/trade: those
facts buy the share of fires that land on the coins 8fStGV trades, not their money. Step 27 put
half of "its coins" in its own arrival inside our hold (+6.04 % when it comes, -0.62 % when not).
So the door has to name what is not in the price (strategy 1.4) - who is in the frenzy, whether
they still hold, whether this coin's frenzies have been absorbed before, whether re-entry machines
work this coin, and who the seller is.

E is frozen (evidence 1.12). Five fact families, every one computed from prints BEFORE the fire,
and every one from PUBLIC prints only - the six node wallets are dropped from every fact, so no
fact can stand in for their presence (7.4 law 20):

  A  absorption   earlier frenzy-sells on this coin followed by a new high within 15 s, counted
                  only once their 15 s has elapsed; the frenzy event's own earlier book here
  B  composition  who buys in the last 5 s / 60 s: professional builds, routers, fresh wallets,
                  distinct wallets, build concentration
  C  holding      of the tokens bought in the last 60 s / 300 s, the share still held; the
                  coin's five biggest buyers, the share still held
  D  re-entry     wallets that sold and bought back on this coin; professional wallets that
                  round-tripped here; professional wallets holding now
  E  the seller   his profit on the sale, time since his buy, share of his bag sold, buys so far

Read against three labels: the coin list (diagnostic), a node member ARRIVING inside our hold (the
mechanism step 27 found), and the book itself. Then money by bin, and a walk-forward cut on days.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import heapq
import os
import time
from bisect import bisect_left
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from kernel import K, fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import ROUTERS, node_ids
from cvx_hot_door2 import clock

B = 0.2
LAG = 0.115
HOLD = 15.0
ABS_W = 15.0
FRESH = 600.0

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 900)
pd.set_option("display.max_columns", 60)

FACTS = [
    "abs_n", "abs_fail", "abs_rate", "abs_last", "prev_y", "prev_n",
    "nb5", "nw5", "bsol5", "pro_sh5", "rt_sh5", "fresh_sh5", "pro_sh60", "rt_sh60",
    "fresh_sh60", "hhi60",
    "held60", "held300", "top5_held",
    "reent_w", "reent_300", "pro_rt_w", "pro_hold_w", "pro_buy60",
    "s_pnl", "s_hold", "s_frac", "s_nbuys", "s_pro", "s_rt",
]
COLS = ["run", "ev", "day", "c8", "tight", "age", "v", "y15", "y60", "arr6", "arr8", "arrpro"] + FACTS


def auc(x1, x0):
    x1 = np.asarray(x1, dtype=float); x0 = np.asarray(x0, dtype=float)
    x1 = x1[np.isfinite(x1)]; x0 = x0[np.isfinite(x0)]
    if len(x1) < 30 or len(x0) < 30:
        return np.nan
    s0 = np.sort(x0)
    lo = np.searchsorted(s0, x1, side="left"); hi = np.searchsorted(s0, x1, side="right")
    return round(float(((lo + hi) / 2.0 / len(s0)).mean()), 4)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    inv = {v: k for k, v in lab.items()}
    w8 = inv["8fStGV"]
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    is_node = np.isin(T.wallet, NODE)
    coins8 = set(np.unique(T.run_of[np.nonzero(T.wallet == w8)[0]]).tolist())
    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router_all = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()
    nb = len(T.builds)
    cnt = np.bincount(T.build, minlength=nb).astype(np.int64)
    wpb = pd.DataFrame({"b": T.build, "w": T.wallet}).drop_duplicates()
    nwal = np.bincount(wpb.b.to_numpy(), minlength=nb).astype(np.int64)
    is_pro_b = (cnt >= 200) & (nwal <= 50)
    first_t = pd.Series(T.t).groupby(T.wallet).transform("min").to_numpy()
    fresh_all = (T.t - first_t) < FRESH
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    nruns = len(T.start)
    lim = int(os.environ.get("HOTD3_LIMIT", "0"))
    rng = range(nruns if not lim else min(lim, nruns))
    print("tape %s prints  coins %s  %.2f days  pro builds %d  %ds"
          % (f"{T.n:,}", f"{nruns:,}", days, int(is_pro_b.sum()), time.time() - t0), flush=True)

    rows = []
    nan = float("nan")
    for r in rng:
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 60 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]; bu = T.build[a:b]; rt = router_all[a:b]
        fresh = fresh_all[a:b]
        tm = np.maximum.accumulate(t)
        age = tm - c_s[r]
        if age[-1] < 60.0:
            continue
        mine = is_node[a:b]
        pub = ~mine
        big = pub & (side == -1) & (sol >= 1.0)
        if not (big & (age >= 60.0)).any():
            continue
        is_pro = is_pro_b[bu]
        pubbuy = pub & (side == 1)
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
        tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        j60 = np.searchsorted(tm, tm - 60.0, side="left")
        j300 = np.searchsorted(tm, tm - 300.0, side="left")
        cb = np.concatenate(([0.0], np.cumsum(np.where(side == 1, sol, 0.0))))
        buys2 = cb[np.arange(n)] - cb[j2]
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))

        # the frenzy-sells of this coin, at any age, and whether each was absorbed
        cand = np.nonzero(big)[0]
        nb5a = {}
        fz = []
        for k in cand:
            k = int(k)
            nb5a[k] = len(np.unique(bu[j5[k]:k]))
            if nb5a[k] >= 12:
                fz.append(k)
        if not fz:
            continue
        ab_p = np.array(fz)
        ab_tc = np.empty(len(fz)); ab_ok = np.zeros(len(fz), dtype=bool)
        for q, p in enumerate(fz):
            e = int(np.searchsorted(t, t[p] + ABS_W, side="right"))
            ref = rmax[p - 1] if p > 0 else v[p]
            ab_ok[q] = (e > p + 1) and (float(v[p + 1:e].max()) > ref)
            ab_tc[q] = tm[p] + ABS_W

        # the seller's last buy on this coin, as the frozen E reads it
        lastb = {}
        shold = np.full(n, np.nan)
        for i in range(n):
            if side[i] == -1:
                lb = lastb.get(wal[i])
                if lb is not None:
                    shold[i] = tm[i] - lb
            else:
                lastb[wal[i]] = tm[i]

        # two fire lists, each one position per coin at a time: ev 0 = every frenzy-sell,
        # ev 1 = the frozen E (evidence 1.12) with its OWN occupancy
        fires = []
        for ev in (0, 1):
            last = -1
            for k in fz:
                if age[k] < 60.0 or k <= last:
                    continue
                if ev == 1 and not (nb5a[k] >= 15 and buys2[k] >= 2.0 and stall[k] <= 20.0
                                    and shold[k] <= 30.0):
                    continue
                ei = fill_idx(t, k, LAG)
                jx = max(int(np.searchsorted(t, t[ei] + HOLD, side="right") - 1), ei)
                xi = fill_idx(t, jx, LAG)
                fires.append((k, ei, xi, net(float(v[ei]), float(v[xi]), B),
                              clock(t, v, k, 60.0), ev))
                last = xi
        if not fires:
            continue
        fset = {}
        for q, f in enumerate(fires):
            fset.setdefault(f[0], []).append(q)
        nodebuy_t = tm[mine & (side == 1)]
        w8buy_t = tm[(wal == w8) & (side == 1)]
        probuy_t = tm[pubbuy & is_pro]
        c8 = int(r in coins8)

        pos = {}; cost = {}; lastbuy = {}; nbuys = {}; btot = {}
        sold = set(); reent = set(); reent_t = []
        pro_rt = set(); pro_hold = set()
        prev_ys = ([], [])

        def held(jj, k):
            pb = pubbuy[jj:k]
            if not pb.any():
                return nan
            ww = wal[jj:k][pb]; tk = tokd[jj:k][pb]
            uw, iv = np.unique(ww, return_inverse=True)
            bw = np.bincount(iv, weights=tk)
            if bw.sum() <= 0:
                return nan
            pn = np.array([pos.get(x, 0.0) for x in uw])
            return float(np.minimum(pn, bw).sum() / bw.sum())

        def comp(jj, k):
            pb = pubbuy[jj:k]
            if not pb.any():
                return nan, nan, nan
            return (float(is_pro[jj:k][pb].mean()), float(rt[jj:k][pb].mean()),
                    float(fresh[jj:k][pb].mean()))

        def arrived(ts, lo, hi):
            q = int(np.searchsorted(ts, lo, side="right"))
            return int(q < len(ts) and ts[q] <= hi)

        for i in range(n):
            for q in fset.get(i, ()):
                k, ei, xi, y15, y60, ev = fires[q]
                w = wal[k]
                p = pos.get(w, 0.0)
                tk = tokd[k]
                avgc = cost.get(w, 0.0) / p if p > 0 else nan
                pnow = sol[k] / tk if tk > 0 else nan
                s_pnl = (pnow / avgc - 1.0) * 100.0 if (np.isfinite(avgc) and avgc > 0) else nan
                lb = lastbuy.get(w)
                s_hold = tm[k] - lb if lb is not None else nan
                s_frac = min(tk / p, 1.5) if p > 0 else nan
                tight = int(nb5a[k] >= 15 and buys2[k] >= 2.0 and stall[k] <= 20.0
                            and np.isfinite(s_hold) and s_hold <= 30.0)
                m = (ab_p < k) & (ab_tc <= tm[k])
                na = int(ab_ok[m].sum()); nall = int(m.sum())
                pb5 = pubbuy[j5[k]:k]
                pro5, rt5, fr5 = comp(j5[k], k)
                pro60, rt60, fr60 = comp(j60[k], k)
                bb = bu[j60[k]:k][pub[j60[k]:k]]
                if len(bb):
                    _, cc = np.unique(bb, return_counts=True)
                    hhi = float(((cc / cc.sum()) ** 2).sum())
                else:
                    hhi = nan
                top = heapq.nlargest(5, btot.items(), key=lambda kv: kv[1])
                tb = sum(x[1] for x in top)
                top5 = (sum(min(pos.get(x[0], 0.0), x[1]) for x in top) / tb) if tb > 0 else nan
                ft = t[ei]
                rows.append((
                    r, ev, int(T.day[a + k]), c8, tight, float(age[k]), float(vbef[k]), y15, y60,
                    arrived(nodebuy_t, ft, ft + HOLD), arrived(w8buy_t, ft, ft + HOLD),
                    arrived(probuy_t, ft, ft + HOLD),
                    float(na), float(nall - na), (na / nall) if nall else nan,
                    float(ab_ok[m][-1]) if nall else nan,
                    float(np.mean(prev_ys[ev])) if prev_ys[ev] else nan, float(len(prev_ys[ev])),
                    float(nb5a[k]), float(len(np.unique(wal[j5[k]:k][pb5]))),
                    float(sol[j5[k]:k][pb5].sum()), pro5, rt5, fr5, pro60, rt60, fr60, hhi,
                    held(j60[k], k), held(j300[k], k), top5,
                    float(len(reent)),
                    float(len(reent_t) - bisect_left(reent_t, tm[k] - 300.0)),
                    float(len(pro_rt)), float(len(pro_hold)),
                    float(sol[j60[k]:k][(pubbuy & is_pro)[j60[k]:k]].sum()),
                    s_pnl, s_hold, s_frac, float(nbuys.get(w, 0)), float(is_pro[k]),
                    float(rt[k])))
                prev_ys[ev].append(y15)
            if mine[i]:
                continue
            w = wal[i]
            if side[i] == 1:
                if w in sold:
                    reent.add(w)
                    reent_t.append(tm[i])
                pos[w] = pos.get(w, 0.0) + tokd[i]
                cost[w] = cost.get(w, 0.0) + sol[i]
                btot[w] = btot.get(w, 0.0) + tokd[i]
                lastbuy[w] = tm[i]
                nbuys[w] = nbuys.get(w, 0) + 1
                if is_pro[i]:
                    pro_hold.add(w)
            else:
                p = pos.get(w, 0.0)
                if p > 0:
                    cost[w] = cost.get(w, 0.0) * max(p - tokd[i], 0.0) / p
                pos[w] = max(p - tokd[i], 0.0)
                if w in lastbuy:
                    sold.add(w)
                    if is_pro[i]:
                        pro_rt.add(w)
                if pos[w] <= 0.02 * btot.get(w, 0.0):
                    pro_hold.discard(w)
        if r % 20000 == 0:
            print("  run %s  fires %s  %ds" % (f"{r:,}", f"{len(rows):,}", time.time() - t0),
                  flush=True)

    D = pd.DataFrame(rows, columns=COLS)
    D.to_parquet(data_file("cvx_hot_door3.parquet"), index=False)
    Ft = D[D.ev == 1]
    D0 = D
    D = D[D.ev == 0]
    print("\nfrenzy fires %s over %s coins  on its coins %.1f %%  tight %s  node arrives %.1f %%  %ds"
          % (f"{len(D):,}", f"{D.run.nunique():,}", 100 * D.c8.mean(), f"{len(Ft):,}",
             100 * D.arr6.mean(), time.time() - t0), flush=True)
    report(D0, days)


def book(d, days, ycol="y15"):
    dd = d.assign(y=d[ycol])
    s = dd.y.sum()
    top = dd.y.nlargest(max(1, int(round(0.01 * len(dd))))).sum()
    c = cell(dd, days=days)
    c["body"] = round(s - top, 2)
    c["maxcoin"] = round(100 * dd.groupby("run").y.sum().max() / s, 1) if s > 0 else np.nan
    c["its"] = round(100 * dd.c8.mean(), 1)
    return c


def bins(x, k):
    ok = np.isfinite(x)
    edges = np.unique(np.nanquantile(x[ok], np.linspace(0, 1, k + 1))) if ok.any() else []
    return ok, edges


def report(D, days):
    Ft = D[D.ev == 1] if "ev" in D else D[D.tight == 1]
    D = D[D.ev == 0] if "ev" in D else D
    base = [dict(slice="frenzy fires", **book(D, days)), dict(slice="tight fires", **book(Ft, days))]
    for nm, col, val in (("node arrives in hold", "arr6", 1), ("node does not", "arr6", 0),
                         ("8fStGV arrives in hold", "arr8", 1),
                         ("a pro build buys in hold", "arrpro", 1),
                         ("no pro build", "arrpro", 0)):
        base.append(dict(slice="frenzy, " + nm, **book(D[D[col] == val], days)))
        base.append(dict(slice="tight, " + nm, **book(Ft[Ft[col] == val], days)))
    for nm, m in (("its coins", 1), ("other coins", 0)):
        base.append(dict(slice="tight, " + nm, **book(Ft[Ft.c8 == m], days)))
    show(base, "0. the frozen event, and the arrival split")

    out = []
    for f in FACTS:
        x = D[f].to_numpy(dtype=float)
        xt = Ft[f].to_numpy(dtype=float)
        out.append(dict(fact=f, cov=round(float(np.isfinite(x).mean()), 3),
                        c8=auc(x[D.c8 == 1], x[D.c8 == 0]),
                        arrive=auc(x[D.arr6 == 1], x[D.arr6 == 0]),
                        arrpro=auc(x[D.arrpro == 1], x[D.arrpro == 0]),
                        win=auc(x[D.y15 > 0], x[D.y15 <= 0]),
                        win_tight=auc(xt[Ft.y15 > 0], xt[Ft.y15 <= 0]),
                        p50_its=round(float(np.nanmedian(x[D.c8 == 1])), 3)
                        if np.isfinite(x[D.c8 == 1]).any() else np.nan,
                        p50_other=round(float(np.nanmedian(x[D.c8 == 0])), 3)
                        if np.isfinite(x[D.c8 == 0]).any() else np.nan))
    R = pd.DataFrame(out)
    R["dev"] = (R[["c8", "arrive", "win"]] - 0.5).abs().max(axis=1)
    R = R.sort_values("dev", ascending=False)
    print("\n=== 1. AUC of each door fact  (0.5 = nothing): its coins / node arrives in hold / a pro"
          " build buys in hold / the fire wins; frenzy fires, and wins on tight fires")
    print(R.to_string(index=False), flush=True)

    for ev, d, kq in (("frenzy", D, 5), ("tight", Ft, 3)):
        rows = []
        for f in FACTS:
            x = d[f].to_numpy(dtype=float)
            ok, edges = bins(x, kq)
            if (~ok).sum() >= 150:
                rows.append(dict(fact=f, bin="nan", **book(d[~ok], days)))
            uv = np.unique(x[ok])
            if len(uv) <= 6:
                for u in uv:
                    m = ok & (x == u)
                    if m.sum() >= 150:
                        rows.append(dict(fact=f, bin="=%.3g" % u, **book(d[m], days)))
                continue
            if len(edges) < 2:
                continue
            for lo_, hi_ in zip(edges[:-1], edges[1:]):
                m = ok & (x >= lo_) & ((x <= hi_) if hi_ == edges[-1] else (x < hi_))
                if m.sum() >= 150:
                    rows.append(dict(fact=f, bin="%.3g..%.3g" % (lo_, hi_), **book(d[m], days)))
        show(rows, "2. money by bin, %s fires, cap15" % ev)

    # walk-forward: pick a one-sided cut on half the days, score it on the other half
    dl = np.sort(D.day.unique())
    halves = (dl[: len(dl) // 2], dl[len(dl) // 2:])
    for ev, d in (("frenzy", D), ("tight", Ft)):
        rows = []
        for f in FACTS:
            res = {}
            for tag, tr, te in (("A", halves[0], halves[1]), ("B", halves[1], halves[0])):
                dtr = d[d.day.isin(tr)]; dte = d[d.day.isin(te)]
                x = dtr[f].to_numpy(dtype=float)
                if np.isfinite(x).sum() < 100:
                    continue
                best = None
                for qq in np.arange(0.2, 0.81, 0.1):
                    thr = float(np.nanquantile(x, qq))
                    for sgn in (1, -1):
                        m = (dtr[f] >= thr) if sgn == 1 else (dtr[f] <= thr)
                        if m.sum() < 0.25 * len(dtr) or m.sum() < 60:
                            continue
                        sc = float(dtr.y15[m].mean())
                        if best is None or sc > best[0]:
                            best = (sc, thr, sgn)
                if best is None:
                    continue
                sc, thr, sgn = best
                mt = (dte[f] >= thr) if sgn == 1 else (dte[f] <= thr)
                c = book(dte[mt], days)
                res[tag] = (round(sc / B * 100, 2), "%s%.3g" % (">=" if sgn == 1 else "<=", thr),
                            c["pct"], c["pos"], c["n"])
            if len(res) == 2:
                rows.append(dict(fact=f, cutA=res["A"][1], trainA=res["A"][0], testA=res["A"][2],
                                 posA=res["A"][3], nA=res["A"][4], cutB=res["B"][1],
                                 trainB=res["B"][0], testB=res["B"][2], posB=res["B"][3],
                                 nB=res["B"][4],
                                 worst_test=min(res["A"][2], res["B"][2])))
        W = pd.DataFrame(rows).sort_values("worst_test", ascending=False)
        base_a = book(d[d.day.isin(halves[1])], days)["pct"]
        base_b = book(d[d.day.isin(halves[0])], days)["pct"]
        print("\n=== 3. walk-forward one-sided cut, %s fires  (no cut: test A %.2f %%, test B %.2f %%)"
              % (ev, base_a, base_b))
        print(W.to_string(index=False), flush=True)


if __name__ == "__main__":
    main()
