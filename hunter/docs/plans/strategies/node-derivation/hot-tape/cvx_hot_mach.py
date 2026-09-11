"""Hot-tape node, step 19: the MACHINE axis, which this node's event study has never used.

THE OBSERVATION THAT NAMES THIS RUN. Strategy 1.4 splits the tape in two:

  IN the price (spent)                  NOT in the price (the only predictive material)
  every buy and sell that landed        which MACHINE class made the last prints
  windowed flow, net flow, buy share    how many INDEPENDENT machines are acting
  volume, any % off the low or high     whether the cohort that pushed this coin is still in
  any zigzag, any breakout              whether the creator has sold

Every feature in the entire hot-tape series - 1.6's `wake`/`buy2`/`gap`/`vol20`, 1.7's model,
1.8's door census, and steps 15-18 of this file family - sits in the LEFT column. `mv60`,
`dd_peak`, `n60`, `stall`, `rise10` are the price path and its counts. The axis the strategy file
says is the only unpriced one has never been put into this node's event.

So this run asks one question, on the design that works (within the mint, so the coin is constant):

    do MACHINE facts separate the moments of the two members that pay at our seat?

  cases      buys of `8fStGV` and `AbQcLH` - the two whose own decisions book +2.3 %/trade,
             8/8 and 7/7 days, at the RACE seat under a 15 s clock
  controls   non-buy prints on the SAME coin within +/- 60 s
  also       the same design for the other four members, so the contrast between the two animals
             is read on the machine axis rather than only on the price axis

Nothing here is a term yet - a machine fact becomes a term as ix STRUCTURE, never as a wallet
(7.4 law 20). This step only answers whether the unpriced axis carries anything on this node.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from cvx import DAY0
from tape import Tape
from cvx_hot_event import ROUTERS, node_ids

CTRL_WIN = 60.0
N_CTRL = 4
SEED = 20260910
GOOD = ("8fStGV", "AbQcLH")

pd.set_option("display.width", 460)
pd.set_option("display.max_rows", 300)
pd.set_option("display.max_columns", 40)


def strat_rank(C, Kc, cols):
    num = np.zeros(len(cols)); den = np.zeros(len(cols))
    cg = {r: g[cols].to_numpy() for r, g in C.groupby("run", sort=False)}
    kg = {r: g[cols].to_numpy() for r, g in Kc.groupby("run", sort=False)}
    for r, cm in cg.items():
        km = kg.get(r)
        if km is None:
            continue
        for j in range(len(cols)):
            kv = km[:, j]; kv = kv[np.isfinite(kv)]
            if len(kv) < 2:
                continue
            cv = cm[:, j]; cv = cv[np.isfinite(cv)]
            if not len(cv):
                continue
            kv = np.sort(kv)
            lo = np.searchsorted(kv, cv, side="left")
            hi = np.searchsorted(kv, cv, side="right")
            num[j] += float(((lo + hi) / 2.0 / len(kv)).sum()); den[j] += len(cv)
    out = []
    for j, c in enumerate(cols):
        if den[j]:
            mp = num[j] / den[j]
            out.append(dict(feature=c, n=int(den[j]), pctile=round(mp, 4),
                            dev=round(abs(mp - 0.5), 4), dir="HIGH" if mp > 0.5 else "low"))
    return pd.DataFrame(out).sort_values("dev", ascending=False)


FEATS = ["nb5", "nb20", "nb60", "nb300", "newb20", "newb60", "pro5", "pro20",
         "pro_share20", "rt_share20", "prev_pro", "prev_rt", "prev_new",
         "hhi20", "sellb20", "cr_prints", "cr_sold"]


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()

    # build-level census: a professional build is 200+ prints on few wallets (6.14 spelling)
    nb = len(T.builds)
    cnt = np.bincount(T.build, minlength=nb).astype(np.int64)
    wpb = pd.DataFrame({"b": T.build, "w": T.wallet}).drop_duplicates()
    nw = np.bincount(wpb.b.to_numpy(), minlength=nb).astype(np.int64)
    is_pro = (cnt >= 200) & (nw <= 50)
    print("tape %s prints  builds %s  professional %s  %ds"
          % (f"{T.n:,}", f"{nb:,}", f"{int(is_pro.sum()):,}", time.time() - t0), flush=True)

    is_node = np.isin(T.wallet, NODE)
    w_lab = {i: lab[i] for i in NODE}
    runs = np.unique(T.run_of[np.nonzero(is_node)[0]])
    rng = np.random.default_rng(SEED)

    rows_c, rows_k = [], []
    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 30:
            continue
        t = T.t[a:b]; side = T.side[a:b]; bu = T.build[a:b]
        wal = T.wallet[a:b]; rt = router[a:b]
        mine = is_node[a:b]
        pro = is_pro[bu]
        cases = np.nonzero(mine & (side == 1))[0]
        if not len(cases):
            continue
        creator = bu[0] if n else -1
        tm = np.maximum.accumulate(t)
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        j20 = np.searchsorted(tm, tm - 20.0, side="left")
        j60 = np.searchsorted(tm, tm - 60.0, side="left")
        j300 = np.searchsorted(tm, tm - 300.0, side="left")

        near = np.zeros(n, dtype=bool)
        for k in cases:
            lo = np.searchsorted(t, t[k] - CTRL_WIN, side="left")
            hi = np.searchsorted(t, t[k] + CTRL_WIN, side="right")
            near[lo:hi] = True
        near &= ~mine
        pool = np.nonzero(near)[0]
        if len(pool) < 4:
            continue
        pick = rng.choice(pool, size=min(len(pool), N_CTRL * len(cases)), replace=False)

        def feats(i):
            i = int(i)
            if i < 2:
                return None
            s5, s20, s60, s300 = int(j5[i]), int(j20[i]), int(j60[i]), int(j300[i])
            b20 = bu[s20:i]
            if len(b20) == 0:
                return None
            u20, c20 = np.unique(b20, return_counts=True)
            b60 = bu[s60:i]
            u60 = np.unique(b60)
            prior20 = set(np.unique(bu[:s20]).tolist())
            prior60 = set(np.unique(bu[:s60]).tolist())
            p = c20 / c20.sum()
            n20 = float(i - s20)
            crp = float((bu[:i] == creator).sum())
            crs = float(((bu[:i] == creator) & (side[:i] == 0)).sum())
            return (float(len(np.unique(bu[s5:i]))), float(len(u20)), float(len(u60)),
                    float(len(np.unique(bu[s300:i]))),
                    float(sum(1 for x in u20.tolist() if x not in prior20)),
                    float(sum(1 for x in u60.tolist() if x not in prior60)),
                    float(pro[s5:i].sum()), float(pro[s20:i].sum()),
                    float(pro[s20:i].sum() / max(n20, 1)),
                    float(rt[s20:i].sum() / max(n20, 1)),
                    float(pro[i - 1]), float(rt[i - 1]),
                    float(bu[i - 1] not in prior20),
                    float((p * p).sum()),
                    float((side[s20:i] == 0).sum() / max(n20, 1)),
                    crp, crs)

        for k in cases:
            f = feats(k)
            if f is not None:
                rows_c.append((r, w_lab[int(wal[k])]) + f)
        for k in pick:
            f = feats(k)
            if f is not None:
                rows_k.append((r, "ctrl") + f)
        if r % 3000 == 0:
            print("  run %d  cases %s  %ds" % (r, f"{len(rows_c):,}", time.time() - t0),
                  flush=True)

    C = pd.DataFrame(rows_c, columns=["run", "w"] + FEATS)
    Kc = pd.DataFrame(rows_k, columns=["run", "w"] + FEATS)
    C.to_parquet(data_file("cvx_hot_mach_cases.parquet"), index=False)
    Kc.to_parquet(data_file("cvx_hot_mach_ctrl.parquet"), index=False)
    print("\ncases %s  controls %s  coins %s  %ds"
          % (f"{len(C):,}", f"{len(Kc):,}", f"{C.run.nunique():,}", time.time() - t0), flush=True)

    print("\n=== A. machine facts at THEIR moment vs a non-buy print on the SAME coin")
    for nm, sub in (("the two that pay (8fStGV, AbQcLH)", C[C.w.isin(GOOD)]),
                    ("the other four", C[~C.w.isin(GOOD)])):
        keep = set(sub.run) & set(Kc.run)
        d = strat_rank(sub[sub.run.isin(keep)], Kc[Kc.run.isin(keep)], FEATS)
        print("\n  %s   cases %s  coins %s" % (nm, f"{len(sub):,}", f"{len(keep):,}"))
        print(d.to_string(index=False), flush=True)

    print("\n=== B. the two animals against each other, on the coins they share")
    g = C[C.w.isin(GOOD)]
    o = C[~C.w.isin(GOOD)]
    keep = set(g.run) & set(o.run)
    d = strat_rank(g[g.run.isin(keep)], o[o.run.isin(keep)], FEATS)
    print("  shared coins %s   good %s   other %s"
          % (f"{len(keep):,}", f"{len(g[g.run.isin(keep)]):,}", f"{len(o[o.run.isin(keep)]):,}"))
    print(d.to_string(index=False), flush=True)

    print("\n=== C. medians")
    M = pd.concat([C.assign(grp=np.where(C.w.isin(GOOD), "GOOD", "other")),
                   Kc.assign(grp="ctrl")])
    print(M.groupby("grp")[FEATS].median().T.round(3).to_string(), flush=True)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
