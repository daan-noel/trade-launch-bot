"""Hot-tape node, step 8: is the within-mint signal real, or is it the control sampling?

Two control designs, and NEITHER is neutral:

  PRINT controls (steps 2-6)  a control is a public print, so its `gap` is the FULL inter-arrival
                              interval, while a node buy sits somewhere INSIDE one - about half
                              of one on average, by arithmetic alone.
  TIME controls (step 7)      a control is a uniformly random second, and a random second on a
                              coin is usually QUIET, because gaps dominate the time axis. Cases
                              read gap 0.156 s against controls 4.163 s, which mostly says "they
                              trade when there is trading".

So the question the model has to survive is not which control is right, but whether the signal
lives anywhere except in the timing artifact. Three fits, same design otherwise:

  1  print controls, every feature                  - the number from step 6, for reference
  2  print controls, the WHOLE gap family removed   - the decisive one
  3  time controls, the whole gap family removed    - the same question from the other bias

If (2) collapses towards 0.50 the signal was the sampling. If it holds near (1) the model is
reading the tape, and the two opposite biases bracketing it make that hard to explain away.

Split is by COIN, never by day: a coin never appears in both halves. Their wallets are the
instrument (7.4 law 20).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

from cvx_hot_model import PURE, PLUS, logistic_fit, strat_auc

SEED = 20260910
GAP_FAMILY = ("gap", "gap_z", "bshare2", "bshare2_isNaN", "n2", "n2_z")

pd.set_option("display.width", 420)
pd.set_option("display.max_rows", 400)


def run(tag, cases_f, ctrl_f, feats, drop):
    C = pd.read_parquet(data_file(cases_f)); C["y"] = 1
    K = pd.read_parquet(data_file(ctrl_f)); K["y"] = 0
    D = pd.concat([C, K], ignore_index=True)
    runs = np.sort(D.run.unique())
    rng = np.random.default_rng(SEED)
    fit_runs = set(rng.choice(runs, size=len(runs) // 2, replace=False).tolist())
    D["fit"] = D.run.isin(fit_runs)

    cols = [c for c in feats if c in D.columns and c not in drop]
    X = D[cols].to_numpy(dtype=np.float64)
    miss = ~np.isfinite(X)
    Xi = np.clip(np.where(miss, 0.0, X), -8.0, 8.0)
    flags = [j for j in range(len(cols)) if miss[:, j].mean() > 0.02
             and cols[j] + "_isNaN" not in drop]
    names = list(cols)
    if flags:
        Xi = np.hstack([Xi, miss[:, flags].astype(np.float64)])
        names += [cols[j] + "_isNaN" for j in flags]
    keep = [j for j, nm in enumerate(names) if nm not in drop]
    Xi = Xi[:, keep]; names = [names[j] for j in keep]

    y = D.y.to_numpy(dtype=np.float64); f = D.fit.to_numpy()
    mu = Xi[f].mean(axis=0); sd = Xi[f].std(axis=0); sd[sd < 1e-9] = 1.0
    Z = (Xi - mu) / sd
    w = logistic_fit(Z[f], y[f])
    D["score"] = np.hstack([np.ones((len(Z), 1)), Z]) @ w
    a_fit, a_hold = strat_auc(D[f]), strat_auc(D[~f])

    print("\n=== %s   (%d features, %s cases / %s controls, %s coins)"
          % (tag, len(names), f"{len(C):,}", f"{len(K):,}", f"{len(runs):,}"), flush=True)
    print("  within-coin AUC   fit %.4f   HOLD %.4f" % (a_fit, a_hold), flush=True)
    coef = pd.DataFrame(dict(feature=names, weight=w[1:]))
    coef["abs"] = coef.weight.abs()
    print(coef.sort_values("abs", ascending=False).head(10)[["feature", "weight"]]
          .to_string(index=False), flush=True)
    h = D[~f].copy()
    h["q"] = h.groupby("run").score.rank(pct=True)
    top = h[h.q > 0.9]
    base = float(h.y.mean())
    print("  top decile inside each coin: %.2f %% of those moments are theirs (base %.2f %%),"
          " lift %.2f" % (100 * float(top.y.mean()), 100 * base,
                          float(top.y.mean()) / base), flush=True)
    return a_hold


def main() -> None:
    t0 = time.time()
    a1 = run("1  PRINT controls, every feature",
             "cvx_hot_event_cases.parquet", "cvx_hot_event_ctrl.parquet", PLUS, ())
    a2 = run("2  PRINT controls, GAP FAMILY REMOVED",
             "cvx_hot_event_cases.parquet", "cvx_hot_event_ctrl.parquet", PLUS, GAP_FAMILY)
    a3 = run("3  TIME controls, GAP FAMILY REMOVED",
             "cvx_hot_event3_cases.parquet", "cvx_hot_event3_ctrl.parquet", PLUS, GAP_FAMILY)
    a4 = run("4  PRINT controls, GAP REMOVED, COIN-RELATIVE ONLY",
             "cvx_hot_event_cases.parquet", "cvx_hot_event_ctrl.parquet",
             PURE, GAP_FAMILY)
    a5 = run("5  TIME controls, GAP REMOVED, COIN-RELATIVE ONLY",
             "cvx_hot_event3_cases.parquet", "cvx_hot_event3_ctrl.parquet",
             PURE, GAP_FAMILY)
    print("\n=== VERDICT")
    print("  every feature, print controls   AUC %.4f" % a1)
    print("  no gap family, print controls   AUC %.4f   (%.4f lost to the gap family)"
          % (a2, a1 - a2))
    print("  no gap family, TIME controls    AUC %.4f" % a3)
    print("  no gap, coin-relative only, print   AUC %.4f" % a4)
    print("  no gap, coin-relative only, TIME    AUC %.4f" % a5)
    print("  Config 4 is what a live rule can compute: every feature is the")
    print("  coin against its own history, so no stratum is needed to score.")
    print("  0.50 would mean the moment is not learnable at all.")
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
