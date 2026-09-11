"""Hot-tape node, step 9: fire the within-mint model on the whole tape and book it.

The model is config 4 of `cvx_hot_model2.py`: coin-relative features only, the gap family
removed, fitted on half the coins and reading **AUC 0.7212** on the other half, with the top
decile inside a coin holding 44.4 % of their buys against a 20 % base (lift 2.22).

Coin-relative means every feature is that coin measured against its own prior history, so a live
rule can compute the score from the coin's own tape with no stratum and no lookahead. That is
what makes this scoreable outside the case-control frame at all.

The fit is repeated here rather than loaded, so the design matrix that scores the tape is built
by the same code that built the one it was fitted on.

Fired on the RISING EDGE of the score crossing a threshold, one position per coin, 0.2 SOL,
lag_115 on both legs. Thresholds are score percentiles taken over the whole tape, so the cells
are named by ticket volume rather than by a magic number.

REACTION COST is reported beside every cell. Evidence 1.6: an event whose price has already moved
by the time we can fill is unreachable whatever its lift, and this model was fitted to imitate a
node whose own PEER-seat ceiling is only -0.14 % (1.4). Imitation is not the goal and cannot be
the result - the question is whether the score is a usable EVENT slot to hang a permission on.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import pyarrow.parquet as pq

from cvx import B, DAY0, cell, show
from kernel import book
from tape import Tape
from cvx_hot_event import expanding_z, features, node_ids
from cvx_hot_model import PURE, logistic_fit
from cvx_hot_model2 import GAP_FAMILY

ROUTERS = ("Axiom", "Terminal", "GMGN", "Photon", "Bloom", "Trojan", "BullX", "Padre", "DFlow")
SEED = 20260910
# The diagnosis that produced this grid: the model's fires pay +2.37 %/trade 7/8 days when one
# of the six buys INSIDE our hold (30.1 % of fires) and -8 to -13 % when nobody arrives. So the
# money is incoming demand, and the question the exit has to answer is "did anybody come?".
# `abort=(T, g)` closes at T seconds if price has not risen g percent - it is conditioned on the
# ABSENCE of a move, so it fills on a quiet tape rather than into a cascade, and it leaves a
# position that IS moving completely alone. Silence satisfies it, which is exactly right here.
EXITS = {
    "cap20": dict(cap=20.0), "cap60": dict(cap=60.0),
    "tr25_c600": dict(trail=25.0, cap=600.0),
    "shipped": dict(arm=21.0, trail=36.0, stop=43.75, cap=1200.0),
    "ab5_tr25": dict(abort=(5.0, 0.0), trail=25.0, cap=600.0),
    "ab10_tr25": dict(abort=(10.0, 0.0), trail=25.0, cap=600.0),
    "ab15_tr25": dict(abort=(15.0, 0.0), trail=25.0, cap=600.0),
    "ab10g2_tr25": dict(abort=(10.0, 2.0), trail=25.0, cap=600.0),
    "ab20g5_tr25": dict(abort=(20.0, 5.0), trail=25.0, cap=600.0),
    "ab10_ship": dict(abort=(10.0, 0.0), arm=21.0, trail=36.0, stop=43.75, cap=1200.0),
    "ab20g5_ship": dict(abort=(20.0, 5.0), arm=21.0, trail=36.0, stop=43.75, cap=1200.0),
}
CUTS = (99.9, 99.5, 99.0, 98.0, 95.0)

pd.set_option("display.width", 430)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 40)


def design(D, cols, flag_cols):
    X = D[cols].to_numpy(dtype=np.float64) if isinstance(D, pd.DataFrame) else D
    miss = ~np.isfinite(X)
    Xi = np.clip(np.where(miss, 0.0, X), -8.0, 8.0)
    fi = [cols.index(c) for c in flag_cols]
    if fi:
        Xi = np.hstack([Xi, miss[:, fi].astype(np.float64)])
    return Xi


def main() -> None:
    t0 = time.time()
    # ---- refit config 4 -----------------------------------------------------------------
    C = pd.read_parquet(data_file("cvx_hot_event_cases.parquet")); C["y"] = 1
    K = pd.read_parquet(data_file("cvx_hot_event_ctrl.parquet")); K["y"] = 0
    D = pd.concat([C, K], ignore_index=True)
    cols = [c for c in PURE if c in D.columns and c not in GAP_FAMILY]
    Xraw = D[cols].to_numpy(dtype=np.float64)
    missrate = (~np.isfinite(Xraw)).mean(axis=0)
    flag_cols = [cols[j] for j in range(len(cols))
                 if missrate[j] > 0.02 and cols[j] + "_isNaN" not in GAP_FAMILY]
    names = cols + [c + "_isNaN" for c in flag_cols]
    Xi = design(D, cols, flag_cols)
    runs = np.sort(D.run.unique())
    rng = np.random.default_rng(SEED)
    fit_runs = set(rng.choice(runs, size=len(runs) // 2, replace=False).tolist())
    f = D.run.isin(fit_runs).to_numpy()
    mu = Xi[f].mean(axis=0); sd = Xi[f].std(axis=0); sd[sd < 1e-9] = 1.0
    w = logistic_fit((Xi[f] - mu) / sd, D.y.to_numpy(dtype=np.float64)[f])
    print("model refitted: %d features, %s fit rows  %ds"
          % (len(names), f"{int(f.sum()):,}", time.time() - t0), flush=True)

    # ---- score the whole tape -----------------------------------------------------------
    lab = node_ids(); NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0
    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    router_all = ix.column("tool").to_pandas().isin(ROUTERS).to_numpy()
    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()
    sw = pd.read_parquet(data_file("cvx_swdoor.parquet")).drop_duplicates("mint").set_index("mint")
    cbuild = sw.reindex(T.mints).c_build.fillna("?").to_numpy()
    is_node = np.isin(T.wallet, NODE)
    zcols = [c for c in cols if c.endswith("_z")]
    basec = sorted({c[:-2] for c in zcols})

    scored = {}
    samp = []
    print("scoring the tape ...", flush=True)
    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 30 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]
        age = t - c_s[r]
        if age[-1] < 60.0:
            continue
        v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        mine = is_node[a:b]
        fe = features(t, v, sol, side, age, mine, router_all[a:b])
        for c in basec:
            fe[c + "_z"] = expanding_z(fe[c])
        X = np.column_stack([fe[c] for c in cols])
        Z = (design(X, cols, flag_cols) - mu) / sd
        sc = np.hstack([np.ones((len(Z), 1)), Z]) @ w
        scored[r] = sc.astype(np.float32)
        if r % 7 == 0:
            samp.append(sc[np.isfinite(sc)][::13])
        if r % 20000 == 0:
            print("  run %d  %ds" % (r, time.time() - t0), flush=True)

    samp = np.concatenate(samp)
    thr = {c: float(np.percentile(samp, c)) for c in CUTS}
    print("score percentiles over the tape: %s  %ds"
          % ({k: round(x, 2) for k, x in thr.items()}, time.time() - t0), flush=True)

    # ---- fire on the rising edge and book ------------------------------------------------
    rows = {(c, x): [] for c in CUTS for x in EXITS}
    for r, sc in scored.items():
        a, b = T.start[r], T.end[r]
        n = b - a
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]; sl = T.slot[a:b]
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        day = T.day[a:b]; age = t - c_s[r]
        for c in CUTS:
            m = np.nan_to_num(sc >= thr[c], nan=False).astype(bool)
            edges = np.nonzero(m & ~np.concatenate(([False], m[:-1])))[0]
            if not len(edges):
                continue
            for xname, xspec in EXITS.items():
                last = -1
                acc = rows[(c, xname)]
                for k in edges:
                    k = int(k)
                    if k <= last or k < 2 or k >= n - 1:
                        continue
                    y, ei, xi, reason = book(t, v, sl, k, xspec, lag=0.115, B=B)
                    acc.append((y, r, int(day[k]), reason, str(cbuild[r]),
                                float((v[ei] / vbef[k]) ** 2 - 1.0),
                                float(t[xi] - t[ei]), float(v[ei]), float(age[k])))
                    last = xi

    outc = ["y", "run", "day", "reason", "cbuild", "react", "hold", "v", "age"]
    out = []
    for c in CUTS:
        for xname in EXITS:
            d = pd.DataFrame(rows[(c, xname)], columns=outc)
            if not len(d):
                out.append(dict(cut=c, exit=xname, n=0)); continue
            cc = cell(d, days=days)
            per_day = d.groupby("day").run.nunique().sort_index()
            byb = d.groupby("cbuild").y.sum().sort_values(ascending=False)
            tot = float(d.y.sum())
            top1 = d.y.nlargest(max(1, int(round(0.01 * len(d))))).sum()
            out.append(dict(
                cut="top %.1f%%" % (100 - c), exit=xname, n=cc["n"], mints=cc["mints"],
                sol=cc["sol"], pct=cc["pct"], pos=cc["pos"], worst=cc["worst"], win=cc["win"],
                floor="%d/%d" % (int((per_day >= 50).sum()), len(per_day)),
                clients=int(d.cbuild.nunique()),
                top_client=round(100 * float(byb.iloc[0]) / tot, 1) if tot > 0 else None,
                loo=round(tot - float(byb.iloc[0]), 2),
                top1=round(100 * float(top1) / tot, 1) if tot > 0 else None,
                REACT=round(100 * float(d.react.mean()), 2),
                hold=round(float(d.hold.median()), 1)))
    show(out, "the within-mint model fired on the whole tape at lag_115")
    pd.DataFrame(rows[(99.0, "cap60")], columns=outc).to_parquet(
        data_file("cvx_hot_score_fires.parquet"), index=False)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
