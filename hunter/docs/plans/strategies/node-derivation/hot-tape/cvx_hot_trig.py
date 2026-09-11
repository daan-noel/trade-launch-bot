"""Hot-tape node, step 21: THEIR TRIGGER PRINT, and therefore their real reaction time.

WHY THIS IS THE GATING RUN. 1.11 finds three of the six members booking +2.27 %/trade on 8 of 8
days - but only at the RACE seat, sequenced BEFORE their print. At our own fill the same decisions
read -0.66 %. So the whole of that money depends on one unanswered question:

    are they REACTING to a print, or are they firing on a STATE?

  reacting to a print  ->  we are behind that print too, and we can only beat them to it if their
                           own latency is longer than our 115 ms. Their trigger becomes our event
                           and the seat is a race we may or may not win.
  firing on a state    ->  nobody is racing to a single print. Anyone computing the same state can
                           be there first, and the RACE seat is reachable in principle.

1.9 retracted the earlier attempt at this. It measured "seconds since the last public buy >= 1 SOL"
and read p50 1.288 s - but on a coin where big buys arrive every couple of seconds that statistic
reads about a second WHETHER OR NOT ANYONE IS REACTING. A waiting time is not a reaction time.

THE MEASUREMENT THAT IS VALID. Excess intensity against the coin's own local print rate.

  cases     every buy of the wallet group
  controls  random times on the SAME coin within +/- 60 s of a case, spliced in by timestamp
  for each  look back 5 s and histogram every earlier print by (class, lag)
  excess    case histogram minus control histogram, per class per lag bin

The control absorbs the coin's own arrival rate exactly, so a flat excess means "no relationship"
and a SPIKE at some lag means "they react to that class of print, that many milliseconds later".
The spike's position is their latency; its height over the null is how much of their firing that
class explains. A waiting-time statistic cannot produce either number.

Every classified print is node-blind. The one class that names the node is reported as a
DIAGNOSTIC only and can never be a term (7.4 law 20).
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

LOOKBACK = 5.0
N_CTRL = 4
CTRL_WIN = 60.0
SEED = 20260911
PAY = ("8fStGV", "AbQcLH", "49uohd")
PERW = ("8fStGV", "AbQcLH", "49uohd", "64hP97", "omegoM", "sssssw")

EDGES = np.array([0.0, 0.025, 0.050, 0.075, 0.100, 0.150, 0.200, 0.300, 0.400,
                  0.600, 0.900, 1.400, 2.200, 3.400, 5.000])
LBL = ["0-25", "25-50", "50-75", "75-100", "100-150", "150-200", "200-300", "300-400",
       "400-600", "600-900", "0.9-1.4s", "1.4-2.2s", "2.2-3.4s", "3.4-5.0s"]

CLASSES = ["any", "buy", "buy>=0.5", "buy>=1", "sell", "sell>=0.5", "sell>=1",
           "up>=1%", "up>=3%", "down>=1%", "down>=2%", "router_buy", "pro", "new_build",
           "burst_start", "NODE(diag)"]

pd.set_option("display.width", 500)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 40)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router_all = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()

    nb = len(T.builds)
    cnt = np.bincount(T.build, minlength=nb).astype(np.int64)
    wpb = pd.DataFrame({"b": T.build, "w": T.wallet}).drop_duplicates()
    nwal = np.bincount(wpb.b.to_numpy(), minlength=nb).astype(np.int64)
    is_pro_b = (cnt >= 200) & (nwal <= 50)

    is_node = np.isin(T.wallet, NODE)
    runs = np.unique(T.run_of[np.nonzero(is_node)[0]])
    rng = np.random.default_rng(SEED)
    nC, nL = len(CLASSES), len(LBL)
    print("tape %s prints  coins %s  %ds" % (f"{T.n:,}", f"{len(runs):,}", time.time() - t0),
          flush=True)

    GRPS = ("pay", "rest") + PERW
    H = {g: np.zeros((nC, nL)) for g in GRPS}
    Hc = {g: np.zeros((nC, nL)) for g in GRPS}
    ncase = {g: 0 for g in GRPS}
    nctrl = {g: 0 for g in GRPS}

    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 30:
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        wal = T.wallet[a:b]; bu = T.build[a:b]
        mine = is_node[a:b]
        rt = router_all[a:b]
        pro = is_pro_b[bu]
        tm = np.maximum.accumulate(t)

        # per-print facts, all node-blind except the diagnostic
        pub = ~mine
        mv = np.concatenate(([0.0], ((v[1:] / v[:-1]) ** 2 - 1.0) * 100.0))
        first_seen = np.zeros(n, dtype=bool)
        seen = set()
        for i2 in range(n):
            x = bu[i2]
            if x not in seen:
                seen.add(x)
                first_seen[i2] = True
        gap = np.concatenate(([1e9], np.diff(tm)))
        burst = gap >= 0.4

        C = np.empty((nC, n), dtype=bool)
        C[0] = pub
        C[1] = pub & (side == 1)
        C[2] = C[1] & (sol >= 0.5)
        C[3] = C[1] & (sol >= 1.0)
        C[4] = pub & (side == -1)
        C[5] = C[4] & (sol >= 0.5)
        C[6] = C[4] & (sol >= 1.0)
        C[7] = pub & (mv >= 1.0)
        C[8] = pub & (mv >= 3.0)
        C[9] = pub & (mv <= -1.0)
        C[10] = pub & (mv <= -2.0)
        C[11] = C[1] & rt
        C[12] = pub & pro
        C[13] = pub & first_seen
        C[14] = pub & burst & (side == 1)
        C[15] = mine

        cases = np.nonzero(mine & (side == 1))[0]
        if not len(cases):
            continue
        wnm = np.array([lab[int(wal[k])] for k in cases])
        grp = np.where(np.isin(wnm, PAY), "pay", "rest")

        near = np.zeros(n, dtype=bool)
        for k in cases:
            lo = np.searchsorted(tm, tm[k] - CTRL_WIN, side="left")
            hi = np.searchsorted(tm, tm[k] + CTRL_WIN, side="right")
            near[lo:hi] = True
        near &= ~mine
        pool = np.nonzero(near)[0]
        if len(pool) < 8:
            continue

        def acc(Hd, i, g):
            i = int(i)
            j = int(np.searchsorted(tm, tm[i] - LOOKBACK, side="left"))
            if j >= i:
                return
            lag = tm[i] - tm[j:i]
            bi = np.searchsorted(EDGES, lag, side="right") - 1
            ok = (bi >= 0) & (bi < nL)
            if not ok.any():
                return
            bi = bi[ok]
            for c in range(nC):
                m = C[c, j:i][ok]
                if m.any():
                    np.add.at(Hd[g][c], bi[m], 1.0)

        for k, g, w in zip(cases, grp, wnm):
            acc(H, k, g)
            ncase[g] += 1
            acc(H, k, w)
            ncase[w] += 1
        # controls, matched in number to the cases of each group on this coin
        for g in GRPS:
            m = int((grp == g).sum()) if g in ("pay", "rest") else int((wnm == g).sum())
            if not m:
                continue
            want = min(len(pool), N_CTRL * m)
            for k in rng.choice(pool, size=want, replace=False):
                acc(Hc, k, g)
                nctrl[g] += 1
        if r % 3000 == 0:
            print("  run %d  cases pay %s rest %s  %ds"
                  % (r, f"{ncase['pay']:,}", f"{ncase['rest']:,}", time.time() - t0), flush=True)

    print("\ncases  pay %s  rest %s   controls  pay %s  rest %s  %ds"
          % (f"{ncase['pay']:,}", f"{ncase['rest']:,}", f"{nctrl['pay']:,}",
             f"{nctrl['rest']:,}", time.time() - t0), flush=True)

    out = {}
    for g in GRPS:
        case = H[g] / max(ncase[g], 1)
        ctrl = Hc[g] / max(nctrl[g], 1)
        with np.errstate(divide="ignore", invalid="ignore"):
            lift = np.where(ctrl > 0, case / ctrl, np.nan)
        out[g] = (case, ctrl, lift)
        D = pd.DataFrame(lift, index=CLASSES, columns=LBL).round(2)
        print("\n=== LIFT over the coin's own print rate, group %s   (1.00 = no relationship)" % g)
        print("    a REACTION is a spike at one lag; a flat row means that class is not the trigger")
        print(D.to_string(), flush=True)
        E = pd.DataFrame(case - ctrl, index=CLASSES, columns=LBL).round(3)
        print("\n--- EXCESS prints per case, group %s" % g)
        print(E.to_string(), flush=True)

    np.savez(data_file("cvx_hot_trig.npz"), classes=np.array(CLASSES), bins=np.array(LBL),
             **{g + "_case": out[g][0] for g in GRPS},
             **{g + "_ctrl": out[g][1] for g in GRPS})
    print("\n=== BURST-START lift by wallet: where the peak sits IS the reaction time")
    bi = CLASSES.index("burst_start")
    rows = []
    for g in PERW:
        lf = out[g][2][bi]
        k = int(np.nanargmax(lf))
        rows.append(dict(wallet=g, cases=ncase[g], peak_bin=LBL[k],
                         peak_lift=round(float(lf[k]), 2),
                         **{LBL[j]: round(float(lf[j]), 2) for j in range(len(LBL))}))
    print(pd.DataFrame(rows).to_string(index=False), flush=True)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
