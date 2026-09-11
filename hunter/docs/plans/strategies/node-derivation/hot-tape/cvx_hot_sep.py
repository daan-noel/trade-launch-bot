"""Hot-tape node, step 15: IS THEIR TAIL SEPARABLE AT THEIR OWN DECISION TIME?

WHY THIS RUN EXISTS - the error it corrects.

Every hot-tape step so far (1.6, 1.7, 1.8, 1.10) treats all ~95,000 of their buys as ONE label
and asks "when do they buy". Their own book says that label is worth nothing: median trade
-2.07 %, net margin +1.10 %, and the best 1 % of their trades produce 180.6 % of their net - the
other 99 % collectively lose. Reproduced perfectly at the PEER seat their decisions read
-0.14 %/trade (evidence 1.4). So imitating their moment was never going to pay at any AUC, which
is exactly what 1.7 measured: AUC 0.72 out of sample, -5 %/trade.

The question that was never asked is the one that decides the node:

    among THEIR OWN buys, are the ones that carry the tail separable at decision time?

If they are, that separator is E or P and the node is open. If they are not - if their winners
and their losers look identical on the same coin at the moment they act - then there is nothing
to copy at any seat, and the node closes on a mechanism instead of on a number.

THREE THINGS THIS ADDS THAT NO EARLIER RUN HAD.

  1  THE OUTCOME SPLIT. Cases and controls are both THEIR buys; the split is by what the entry
     is worth to US afterwards. Within a coin, a winner against a loser, so the coin, the
     narrative, the launch, the hour and the door are constant by construction.

  2  THE THREE TERMS FROM THE AUGUST 64hP STUDY, which the H-series never used and which are
     the only decision-time facts ever measured to separate this node's outcomes:
       ep_idx    how many times we have already round-tripped THIS coin. His book:
                 1 episode/mint -3.61 %, 2 +2.11, 3 +5.00, 5 +8.22, 8 +10.42, 10 +11.30.
       own_dep   price now against OUR OWN previous exit on this coin. 61.3 % of his
                 re-entries sit below his last sell, and the payoff is monotone in depth
                 (below -20 % -> +6.92 %, win 58.3 %).
       stall     seconds since the coin last made a new high. His entries BEFORE the coin's
                 ATH win 66.2 % at +11.31 %; after it, 41.9 % at -6.71 %.
     All three are spellable with no wallet identity: two of them refer to OUR OWN position
     history, the third is public tape state. That matters for more than legality - a level
     that was set minutes ago is a state that has been true for seconds, which is the only
     anchor that can be anything but a FOLLOW seat (7.2).

  3  1.6's dd_peak reads 0.408 (their buys sit deep BELOW the coin's peak) while the 64hP
     study says his profitable entries are in the ASCENDING phase. Both hold only if the
     pooled statistic mixes his winners with his losers. That is a prediction, and this run
     tests it directly.

BASIS. The 95,135 position-tracked episodes of cvx_hot_exit.py (an episode opens when their
position leaves zero and closes when it returns). Entry is their buy, the seat is ours - the
last print landed 115 ms after it. Features are built from prints 0..ki-1 on that coin only,
node-blind, with the coin's own expanding z-score beside every absolute value. Their wallets
are the instrument (7.4 law 20): a split and a shape, never a term.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import os
from collections import deque
import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from kernel import book, fill_idx
from cvx import DAY0, cell, show
from tape import Tape
from cvx_hot_event import (FEATS, LOCAL_Z, ROUTERS, expanding_z, features, node_ids,
                           pct_from_reserve)

B = 0.2
LAG = 0.115
SEED = 20260910

pd.set_option("display.width", 460)
pd.set_option("display.max_rows", 600)
pd.set_option("display.max_columns", 50)

# the two standing exit families, side by side, plus the scalper control
EXITS = {
    "cap60": dict(cap=60.0),
    "tr40_c600": dict(trail=40.0, cap=600.0),
    "cap15": dict(cap=15.0),
}

NEW = ["ep_idx", "own_dep", "own_gap", "stall", "rise10", "rise30"]


def qs(s, P=(10, 25, 50, 75, 90)):
    s = np.asarray(s, dtype=float)
    s = s[np.isfinite(s)]
    if not s.size:
        return "  ".join("%8s" % "-" for _ in P)
    return "  ".join("%8.2f" % x for x in np.percentile(s, P))


def own_terms(t, v, vbef, side, wal, w, ki_list):
    """ep_idx, own_dep, own_gap for one wallet's episodes on one coin, in chain order.

    ep_idx  = closed episodes of THIS wallet on THIS coin strictly before this entry
    own_dep = price at the entry against the price at which the previous episode CLOSED
    own_gap = seconds since that close
    """
    out = {}
    p = np.nonzero(wal == w)[0]
    pos = 0.0
    peak = 0.0
    ki = None
    n_closed = 0
    last_v = np.nan
    last_t = np.nan
    from kernel import K as KK
    for k in p:
        k = int(k)
        if side[k] == 1:
            tk = KK / vbef[k] - KK / v[k]
            if ki is None:
                ki = k
                peak = 0.0
                out[k] = (n_closed, last_v, last_t)
            pos += tk
            peak = max(peak, pos)
        else:
            if ki is None:
                continue
            pos -= KK / v[k] - KK / vbef[k]
            if pos <= 0.02 * peak:
                n_closed += 1
                last_v = float(vbef[k])
                last_t = float(t[k])
                ki = None
                pos = 0.0
    res = {}
    for k in ki_list:
        nc, lv, lt = out.get(int(k), (0, np.nan, np.nan))
        dep = np.nan
        if np.isfinite(lv) and lv > 0:
            dep = float((vbef[int(k)] / lv) ** 2 - 1.0) * 100.0
        res[int(k)] = (nc, dep, float(t[int(k)] - lt) if np.isfinite(lt) else np.nan)
    return res


def phase_terms(t, v):
    """stall (seconds since the coin last made a new high) and rise off the recent low.

    Both read the state STRICTLY BEFORE print i: the running max and the window low are built
    from prints 0..i-1 and compared against v[i-1]. `closed="left"` is what makes the rolling
    window exclude the print being decided on.
    """
    vprev = np.concatenate(([np.nan], v[:-1]))
    rmax = np.maximum.accumulate(v)
    newhigh = np.concatenate(([True], v[1:] > rmax[:-1]))
    t_nh = pd.Series(np.where(newhigh, t, np.nan)).ffill().to_numpy()
    stall = np.concatenate(([0.0], t[1:] - t_nh[:-1]))

    tm = np.maximum.accumulate(t)      # chain order; t_ms itself is not strictly monotonic
    return stall, pct_from_reserve(vprev, _rollmin(tm, v, 10.0)), \
        pct_from_reserve(vprev, _rollmin(tm, v, 30.0))


def _rollmin(tm, v, win):
    """min of v over the prints in [tm[i] - win, tm[i]), i.e. STRICTLY before print i."""
    n = len(tm)
    j = np.searchsorted(tm, tm - win, side="left")
    out = np.full(n, np.nan)
    dq = deque()                       # indices of a non-decreasing v, all < i
    nxt = 0
    for i in range(n):
        while nxt < i:
            while dq and v[dq[-1]] >= v[nxt]:
                dq.pop()
            dq.append(nxt)
            nxt += 1
        left = j[i]
        while dq and dq[0] < left:
            dq.popleft()
        if dq:
            out[i] = v[dq[0]]
    return out


def strat_rank(C, Kc, cols, key="run"):
    """A case's mean percentile among the CONTROLS of its own stratum. 0.50 = no information."""
    num = np.zeros(len(cols))
    den = np.zeros(len(cols))
    cg = {r: g[cols].to_numpy() for r, g in C.groupby(key, sort=False)}
    kg = {r: g[cols].to_numpy() for r, g in Kc.groupby(key, sort=False)}
    for r, cm in cg.items():
        km = kg.get(r)
        if km is None:
            continue
        for j in range(len(cols)):
            kv = km[:, j]
            kv = kv[np.isfinite(kv)]
            if len(kv) < 2:
                continue
            cv = cm[:, j]
            cv = cv[np.isfinite(cv)]
            if not len(cv):
                continue
            kv = np.sort(kv)
            lo = np.searchsorted(kv, cv, side="left")
            hi = np.searchsorted(kv, cv, side="right")
            num[j] += float(((lo + hi) / 2.0 / len(kv)).sum())
            den[j] += len(cv)
    out = []
    for j, col in enumerate(cols):
        if den[j]:
            mp = num[j] / den[j]
            out.append(dict(feature=col, n=int(den[j]), pctile=round(mp, 4),
                            dev=round(abs(mp - 0.5), 4),
                            dir="HIGH" if mp > 0.5 else "low"))
    return pd.DataFrame(out).sort_values("dev", ascending=False)


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    inv = {v: k for k, v in lab.items()}
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    is_node = np.isin(T.wallet, NODE)

    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router_all = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()

    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()

    E = pd.read_parquet(data_file("cvx_hot_exit_eps.parquet"))
    print("tape %s prints  episodes %s  coins %s  %.2f days  %ds"
          % (f"{T.n:,}", f"{len(E):,}", f"{E.run.nunique():,}", days, time.time() - t0),
          flush=True)

    cols = FEATS + [c + "_z" for c in LOCAL_Z]
    rows = []
    byrun = {}
    for i, r in enumerate(E.run.values):
        byrun.setdefault(int(r), []).append(i)

    lim = int(os.environ.get("HOTSEP_LIMIT", "0"))
    if lim:
        byrun = {r: idxs for r, idxs in list(byrun.items())[:lim]}
        print("SMOKE TEST: %d runs only" % lim, flush=True)

    kw = E.w.values
    kki = E.ki.values.astype(int)
    done = 0
    for r, idxs in byrun.items():
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 10 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        slot = T.slot[a:b]; wal = T.wallet[a:b]
        mine = is_node[a:b]
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)

        f = features(t, v, sol, side, t - c_s[r], mine, router_all[a:b])
        for c in LOCAL_Z:
            f[c + "_z"] = expanding_z(f[c])
        stall, rise10, rise30 = phase_terms(t, v)
        M = np.column_stack([f[c] for c in cols]).astype(np.float32)

        # own-position terms, per wallet
        own = {}
        for wname in set(kw[i] for i in idxs):
            wid = inv[wname]
            kl = [kki[i] for i in idxs if kw[i] == wname]
            own.update({(wname, k): val for k, val in
                        own_terms(t, v, vbef, side, wal, wid, kl).items()})

        for i in idxs:
            k = int(kki[i])
            if k >= n:
                continue
            nc, dep, gp = own.get((kw[i], k), (0, np.nan, np.nan))
            y = {}
            for name, spec in EXITS.items():
                nn, ei, xi, _ = book(t, v, slot, k, dict(spec), lag=LAG, B=B)
                y["y_" + name] = nn
            rows.append((i, nc, dep, gp, float(stall[k]), float(rise10[k]), float(rise30[k]))
                        + tuple(M[k]) + tuple(y[c] for c in ("y_cap60", "y_tr40_c600",
                                                             "y_cap15")))
        done += 1
        if done % 3000 == 0:
            print("  runs %d  rows %s  %ds" % (done, f"{len(rows):,}", time.time() - t0),
                  flush=True)

    F = pd.DataFrame(rows, columns=["ei"] + NEW + cols + ["y_cap60", "y_tr40", "y_cap15"])
    F = F.set_index("ei")
    D = E.join(F, how="inner")
    D["y"] = D.y_tr40
    D.to_parquet(data_file("cvx_hot_sep.parquet"), index=False)
    print("\nfeature table %s rows  %ds" % (f"{len(D):,}", time.time() - t0), flush=True)

    P = (10, 25, 50, 75, 90)
    hdr = "                " + "  ".join("%8s" % ("p%d" % p) for p in P)
    print("\n=== 0. THE THREE NEW TERMS, distribution")
    print(hdr)
    print("  ep_idx        %s" % qs(D.ep_idx, P))
    print("  own_dep %%     %s   <- price at re-entry vs their own previous exit" % qs(D.own_dep, P))
    print("  own_gap s     %s" % qs(D.own_gap, P))
    print("  stall s       %s   <- seconds since the coin last made a new high" % qs(D.stall, P))
    print("  rise10 %%      %s" % qs(D.rise10, P))
    re_share = float((D.ep_idx > 0).mean()) * 100
    below = float((D.own_dep[D.ep_idx > 0] < 0).mean()) * 100
    print("\n  re-entries %.1f %% of episodes; of those %.1f %% sit BELOW their own previous exit"
          % (re_share, below), flush=True)

    # ---- 1. the money, by each new term ---------------------------------------------------
    print("\n=== 1. WHAT EACH NEW TERM IS WORTH AT OUR SEAT  (their entry, our fill, 0.2 SOL)")
    for xname, ylab in (("y_tr40", "trail40 c600"), ("y_cap60", "clock 60")):
        rows2 = []
        d = D.copy()
        d["y"] = d[xname]
        rows2.append(dict(term="all", bin="-", **cell(d, days=days)))
        for lo, hi in ((0, 0), (1, 1), (2, 3), (4, 7), (8, 999)):
            s = d[(d.ep_idx >= lo) & (d.ep_idx <= hi)]
            rows2.append(dict(term="ep_idx", bin="%d-%d" % (lo, hi), **cell(s, days=days)))
        dd = d[d.ep_idx > 0]
        for lo, hi in ((-1e9, -30), (-30, -15), (-15, -5), (-5, 5), (5, 25), (25, 1e9)):
            s = dd[(dd.own_dep > lo) & (dd.own_dep <= hi)]
            rows2.append(dict(term="own_dep", bin="%g..%g" % (lo, hi), **cell(s, days=days)))
        for lo, hi in ((0, 5), (5, 20), (20, 60), (60, 180), (180, 1e9)):
            s = d[(d.stall > lo) & (d.stall <= hi)]
            rows2.append(dict(term="stall", bin="%g-%g" % (lo, hi), **cell(s, days=days)))
        for lo, hi in ((-1e9, -30), (-30, -10), (-10, 0), (0, 10), (10, 1e9)):
            s = d[(d.dd_peak > lo) & (d.dd_peak <= hi)]
            rows2.append(dict(term="dd_peak", bin="%g..%g" % (lo, hi), **cell(s, days=days)))
        show(rows2, "1.%s  exit = %s" % (xname, ylab))

    # ---- 2. within-coin: their winners against their losers -------------------------------
    print("\n=== 2. WITHIN THE COIN: their WINNERS against their LOSERS at decision time")
    lab_cols = [("mfe60>=20%", D.mfe60 >= 0.20), ("mfe1800>=100%", D.mfe1800 >= 1.0),
                ("y_tr40>0", D.y_tr40 > 0)]
    fcols = NEW + cols
    for name, mask in lab_cols:
        C = D[mask]
        Kc = D[~mask]
        keep = set(C.run) & set(Kc.run)
        C = C[C.run.isin(keep)]
        Kc = Kc[Kc.run.isin(keep)]
        rk = strat_rank(C[["run"] + fcols], Kc[["run"] + fcols], fcols)
        print("\n  label %s   winners %s  losers %s  coins %s"
              % (name, f"{len(C):,}", f"{len(Kc):,}", f"{len(keep):,}"))
        print(rk.head(14).to_string(index=False), flush=True)

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
