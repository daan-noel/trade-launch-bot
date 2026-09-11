"""Hot-tape node, step 32: the PERMISSION - which frenzies die.

The sentence (evidence 1.13): frozen E x abs_rate <= 0.333 x the member's bracket (take profit
+10 %, stop -25 %, 60 s) = +1.22 %/trade, 5/7, top 1 % 39.7 %. Step 31 shows the losers are not
an exit problem - the stop-outs average -31 % and every tighter reaction cuts more recoveries than
it saves. So the losers are a do-not-enter problem: is anything true AT THE FIRE that tells a
frenzy about to die from one about to run?

  cases     fires that end at the stop (-25 %)
  controls  fires that end at the take profit (+10 %)
  facts     public prints before the fire, the six node wallets dropped from every holder and
            composition fact (7.4 law 20); price facts use every print, as the price does

  OVERHANG     share of recent buys still held; share of held tokens sitting on 20 %+ profit;
               the ten biggest holders' share; holder count; the creator sold / still holds
  HEAT         move over 10 / 30 / 60 s; rise from the 60 s low; the sell against the pool
  COMPOSITION  fresh wallets, one-operator recipes, routers in the last 5 s; distinct recipes and
               wallets; the biggest single buyer's share of the 5 s buying; sell SOL in 5 s;
               big sells in 60 s
  SELLER       his profit, his hold, the share of his bag

Discovery runs on every fire of E (no door, n ~ 3,000); the money check runs behind the door too.
A cut is chosen on half the days and scored on the other half, keeping at least 40 % of fires - a
permission removes the dying frenzies, it does not pick a sliver.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from kernel import K, fill_idx
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import ROUTERS, node_ids
from cvx_hot_door3 import auc
from cvx_hot_exit7 import EXITS, run_exit

B = 0.2
LAG = 0.115
ABS_W = 15.0
ABS_MAX = 0.333
FRESH = 600.0
BRACKET = EXITS[0]

FACTS = ["held60", "held300", "inprofit20", "top10_sh", "holders", "cr_sold", "cr_frac",
         "mv10", "mv30", "mv60", "rise60", "sell_rel", "v", "age",
         "fresh_sh5", "pro_sh5", "rt_sh5", "nb5", "nw5", "whale5", "bsol5", "ssol5", "sellsh5",
         "nbig60", "s_pnl", "s_hold", "s_frac"]

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 600)


def pctv(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return float(((a / b) ** 2 - 1.0) * 100.0)


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
    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router_all = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()
    nb = len(T.builds)
    cnt = np.bincount(T.build, minlength=nb).astype(np.int64)
    wpb = pd.DataFrame({"b": T.build, "w": T.wallet}).drop_duplicates()
    nwal = np.bincount(wpb.b.to_numpy(), minlength=nb).astype(np.int64)
    is_op_b = (cnt >= 200) & (nwal <= 50)
    first_t = pd.Series(T.t).groupby(T.wallet).transform("min").to_numpy()
    fresh_all = (T.t - first_t) < FRESH
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    print("tape %s prints  %ds" % (f"{T.n:,}", time.time() - t0), flush=True)

    rows = []
    nan = float("nan")
    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 60 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]; bu = T.build[a:b]; rt = router_all[a:b]; fresh = fresh_all[a:b]
        tm = np.maximum.accumulate(t)
        age = tm - c_s[r]
        if age[-1] < 60.0:
            continue
        mine = is_node[a:b]
        pub = ~mine
        big = pub & (side == -1) & (sol >= 1.0)
        if not (big & (age >= 60.0)).any():
            continue
        is_op = is_op_b[bu]
        pubbuy = pub & (side == 1)
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
        tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j3 = np.searchsorted(tm, tm - 3.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        j10 = np.searchsorted(tm, tm - 10.0, side="left")
        j30 = np.searchsorted(tm, tm - 30.0, side="left")
        j60 = np.searchsorted(tm, tm - 60.0, side="left")
        j300 = np.searchsorted(tm, tm - 300.0, side="left")
        cb = np.concatenate(([0.0], np.cumsum(np.where(side == 1, sol, 0.0))))
        cbp = np.concatenate(([0.0], np.cumsum(np.where(pubbuy, sol, 0.0))))
        buys2 = cb[np.arange(n)] - cb[j2]
        cb3 = cbp[np.arange(n) + 1] - cbp[j3]
        mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        lastb = {}
        shold = np.full(n, np.nan)
        for i in range(n):
            if side[i] == -1:
                lb = lastb.get(wal[i])
                if lb is not None:
                    shold[i] = tm[i] - lb
            else:
                lastb[wal[i]] = tm[i]
        fz = []
        nb5a = {}
        for k in np.nonzero(big)[0]:
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
        ev = [k for k in fz if age[k] >= 60.0 and nb5a[k] >= 15 and buys2[k] >= 2.0
              and stall[k] <= 20.0 and shold[k] <= 30.0]
        if not ev:
            continue

        # the sentence's fires, each door with its own occupancy under the bracket
        fires = []
        for door in (0, 1):
            last = -1
            for k in ev:
                if k <= last:
                    continue
                if door:
                    m = (ab_p < k) & (ab_tc <= tm[k])
                    if not m.any() or ab_ok[m].mean() > ABS_MAX:
                        continue
                ei = fill_idx(t, k, LAG)
                y, x, why, hold = run_exit(BRACKET, t, v, side, sol, mv, cb3, ei)
                fires.append((k, door, y, why))
                last = x
        fset = {}
        for q, f in enumerate(fires):
            fset.setdefault(f[0], []).append(q)

        creator = wal[0]
        pos = {}; cost = {}; btot = {}; lastbuy = {}
        cr_sold = False

        def held(jj, k):
            pb = pubbuy[jj:k]
            if not pb.any():
                return nan
            uw, iv = np.unique(wal[jj:k][pb], return_inverse=True)
            bw = np.bincount(iv, weights=tokd[jj:k][pb])
            if bw.sum() <= 0:
                return nan
            pn = np.array([pos.get(x, 0.0) for x in uw])
            return float(np.minimum(pn, bw).sum() / bw.sum())

        for i in range(n):
            for q in fset.get(i, ()):
                k, door, y, why = fires[q]
                w = wal[k]
                p = pos.get(w, 0.0)
                tk = tokd[k]
                avgc = cost.get(w, 0.0) / p if p > 0 else nan
                pnow = sol[k] / tk if tk > 0 else nan
                s_pnl = (pnow / avgc - 1.0) * 100.0 if (np.isfinite(avgc) and avgc > 0) else nan
                lb = lastbuy.get(w)
                spot = v[k - 1] ** 2 / K
                hp = np.array([x for x in pos.values() if x > 0])
                hw = [(x, cost.get(ww, 0.0)) for ww, x in pos.items() if x > 0]
                tot = float(hp.sum()) if len(hp) else 0.0
                if tot > 0:
                    prof = sum(x for x, c in hw if c > 0 and spot / (c / x) - 1.0 >= 0.20)
                    top10 = float(np.sort(hp)[-10:].sum() / tot)
                else:
                    prof = 0.0; top10 = nan
                pb5 = pubbuy[j5[k]:k]
                ps5 = pub[j5[k]:k] & (side[j5[k]:k] == -1)
                bs = sol[j5[k]:k][pb5]
                if len(bs):
                    _, iv = np.unique(wal[j5[k]:k][pb5], return_inverse=True)
                    whale = float(np.bincount(iv, weights=bs).max() / bs.sum())
                    fr5 = float(fresh[j5[k]:k][pb5].mean()); op5 = float(is_op[j5[k]:k][pb5].mean())
                    rt5 = float(rt[j5[k]:k][pb5].mean())
                else:
                    whale = fr5 = op5 = rt5 = nan
                ss5 = float(sol[j5[k]:k][ps5].sum()); bs5 = float(bs.sum())
                lo60 = float(v[j60[k]:k].min()) if k > j60[k] else v[k - 1]
                crp = btot.get(creator, 0.0)
                rows.append((
                    r, door, int(T.day[a + k]), y, why,
                    held(j60[k], k), held(j300[k], k), prof / tot if tot > 0 else nan, top10,
                    float(len(hp)), float(cr_sold),
                    pos.get(creator, 0.0) / crp if crp > 0 else nan,
                    pctv(v[k - 1], v[max(int(j10[k]) - 1, 0)]),
                    pctv(v[k - 1], v[max(int(j30[k]) - 1, 0)]),
                    pctv(v[k - 1], v[max(int(j60[k]) - 1, 0)]),
                    pctv(v[k - 1], lo60), float(sol[k] / vbef[k] * 100.0),
                    float(vbef[k]), float(age[k]),
                    fr5, op5, rt5, float(nb5a[k]), float(len(np.unique(wal[j5[k]:k][pb5]))),
                    whale, bs5, ss5, ss5 / (ss5 + bs5) if (ss5 + bs5) > 0 else nan,
                    float(((side[j60[k]:k] == -1) & (sol[j60[k]:k] >= 1.0)
                           & pub[j60[k]:k]).sum()),
                    s_pnl, tm[k] - lb if lb is not None else nan,
                    min(tk / p, 1.5) if p > 0 else nan))
            if mine[i]:
                continue
            w = wal[i]
            if side[i] == 1:
                pos[w] = pos.get(w, 0.0) + tokd[i]
                cost[w] = cost.get(w, 0.0) + sol[i]
                btot[w] = btot.get(w, 0.0) + tokd[i]
                lastbuy[w] = tm[i]
            else:
                p = pos.get(w, 0.0)
                if p > 0:
                    cost[w] = cost.get(w, 0.0) * max(p - tokd[i], 0.0) / p
                pos[w] = max(p - tokd[i], 0.0)
                if w == creator:
                    cr_sold = True

    D = pd.DataFrame(rows, columns=["run", "door", "day", "y", "why"] + FACTS)
    D.to_parquet(data_file("cvx_hot_perm.parquet"), index=False)
    print("fires %s  (door %s)  %ds" % (f"{len(D):,}", f"{int(D.door.sum()):,}",
                                        time.time() - t0), flush=True)
    report(D, days)


def book(d, days):
    s = d.y.sum()
    top = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum()
    c = cell(d, days=days)
    c["body"] = round(s - top, 2)
    c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
    c["sl"] = round(100 * float((d.why == "sl").mean()), 1)
    c["tp"] = round(100 * float((d.why == "tp").mean()), 1)
    return c


def report(D, days):
    N = D[D.door == 0]; Y = D[D.door == 1]
    show([dict(slice="E, no door", **book(N, days)), dict(slice="E, door", **book(Y, days))],
         "0. the sentence under the bracket")

    out = []
    for f in FACTS:
        x = N[f].to_numpy(dtype=float); xd = Y[f].to_numpy(dtype=float)
        out.append(dict(fact=f, cov=round(float(np.isfinite(x).mean()), 3),
                        die_vs_tp=auc(x[N.why == "sl"], x[N.why == "tp"]),
                        die_vs_tp_door=auc(xd[Y.why == "sl"], xd[Y.why == "tp"]),
                        lose=auc(x[N.y <= 0], x[N.y > 0]),
                        p50_die=round(float(np.nanmedian(x[N.why == "sl"])), 3),
                        p50_tp=round(float(np.nanmedian(x[N.why == "tp"])), 3)))
    R = pd.DataFrame(out)
    R["dev"] = (R.die_vs_tp - 0.5).abs()
    R = R.sort_values("dev", ascending=False)
    print("\n=== 1. AUC: stop-outs against take-profits (> 0.5 = the dying frenzy has MORE of it)")
    print(R.to_string(index=False), flush=True)

    for nm, d, kq in (("no door", N, 5), ("door", Y, 3)):
        rows = []
        for f in R.fact:
            x = d[f].to_numpy(dtype=float)
            ok = np.isfinite(x)
            if ok.sum() < 100:
                continue
            uv = np.unique(x[ok])
            if len(uv) <= 3:
                for u in uv:
                    m = ok & (x == u)
                    if m.sum() >= 80:
                        rows.append(dict(fact=f, bin="=%.3g" % u, **book(d[m], days)))
                continue
            edges = np.unique(np.nanquantile(x[ok], np.linspace(0, 1, kq + 1)))
            for lo_, hi_ in zip(edges[:-1], edges[1:]):
                m = ok & (x >= lo_) & ((x <= hi_) if hi_ == edges[-1] else (x < hi_))
                if m.sum() >= 80:
                    rows.append(dict(fact=f, bin="%.3g..%.3g" % (lo_, hi_), **book(d[m], days)))
        show(rows, "2. money by bin, E %s, bracket" % nm)

    dl = np.sort(D.day.unique())
    halves = (dl[: len(dl) // 2], dl[len(dl) // 2:])
    for nm, d in (("no door", N), ("door", Y)):
        rows = []
        for f in FACTS:
            res = {}
            for tag, tr, te in (("A", halves[0], halves[1]), ("B", halves[1], halves[0])):
                dtr = d[d.day.isin(tr)]; dte = d[d.day.isin(te)]
                x = dtr[f].to_numpy(dtype=float)
                if np.isfinite(x).sum() < 60:
                    continue
                best = None
                for qq in np.arange(0.1, 0.91, 0.1):
                    thr = float(np.nanquantile(x, qq))
                    for sgn in (1, -1):
                        m = (dtr[f] >= thr) if sgn == 1 else (dtr[f] <= thr)
                        if m.sum() < 0.40 * len(dtr):
                            continue
                        sc = float(dtr.y[m].mean())
                        if best is None or sc > best[0]:
                            best = (sc, thr, sgn)
                if best is None:
                    continue
                sc, thr, sgn = best
                mt = (dte[f] >= thr) if sgn == 1 else (dte[f] <= thr)
                c = book(dte[mt], days)
                res[tag] = ("%s%.3g" % (">=" if sgn == 1 else "<=", thr), c["pct"], c["pos"],
                            c["n"], c["sl"])
            if len(res) == 2:
                rows.append(dict(fact=f, cutA=res["A"][0], testA=res["A"][1], posA=res["A"][2],
                                 nA=res["A"][3], slA=res["A"][4], cutB=res["B"][0],
                                 testB=res["B"][1], posB=res["B"][2], nB=res["B"][3],
                                 slB=res["B"][4], worst=min(res["A"][1], res["B"][1])))
        W = pd.DataFrame(rows).sort_values("worst", ascending=False)
        ba = book(d[d.day.isin(halves[1])], days); bb = book(d[d.day.isin(halves[0])], days)
        print("\n=== 3. walk-forward permission cut, E %s  (no cut: test A %.2f %% sl %.1f, test B "
              "%.2f %% sl %.1f)" % (nm, ba["pct"], ba["sl"], bb["pct"], bb["sl"]))
        print(W.to_string(index=False), flush=True)


if __name__ == "__main__":
    main()
