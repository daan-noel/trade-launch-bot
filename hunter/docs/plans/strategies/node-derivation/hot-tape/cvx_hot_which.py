"""Hot-tape node, step 24: WHICH dump prints does the cleanest member buy?

Step 23 splits the node's edge in two, and only one half is missing:

  firing on EVERY public sell >= 1 SOL, full tape           -4.32 %/trade   0/8
  firing on the sells 8fStGV reacts to, at our own 115 ms   +0.38 %/trade   5/8, and +0.21 % even
                                                                             on the 2/3 where we
                                                                             land BEHIND it

So the event class is right, the seat is survivable, and about 4.7 points sit in WHICH sell. This
run asks what is different about the sells it buys, on the design that holds everything else
constant - the same coin:

  cases     public sells >= 1 SOL that 8fStGV buys within 300 ms of
  controls  public sells >= 1 SOL on the SAME coins that it does not buy within 300 ms of

The candidate facts are mostly about the SELLER, because who is selling is the unpriced half of a
sell print (strategy 1.4): a quick-flip bot taking a profit, the creator leaving, a holder of a
big bag, a professional build, a router user. Beside them, the size against the pool and the coin's
state at that moment.

Everything is known when the sell print lands. The seller is a wallet on the tape, and "the seller
bought this coin 8 s ago" is a fact about the SELLER'S prints, not about the node - but any term
built from it must still be spelled as tape state or ix structure before it can ship (7.4 law 20).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from kernel import K, fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import ROUTERS, node_ids
from toolkit.contrast import strat_rank  # noqa: F401  - re-exported for the later steps

B = 0.2
LAG = 0.115
REACT = 0.300

pd.set_option("display.width", 480)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 50)

FEATS = ["size", "share", "tmv", "s_hold", "s_frac", "s_pnl", "s_nbuys", "s_creator", "s_pro",
         "s_router", "s_new", "mv60", "mv10", "stall", "dd", "n5", "n60", "nb5", "sells10",
         "buys2", "age", "v"]


def pct(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return np.where(b > 0, (a / b) ** 2 - 1.0, np.nan) * 100.0


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
    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router_all = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()
    nb = len(T.builds)
    cnt = np.bincount(T.build, minlength=nb).astype(np.int64)
    wpb = pd.DataFrame({"b": T.build, "w": T.wallet}).drop_duplicates()
    nwal = np.bincount(wpb.b.to_numpy(), minlength=nb).astype(np.int64)
    is_pro_b = (cnt >= 200) & (nwal <= 50)
    is_node = np.isin(T.wallet, NODE)
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()

    runs8 = np.unique(T.run_of[np.nonzero(T.wallet == w8)[0]])
    print("tape %s prints  8fStGV coins %s  %ds"
          % (f"{T.n:,}", f"{len(runs8):,}", time.time() - t0), flush=True)

    rows = []
    for r in runs8:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 30 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]; bu = T.build[a:b]; rt = router_all[a:b]
        mine = is_node[a:b]
        tm = np.maximum.accumulate(t)
        age = tm - c_s[r]
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        vprev = np.concatenate(([np.nan], v[:-1]))
        mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        j60 = np.searchsorted(tm, tm - 60.0, side="left")
        j10 = np.searchsorted(tm, tm - 10.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        creator = wal[0]
        pub = ~mine

        sells = np.nonzero(pub & (side == -1) & (sol >= 1.0) & (age >= 60.0))[0]
        if not len(sells):
            continue
        b8 = np.nonzero((wal == w8) & (side == 1))[0]
        tb8 = tm[b8] if len(b8) else np.array([])

        # per-wallet running position on this coin, in tokens, and first/last buy time and cost
        pos = {}
        cost = {}
        lastbuy = {}
        nbuys = {}
        seenb = set()
        facts = {}
        sset = set(sells.tolist())
        for i in range(n):
            w = wal[i]
            if i in sset:
                p = pos.get(w, 0.0)
                tok_sold = K / v[i] - K / vbef[i]
                frac = tok_sold / p if p > 0 else np.nan
                avgc = cost.get(w, 0.0) / p if p > 0 else np.nan
                pnow = sol[i] / tok_sold if tok_sold > 0 else np.nan
                spnl = (pnow / avgc - 1.0) * 100.0 if (avgc and np.isfinite(avgc)) else np.nan
                lb = lastbuy.get(w)
                facts[i] = (tm[i] - lb if lb is not None else np.nan,
                            min(frac, 1.5) if np.isfinite(frac) else np.nan, spnl,
                            float(nbuys.get(w, 0)), float(w == creator),
                            float(is_pro_b[bu[i]]), float(rt[i]), float(bu[i] not in seenb))
            if side[i] == 1:
                tk = K / vbef[i] - K / v[i]
                pos[w] = pos.get(w, 0.0) + tk
                cost[w] = cost.get(w, 0.0) + sol[i]
                lastbuy[w] = tm[i]
                nbuys[w] = nbuys.get(w, 0) + 1
            else:
                tk = K / v[i] - K / vbef[i]
                p = pos.get(w, 0.0)
                if p > 0:
                    keep = max(p - tk, 0.0) / p
                    cost[w] = cost.get(w, 0.0) * keep
                pos[w] = max(p - tk, 0.0)
            seenb.add(bu[i])

        for k in sells:
            k = int(k)
            hit = False
            if len(tb8):
                q = int(np.searchsorted(tb8, tm[k], side="right"))
                if q < len(tb8) and tb8[q] - tm[k] <= REACT and b8[q] > k:
                    hit = True
            sh, sf, sp, snb, scr, spro, srt, snew = facts[k]
            nb5 = float(len(np.unique(bu[j5[k]:k])))
            ei = fill_idx(t, k, LAG)
            jx = max(int(np.searchsorted(t, t[ei] + 15.0, side="right") - 1), ei)
            y15 = net(float(v[ei]), float(v[fill_idx(t, jx, LAG)]), B)
            rows.append((r, int(T.day[a + k]), int(hit), float(sol[k]),
                         float(sol[k] / vbef[k]) * 100.0, float(mv[k]),
                         sh, sf, sp, snb, scr, spro, srt, snew,
                         float(pct(vprev[k:k + 1], vprev[max(int(j60[k]), 1):max(int(j60[k]), 1) + 1])[0]),
                         float(pct(vprev[k:k + 1], vprev[max(int(j10[k]), 1):max(int(j10[k]), 1) + 1])[0]),
                         float(stall[k]),
                         float(pct(vprev[k:k + 1], rmax[k - 1:k])[0]) if k > 0 else np.nan,
                         float(k - j5[k]), float(k - j60[k]), nb5,
                         float(((side[j10[k]:k] == -1) & (sol[j10[k]:k] >= 1.0)).sum()),
                         float(sol[j2[k]:k][side[j2[k]:k] == 1].sum()),
                         float(age[k]), float(vbef[k]), y15))

    D = pd.DataFrame(rows, columns=["run", "day", "hit"] + FEATS + ["y15"])
    D.to_parquet(data_file("cvx_hot_which.parquet"), index=False)
    C = D[D.hit == 1]; Kc = D[D.hit == 0]
    keep = set(C.run)
    print("\nsells >= 1 on its coins %s   it buys after %s (%.1f %%)   coins with a hit %s  %ds"
          % (f"{len(D):,}", f"{len(C):,}", 100 * len(C) / len(D), f"{len(keep):,}",
             time.time() - t0), flush=True)

    rk = strat_rank(C, Kc[Kc.run.isin(keep)], FEATS)
    print("\n=== 1. the sells it buys against the sells it ignores, SAME COIN  (0.5 = nothing)")
    print(rk.to_string(index=False), flush=True)

    print("\n=== 2. medians")
    print(D.groupby("hit")[FEATS].median().T.round(3).to_string(), flush=True)

    print("\n=== 3. money: our fill 115 ms after the sell, cap15, on its coins")
    rows2 = [dict(slice="every sell >= 1 on its coins", **cell(D.assign(y=D.y15), days=days)),
             dict(slice="the sells it buys", **cell(C.assign(y=C.y15), days=days)),
             dict(slice="the sells it ignores", **cell(Kc.assign(y=Kc.y15), days=days))]
    for f in rk.feature.head(6):
        lo_, hi_ = D[f].quantile([0.25, 0.75])
        top = rk[rk.feature == f].dir.iloc[0]
        m = D[f] >= hi_ if top == "HIGH" else D[f] <= lo_
        s = D[m]
        rows2.append(dict(slice="%s %s %.3g (the side it prefers)" % (f, ">=" if top == "HIGH"
                                                                          else "<=",
                                                                      hi_ if top == "HIGH" else lo_),
                          **cell(s.assign(y=s.y15), days=days)))
    show(rows2, "3. the book")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
