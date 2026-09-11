"""Hot-tape node, step 26: the DOOR for the frenzy-absorbed sell.

Step 25 fills E and leaves D as the one empty slot:

  sell >= 1 SOL . 15+ builds in the last 5 s . 2+ SOL bought in the last 2 s . new high <= 20 s
  . seller bought <= 30 s ago . cap15
      on 8fStGV's own coins     +1.30 %/trade   5/7 days   body +1.20 SOL   biggest coin 8.2 %
      on every other coin       -3.96 %/trade   0/7 days

The event reproduces its selection inside its coins and loses everywhere else, so five points sit
in WHICH COIN. Its coin list is a hindsight door and a wallet term (7.4 law 20); the question here
is what about those coins is knowable, publicly, at the moment the event fires.

Every coin fact is computed from the coin's prints BEFORE the fire. Fires are at age >= 60 s, so a
fact dated at age 60 s is causal here - the step-17 lookahead was a fire at age 20 s using it.

  cases     fires on a coin 8fStGV touches     controls   fires on every other coin
  then      money, on ALL fires, by each door fact that separates the two
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from kernel import fill_idx, net
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import node_ids

B = 0.2
LAG = 0.115

pd.set_option("display.width", 490)
pd.set_option("display.max_rows", 900)
pd.set_option("display.max_columns", 60)

DOOR = ["age", "v", "v_peak", "dd_life", "prints", "wallets", "builds", "live60", "busy60",
        "wal60", "buyshare", "sol_in", "n_frenzy", "n_bigsell", "cr_sold", "top_wal_share",
        "rate300"]


def pct(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return np.where(b > 0, (a / b) ** 2 - 1.0, np.nan) * 100.0


def clock(t, v, k, cap):
    ei = fill_idx(t, k, LAG)
    j = max(int(np.searchsorted(t, t[ei] + cap, side="right") - 1), ei)
    return net(float(v[ei]), float(v[fill_idx(t, j, LAG)]), B)


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
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    print("tape %s prints  %ds" % (f"{T.n:,}", time.time() - t0), flush=True)

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
        if age[-1] < 60.0:
            continue
        pub = ~is_node[a:b]
        big = pub & (sol >= 1.0) & (side == -1) & (age >= 60.0)
        if not big.any():
            continue
        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        j300 = np.searchsorted(tm, tm - 300.0, side="left")
        cb = np.concatenate(([0.0], np.cumsum(np.where(side == 1, sol, 0.0))))
        cs = np.concatenate(([0.0], np.cumsum(np.where(side == -1, sol, 0.0))))
        buys2 = cb[np.arange(n)] - cb[j2]
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        i60 = int(np.searchsorted(age, 60.0, side="right"))
        live60 = float(v[i60 - 1]) if i60 > 0 else np.nan
        busy60 = float(i60)
        wal60 = float(len(np.unique(wal[:i60]))) if i60 > 0 else 0.0
        creator = wal[0]

        lastbuy = {}
        s_hold = np.full(n, np.nan)
        for i in range(n):
            w = wal[i]
            if side[i] == -1:
                lb = lastbuy.get(w)
                if lb is not None:
                    s_hold[i] = tm[i] - lb
            else:
                lastbuy[w] = tm[i]

        cand = np.nonzero(big)[0]
        nf = 0
        nbs = 0
        last = -1
        for k in cand:
            k = int(k)
            nb5 = len(np.unique(bu[j5[k]:k]))
            frenzy = (nb5 >= 12)
            tight = (nb5 >= 15) and (buys2[k] >= 2.0) and (stall[k] <= 20.0) \
                and np.isfinite(s_hold[k]) and (s_hold[k] <= 30.0)
            if frenzy and k > last:
                pw = wal[:k]
                uw, cw = np.unique(pw, return_counts=True)
                rows.append((r, int(T.day[a + k]), int(r in coins8), int(tight),
                             float(age[k]), float(v[k - 1]), float(rmax[k - 1]),
                             float(pct(v[k - 1:k], rmax[k - 1:k])[0]),
                             float(k), float(len(uw)), float(len(np.unique(bu[:k]))),
                             live60, busy60, wal60,
                             float(cb[k] / max(cb[k] + cs[k], 1e-9)), float(cb[k]),
                             float(nf), float(nbs),
                             float(((wal[:k] == creator) & (side[:k] == -1)).any()),
                             float(cw.max() / k), float(k - j300[k]),
                             clock(t, v, k, 15.0), clock(t, v, k, 60.0)))
                last = fill_idx(t, int(np.searchsorted(t, t[k] + 15.0, side="right") - 1), LAG)
            if frenzy:
                nf += 1
            nbs += 1
    D = pd.DataFrame(rows, columns=["run", "day", "c8", "tight"] + DOOR + ["y15", "y60"])
    D.to_parquet(data_file("cvx_hot_door2.parquet"), index=False)
    print("frenzy fires %s over %s coins  (on 8fStGV's coins %s)  tight %s  %ds"
          % (f"{len(D):,}", f"{D.run.nunique():,}", f"{int(D.c8.sum()):,}",
             f"{int(D.tight.sum()):,}", time.time() - t0), flush=True)

    # ---- 1. which coin facts separate its coins, among fires of the SAME event --------------
    out = []
    for f in DOOR:
        x1 = D.loc[D.c8 == 1, f].dropna().to_numpy()
        x0 = D.loc[D.c8 == 0, f].dropna().to_numpy()
        if len(x1) < 50 or len(x0) < 50:
            continue
        s0 = np.sort(x0)
        lo = np.searchsorted(s0, x1, side="left"); hi = np.searchsorted(s0, x1, side="right")
        auc = float(((lo + hi) / 2.0 / len(s0)).mean())
        out.append(dict(fact=f, auc=round(auc, 4), dev=round(abs(auc - 0.5), 4),
                        its_coins_p50=round(float(np.median(x1)), 3),
                        other_p50=round(float(np.median(x0)), 3)))
    R = pd.DataFrame(out).sort_values("dev", ascending=False)
    print("\n=== 1. coin facts at the fire: its coins against every other coin, same event")
    print(R.to_string(index=False), flush=True)

    # ---- 2. money, on ALL fires, by the door facts ------------------------------------------
    def row(d, nm):
        dd = d.copy(); dd["y"] = dd.y15
        s = dd.y.sum()
        top = dd.y.nlargest(max(1, int(round(0.01 * len(dd))))).sum()
        c = cell(dd, days=days)
        c["top1"] = round(100 * top / s, 1) if s > 0 else np.nan
        c["body"] = round(s - top, 2)
        c["its_share"] = round(100 * dd.c8.mean(), 1)
        return dict(door=nm, **c)

    rows2 = [row(D, "none (frenzy fires)"), row(D[D.tight == 1], "none (tight fires)")]
    for f in R.fact.head(8):
        q = D[f].quantile([0.2, 0.4, 0.6, 0.8]).to_numpy()
        up = R[R.fact == f].its_coins_p50.iloc[0] > R[R.fact == f].other_p50.iloc[0]
        for lo_, hi_, tag in ((-np.inf, q[0], "q1"), (q[0], q[1], "q2"), (q[1], q[2], "q3"),
                              (q[2], q[3], "q4"), (q[3], np.inf, "q5")):
            s = D[(D[f] > lo_) & (D[f] <= hi_)]
            if len(s) < 200:
                continue
            rows2.append(row(s, "%s %s (%.3g..%.3g)%s" % (f, tag, lo_, hi_,
                                                          "  <- its side" if (up and tag == "q5")
                                                          or (not up and tag == "q1") else "")))
    show(rows2, "2. money on every frenzy fire, by coin fact, cap15")

    # ---- 3. the best two door facts stacked, on the tight event -----------------------------
    top2 = list(R.fact.head(2))
    rows3 = []
    for ev, d in (("frenzy", D), ("tight", D[D.tight == 1])):
        m = pd.Series(True, index=d.index)
        for f in top2:
            up = R[R.fact == f].its_coins_p50.iloc[0] > R[R.fact == f].other_p50.iloc[0]
            thr = D[f].median()
            m &= (d[f] >= thr) if up else (d[f] <= thr)
        rows3.append(row(d[m], "%s + %s" % (ev, " & ".join(top2))))
        rows3.append(row(d[~m], "%s, the rest" % ev))
    show(rows3, "3. the two best coin facts together, cap15")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
