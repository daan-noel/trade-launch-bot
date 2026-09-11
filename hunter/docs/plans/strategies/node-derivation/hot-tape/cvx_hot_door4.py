"""Hot-tape node, step 41: rule 1's door - the arrival of a sell-reactive buyer, and the stop-outs.

Rule 1 (E x established coin x take profit +15 %, stop -25 %, 90 s) books +2.73 % study / +2.53 %
holdout. Two things are left in it:

  the stop-outs      17.6 % of tickets at -30.4 % - the whole cost of the book (1.18)
  the arrival        on 8fStGV's coins, E books +4.46 % on fires BEFORE its first buy there (1.13):
                     a buyer of its kind arriving on the coin after us is the edge a door can add

8fStGV's kind is public behaviour, not a name: a wallet that buys within 300 ms of a public sell
>= 1 SOL. So the new facts count, on this coin and from public prints only (node wallets dropped):

  react_w300    distinct wallets that bought <= 300 ms after a public sell >= 1, in the last 300 s
  react_wall    the same over the coin's life
  react_sol60   SOL they bought in the last 60 s

beside rule 1's overhang, heat and composition facts. Read against the stop-outs, against the
member's later first arrival (diagnostic), and as money - a one-sided cut chosen on half the study
days, scored on the other half, keeping >= 50 % of fires - then the survivors on the holdout.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from bisect import bisect_left
from datetime import datetime, timezone
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import K, fill_idx
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import LOCAL, NODE_NAME, node_ids
from cvx_hot_door3 import auc
from cvx_hot_exit7 import X, run_exit

B = 0.2
LAG = 0.115
T_MIN = datetime(2026, 9, 6, 12, 0, tzinfo=timezone.utc).timestamp()
EXIT = X("tp15 sl25 t90", arm=0.15, cap=90.0)
HOLD_MIN, AGE_MIN = 368, 158.0
FACTS = ["react_w300", "react_wall", "react_sol60", "held60", "held300", "top10_sh", "holders",
         "age", "v", "mv10", "mv60", "rise60", "nb5", "nw5", "bsol5", "ssol5", "whale5",
         "sell_rel", "s_pnl", "s_hold"]

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 400)


def pctv(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return float(((a / b) ** 2 - 1.0) * 100.0)


def run(T, c_s, is_node, w8, t_min=-np.inf):
    rows = []
    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 60 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]; bu = T.build[a:b]
        tm = np.maximum.accumulate(t)
        age = tm - c_s[r]
        if age[-1] < AGE_MIN:
            continue
        mine = is_node[a:b]; pub = ~mine
        big = pub & (side == -1) & (sol >= 1.0)
        if not (big & (age >= AGE_MIN) & (t >= t_min)).any():
            continue
        pubbuy = pub & (side == 1)
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
        tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j3 = np.searchsorted(tm, tm - 3.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        j10 = np.searchsorted(tm, tm - 10.0, side="left")
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
        # a reactive buy: a public buy landing <= 300 ms after a public sell >= 1 SOL
        bst = tm[big]
        q = np.searchsorted(bst, tm, side="right") - 1
        since_big = np.where(q >= 0, tm - bst[np.maximum(q, 0)], np.inf)
        react = pubbuy & (since_big <= 0.300) & (since_big > 0.0)
        # state pass: holders, positions, seller history, reactive wallets
        lastb = {}; shold = np.full(n, np.nan); pos = {}; cost = {}
        hc = np.zeros(n, dtype=np.int32); holders = 0
        spnl = np.full(n, np.nan)
        rw_first = {}; rw_times = []; rw_ids = []
        rw_all = np.zeros(n, dtype=np.int32)
        for i in range(n):
            hc[i] = holders
            rw_all[i] = len(rw_first)
            w = wal[i]
            if side[i] == -1:
                lb = lastb.get(w)
                if lb is not None:
                    shold[i] = tm[i] - lb
                p = pos.get(w, 0.0)
                if big[i] and p > 0 and tokd[i] > 0 and cost.get(w, 0.0) > 0:
                    spnl[i] = ((sol[i] / tokd[i]) / (cost[w] / p) - 1.0) * 100.0
            else:
                lastb[w] = tm[i]
            if mine[i]:
                continue
            if react[i]:
                rw_first.setdefault(w, tm[i]); rw_times.append(tm[i]); rw_ids.append(w)
            p = pos.get(w, 0.0)
            if side[i] == 1:
                qq = p + tokd[i]; cost[w] = cost.get(w, 0.0) + sol[i]
            else:
                qq = max(p - tokd[i], 0.0)
                if p > 0:
                    cost[w] = cost.get(w, 0.0) * qq / p
            holders += int(qq > 0) - int(p > 0); pos[w] = qq
        rw_times = np.array(rw_times); rw_ids = np.array(rw_ids)
        rsol = np.concatenate(([0.0], np.cumsum(np.where(react, sol, 0.0))))
        ev = []
        for k in np.nonzero(big & (age >= AGE_MIN) & (t >= t_min) & (hc >= HOLD_MIN))[0]:
            k = int(k)
            if buys2[k] < 2.0 or stall[k] > 20.0 or not (shold[k] <= 30.0):
                continue
            if len(np.unique(bu[j5[k]:k])) >= 15:
                ev.append(k)
        if not ev:
            continue
        m8 = tm[(wal == w8) & (side == 1)]
        first8 = m8[0] if len(m8) else np.inf
        last = -1
        for k in ev:
            if k <= last:
                continue
            ei = fill_idx(t, k, LAG)
            y, x, why, hold = run_exit(EXIT, t, v, side, sol, mv, cb3, ei)
            last = x
            lo = bisect_left(rw_times.tolist(), tm[k] - 300.0) if len(rw_times) else 0
            hi = bisect_left(rw_times.tolist(), tm[k]) if len(rw_times) else 0
            pb5 = pubbuy[j5[k]:k]
            bs = sol[j5[k]:k][pb5]
            if len(bs):
                _, iv = np.unique(wal[j5[k]:k][pb5], return_inverse=True)
                whale = float(np.bincount(iv, weights=bs).max() / bs.sum())
            else:
                whale = np.nan
            hp = np.array([x_ for x_ in pos.values() if x_ > 0])  # end-of-coin: not used
            pbw = pubbuy[j60[k]:k]
            def held(jj):
                pb = pubbuy[jj:k]
                return np.nan  # placeholder, overwritten below
            rows.append(dict(
                run=r, day=int(T.day[a + k]), y=y, why=why, hold=hold,
                c8=int(len(m8) > 0), before8=int(tm[k] < first8),
                arrive8=int(first8 > tm[k] and first8 <= tm[k] + 90.0),
                react_w300=float(len(np.unique(rw_ids[lo:hi]))) if hi > lo else 0.0,
                react_wall=float(rw_all[k]),
                react_sol60=float(rsol[k] - rsol[j60[k]]),
                holders=float(hc[k]), age=float(age[k]), v=float(vbef[k]),
                mv10=pctv(v[k - 1], v[max(int(j10[k]) - 1, 0)]),
                mv60=pctv(v[k - 1], v[max(int(j60[k]) - 1, 0)]),
                rise60=pctv(v[k - 1], float(v[j60[k]:k].min()) if k > j60[k] else v[k - 1]),
                nb5=float(len(np.unique(bu[j5[k]:k]))),
                nw5=float(len(np.unique(wal[j5[k]:k][pb5]))),
                bsol5=float(bs.sum()), ssol5=float(sol[j5[k]:k][pub[j5[k]:k] & (side[j5[k]:k] == -1)].sum()),
                whale5=whale, sell_rel=float(sol[k] / vbef[k] * 100.0),
                s_pnl=float(spnl[k]), s_hold=float(shold[k]), k=k))
        # held-share and top-10 facts need positions AT the fire: a second, cheap pass
        if rows and rows[-1]["run"] == r:
            ks = {rr["k"]: rr for rr in rows if rr["run"] == r}
            pos2 = {}; cand = sorted(ks)
            ci = 0
            for i in range(n):
                while ci < len(cand) and cand[ci] == i:
                    rr = ks[i]
                    for tag, jj in (("held60", j60[i]), ("held300", j300[i])):
                        pb = pubbuy[jj:i]
                        if pb.any():
                            uw, iv = np.unique(wal[jj:i][pb], return_inverse=True)
                            bw = np.bincount(iv, weights=tokd[jj:i][pb])
                            pn = np.array([pos2.get(x_, 0.0) for x_ in uw])
                            rr[tag] = float(np.minimum(pn, bw).sum() / bw.sum()) if bw.sum() > 0 else np.nan
                        else:
                            rr[tag] = np.nan
                    hp = np.array([x_ for x_ in pos2.values() if x_ > 0])
                    rr["top10_sh"] = float(np.sort(hp)[-10:].sum() / hp.sum()) if len(hp) else np.nan
                    ci += 1
                if mine[i]:
                    continue
                w = wal[i]; p = pos2.get(w, 0.0)
                pos2[w] = p + tokd[i] if side[i] == 1 else max(p - tokd[i], 0.0)
    return pd.DataFrame(rows)


def load_study():
    lab = node_ids(); inv = {v: k for k, v in lab.items()}
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    return T, tok.reindex(T.mints).c_s.to_numpy(), np.isin(T.wallet, list(lab)), inv["8fStGV"], \
        (T.t.max() - T.t.min()) / 86400.0, -np.inf


def load_holdout():
    T = Tape(str(data_file("cvx_holdout_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    W = pd.read_parquet(data_file("cvx_holdout_wallets.parquet"))
    R = pd.read_csv(LOCAL / "solo-traders.csv", usecols=["wallet_address", "node", "use_it"])
    R = R[(R.node == NODE_NAME) & (R.use_it == "yes")]
    node = W[W.address.isin(R.wallet_address)]
    w8 = int(node[node.address.str.startswith("8fStGV")].wallet_id.iloc[0])
    tok = pd.read_parquet(data_file("cvx_holdout_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    return T, tok.reindex(T.mints).c_s.to_numpy(), np.isin(T.wallet, node.wallet_id.to_numpy()), \
        w8, (T.t.max() - T_MIN) / 86400.0, T_MIN


def book(d, days):
    s = d.y.sum()
    top = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum() if len(d) else 0.0
    c = cell(d, days=days)
    c["body"] = round(s - top, 2)
    c["sl"] = round(100 * float((d.why == "sl").mean()), 1) if len(d) else np.nan
    return c


def main() -> None:
    t0 = time.time()
    S = run(*load_study()[:4]); sdays = load_study()[4]
    S.to_parquet(data_file("cvx_hot_door4_study.parquet"), index=False)
    T, c_s, is_node, w8, hdays, tmin = load_holdout()
    H = run(T, c_s, is_node, w8, t_min=tmin)
    H.to_parquet(data_file("cvx_hot_door4_holdout.parquet"), index=False)
    print("rule 1 fires: study %s, holdout %s  %ds" % (f"{len(S):,}", f"{len(H):,}", time.time() - t0))
    show([dict(tape="study", **book(S, sdays)), dict(tape="holdout", **book(H, hdays))],
         "0. rule 1 as booked here")
    for nm, d in (("study", S), ("holdout", H)):
        rows = []
        for f in FACTS:
            x = d[f].to_numpy(dtype=float)
            rows.append(dict(fact=f, stop_vs_tp=auc(x[d.why == "sl"], x[d.why == "tp"]),
                             arrive8=auc(x[(d.arrive8 == 1)], x[(d.before8 == 1) & (d.arrive8 == 0)]
                                         if ((d.before8 == 1) & (d.arrive8 == 0)).sum() >= 30
                                         else x[d.arrive8 == 0]),
                             lose=auc(x[d.y <= 0], x[d.y > 0])))
        R = pd.DataFrame(rows)
        R["dev"] = (R.stop_vs_tp - 0.5).abs()
        print("\n=== 1. AUC on %s: stop-outs against take-profits; the member arriving within 90 s "
              "against not (diagnostic); losers against winners" % nm)
        print(R.sort_values("dev", ascending=False).to_string(index=False))
    dl = np.sort(S.day.unique()); halves = (dl[: len(dl) // 2], dl[len(dl) // 2:])
    rows = []
    for f in FACTS:
        res = {}
        for tag, tr, te in (("A", halves[0], halves[1]), ("B", halves[1], halves[0])):
            dtr = S[S.day.isin(tr)]; dte = S[S.day.isin(te)]
            x = dtr[f].to_numpy(dtype=float)
            if np.isfinite(x).sum() < 60:
                continue
            best = None
            for qq in np.arange(0.1, 0.51, 0.05):
                for sgn, thr in ((1, float(np.nanquantile(x, qq))), (-1, float(np.nanquantile(x, 1 - qq)))):
                    m = (dtr[f] >= thr) if sgn == 1 else (dtr[f] <= thr)
                    if m.sum() < 0.50 * len(dtr):
                        continue
                    sc = float(dtr.y[m].mean())
                    if best is None or sc > best[0]:
                        best = (sc, thr, sgn)
            if best is None:
                continue
            sc, thr, sgn = best
            mt = (dte[f] >= thr) if sgn == 1 else (dte[f] <= thr)
            c = book(dte[mt], sdays)
            res[tag] = (sgn, thr, c["pct"], c["pos"], c["n"])
        if len(res) == 2 and res["A"][0] == res["B"][0]:
            sgn = res["A"][0]; thr = (res["A"][1] + res["B"][1]) / 2.0
            mh = (H[f] >= thr) if sgn == 1 else (H[f] <= thr)
            ch = book(H[mh], hdays)
            rows.append(dict(fact=f, cut="%s%.3g" % (">=" if sgn == 1 else "<=", thr),
                             testA=res["A"][2], posA=res["A"][3], testB=res["B"][2],
                             posB=res["B"][3], worst=min(res["A"][2], res["B"][2]),
                             hold_pct=ch["pct"], hold_pos=ch["pos"], hold_n=ch["n"],
                             hold_sol=ch["sol"], hold_sl=ch["sl"]))
    W = pd.DataFrame(rows).sort_values("worst", ascending=False)
    base = book(H, hdays)
    print("\n=== 2. a one-sided cut chosen on half the study days (>= 50 %% of fires kept), scored on "
          "the other half; both folds must pick the same side; the mean cut on the holdout  "
          "(no cut: study %.2f %%, holdout %.2f %% %s, sl %.1f %%)" %
          (book(S, sdays)["pct"], base["pct"], base["pos"], base["sl"]))
    print(W.to_string(index=False))
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
