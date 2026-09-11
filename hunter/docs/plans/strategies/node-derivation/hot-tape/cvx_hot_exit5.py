"""Hot-tape node, step 29: THEIR EXIT TRIGGER - what makes the members that pay sell, when they do.

The event is theirs (evidence 1.12) and the exit is still a guessed clock (cap15). Workflow
section 5: the exit is derived from the sentence, never swept. The members that pay are scalpers
who "eat convexity and exit smartly", so the exit is read off their own closing sells with the
same instrument that found their entry trigger - excess intensity - and one more, the hazard.

THE CONTROL IS THE HOLD. An exit is a choice made while holding. So:

  cases     each closing sell of the member (the sell that takes the position to <= 2 % of its
            peak size)
  controls  random PUBLIC prints on the same coin inside the same holding episode, strictly
            between its first buy and that sell

  1. print trigger   excess intensity over (print class, lag) in the 5 s before the sell
  2. state trigger   within-episode rank of the sell against its controls: profit since entry,
                     peak since entry, drawdown from that peak, time held, the tape's last seconds
  3. hazard          over EVERY public print of every holding episode: the chance the member's
                     next print is the closing sell, by profit since entry x time held, and by
                     drawdown from the in-hold peak

Profit is read off the curve: (v / v_entry)^2 - 1, v_entry the reserve after the member's first
buy of the episode. Every classified print is node-blind (7.4 law 20).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from kernel import K
from cvx import DAY0
from tape import Tape
from cvx_hot_event import ROUTERS, node_ids
from cvx_hot_trig import CLASSES, EDGES, LBL, LOOKBACK

N_CTRL = 4
SEED = 20260912
PERW = ("8fStGV", "AbQcLH", "49uohd", "64hP97", "omegoM", "sssssw")
PAY = ("8fStGV", "AbQcLH", "49uohd")
STATE = ["pnl", "peak", "dd", "held", "mv2", "mv5", "nb5", "bsol2", "ssol2", "n5", "stall",
         "since_up"]
PNL_E = np.array([-1e9, -20, -10, -5, 0, 5, 10, 20, 40, 1e9])
PNL_L = ["<-20", "-20..-10", "-10..-5", "-5..0", "0..5", "5..10", "10..20", "20..40", ">40"]
HLD_E = np.array([0, 2, 5, 10, 20, 40, 80, 1e9])
HLD_L = ["0-2s", "2-5s", "5-10s", "10-20s", "20-40s", "40-80s", ">80s"]
DD_E = np.array([-1e9, -30, -20, -10, -5, -2, -0.001, 1e9])
DD_L = ["<-30", "-30..-20", "-20..-10", "-10..-5", "-5..-2", "-2..0", "at peak"]

pd.set_option("display.width", 500)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 40)


def pctv(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return ((a / b) ** 2 - 1.0) * 100.0


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
    print("tape %s prints  node coins %s  %ds" % (f"{T.n:,}", f"{len(runs):,}", time.time() - t0),
          flush=True)

    H = {g: np.zeros((nC, nL)) for g in PERW}
    Hc = {g: np.zeros((nC, nL)) for g in PERW}
    ncase = {g: 0 for g in PERW}
    nctrl = {g: 0 for g in PERW}
    HZ = {g: np.zeros((2, len(PNL_L), len(HLD_L))) for g in PERW}
    HD = {g: np.zeros((2, len(DD_L))) for g in PERW}
    srows = []

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
        pub = ~mine
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            tokd = np.where(side == 1, K / vbef - K / v, K / v - K / vbef)
        tokd = np.nan_to_num(tokd, nan=0.0, posinf=0.0, neginf=0.0)
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

        j2 = np.searchsorted(tm, tm - 2.0, side="left")
        j5 = np.searchsorted(tm, tm - 5.0, side="left")
        cbs = np.concatenate(([0.0], np.cumsum(np.where(pub & (side == 1), sol, 0.0))))
        css = np.concatenate(([0.0], np.cumsum(np.where(pub & (side == -1), sol, 0.0))))
        rmax = np.maximum.accumulate(v)
        newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
        t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
        stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))
        up = mv > 0
        t_up = pd.Series(np.where(up & pub, tm, np.nan)).ffill().to_numpy()
        since_up = np.concatenate(([np.nan], tm[1:] - t_up[:-1]))

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

        def state(i, e0, v0):
            # facts at print i, from prints strictly before it, for an episode that opened at e0
            vp = v[i - 1]
            pk = float(v[e0:i].max()) if i > e0 else v0
            return (pctv(vp, v0), pctv(pk, v0), pctv(vp, pk), tm[i] - tm[e0],
                    pctv(vp, v[max(int(j2[i]) - 1, 0)]), pctv(vp, v[max(int(j5[i]) - 1, 0)]),
                    float(len(np.unique(bu[j5[i]:i]))), cbs[i] - cbs[j2[i]],
                    css[i] - css[j2[i]], float(i - j5[i]), stall[i], since_up[i])

        for w in np.unique(wal[mine]):
            g = lab.get(int(w))
            if g not in PERW:
                continue
            idx = np.nonzero(wal == w)[0]
            pos = 0.0; peakpos = 0.0; e0 = -1; v0 = np.nan
            for s in idx:
                s = int(s)
                if side[s] == 1:
                    if pos <= 0.0:
                        e0 = s; v0 = float(v[s]); peakpos = 0.0
                    pos += tokd[s]; peakpos = max(peakpos, pos)
                    continue
                if pos <= 0.0 or e0 < 0:
                    continue
                pos = max(pos - tokd[s], 0.0)
                if pos > 0.02 * peakpos:
                    continue
                # a closing sell: s. the hold is the public prints strictly inside (e0, s)
                hold = np.nonzero(pub[e0 + 1:s])[0] + e0 + 1
                pos = 0.0
                if s <= e0 + 1:
                    continue
                acc(H, s, g); ncase[g] += 1
                st = state(s, e0, v0)
                srows.append((g, r, int(T.day[a + s]), 1) + tuple(float(x) for x in st))
                pi = np.searchsorted(PNL_E, st[0], side="right") - 1
                hi_ = np.searchsorted(HLD_E, st[3], side="right") - 1
                di = np.searchsorted(DD_E, st[2], side="right") - 1
                if 0 <= pi < len(PNL_L) and 0 <= hi_ < len(HLD_L):
                    HZ[g][0, pi, hi_] += 1
                if 0 <= di < len(DD_L):
                    HD[g][0, di] += 1
                if not len(hold):
                    continue
                # hazard denominators: every public print of the hold
                vp = v[hold - 1]
                pk = np.maximum.accumulate(v[e0:s])[hold - 1 - e0]
                pnl = pctv(vp, v0); dd = pctv(vp, pk); hl = tm[hold] - tm[e0]
                pi = np.searchsorted(PNL_E, pnl, side="right") - 1
                hi_ = np.searchsorted(HLD_E, hl, side="right") - 1
                di = np.searchsorted(DD_E, dd, side="right") - 1
                ok = (pi >= 0) & (pi < len(PNL_L)) & (hi_ >= 0) & (hi_ < len(HLD_L))
                np.add.at(HZ[g][1], (pi[ok], hi_[ok]), 1.0)
                okd = (di >= 0) & (di < len(DD_L))
                np.add.at(HD[g][1], di[okd], 1.0)
                for c in rng.choice(hold, size=min(len(hold), N_CTRL), replace=False):
                    acc(Hc, c, g); nctrl[g] += 1
                    srows.append((g, r, int(T.day[a + int(c)]), 0)
                                 + tuple(float(x) for x in state(int(c), e0, v0)))
        if r % 5000 == 0:
            print("  run %d  closing sells %s  %ds"
                  % (r, f"{sum(ncase.values()):,}", time.time() - t0), flush=True)

    print("\nclosing sells %s   controls %s  %ds"
          % ({g: ncase[g] for g in PERW}, {g: nctrl[g] for g in PERW}, time.time() - t0),
          flush=True)
    S = pd.DataFrame(srows, columns=["w", "run", "day", "case"] + STATE)
    S.to_parquet(data_file("cvx_hot_exit5.parquet"), index=False)

    for g in PERW:
        case = H[g] / max(ncase[g], 1)
        ctrl = Hc[g] / max(nctrl[g], 1)
        with np.errstate(divide="ignore", invalid="ignore"):
            lift = np.where(ctrl > 0, case / ctrl, np.nan)
        print("\n=== 1. PRINT TRIGGER of the closing sell, %s  (lift over in-hold public prints;"
              " a spike at one lag is a reaction)" % g)
        print(pd.DataFrame(lift, index=CLASSES, columns=LBL).round(2).to_string(), flush=True)

    print("\n=== 2. STATE at the closing sell against in-hold controls (medians)")
    med = S.groupby(["w", "case"])[STATE].median().round(3)
    print(med.to_string(), flush=True)
    print("\n    AUC: closing sell against its own hold's controls (0.5 = nothing)")
    rows = []
    for g in PERW:
        s = S[S.w == g]
        if not len(s):
            continue
        d = dict(w=g)
        for f in STATE:
            x1 = s.loc[s.case == 1, f].to_numpy(); x0 = s.loc[s.case == 0, f].to_numpy()
            x1 = x1[np.isfinite(x1)]; x0 = np.sort(x0[np.isfinite(x0)])
            if len(x1) < 30 or len(x0) < 30:
                d[f] = np.nan; continue
            lo = np.searchsorted(x0, x1, side="left"); hi = np.searchsorted(x0, x1, side="right")
            d[f] = round(float(((lo + hi) / 2.0 / len(x0)).mean()), 3)
        rows.append(d)
    print(pd.DataFrame(rows).to_string(index=False), flush=True)

    for g in PERW:
        num, den = HZ[g]
        with np.errstate(divide="ignore", invalid="ignore"):
            hz = np.where(den >= 50, 100.0 * num / (num + den), np.nan)
        print("\n=== 3. HAZARD, %s: %% of in-hold public prints at which the member closes next,"
              " by profit since entry (rows) x time held (cols)" % g)
        print(pd.DataFrame(hz, index=PNL_L, columns=HLD_L).round(2).to_string(), flush=True)
        print("    closing sells in each cell")
        print(pd.DataFrame(num, index=PNL_L, columns=HLD_L).astype(int).to_string(), flush=True)
        n1, d1 = HD[g]
        with np.errstate(divide="ignore", invalid="ignore"):
            hd = np.where(d1 >= 50, 100.0 * n1 / (n1 + d1), np.nan)
        print("    by drawdown from the in-hold peak:",
              dict(zip(DD_L, np.round(hd, 2))), " sells", dict(zip(DD_L, n1.astype(int))))
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
