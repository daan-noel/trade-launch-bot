"""Hot-tape node, step 6: a WITHIN-MINT model on the whole feature vector.

Steps 2-5 ranked 50 features one at a time and then cut them at hard thresholds. That threw the
evidence away. `wake_z` sits at 0.660 across ALL 93,137 of their buys, not across a rare tail;
demanding `wake_z >= 2` kept the 2 % of moments where the price had already jumped (+5.98 % to
react) and discarded the other 98 %. The information is in the whole distribution.

So: fit the whole vector at once, on a design where a model CANNOT cheat.

  STRATUM   the mint. Cases are their buys, controls are non-buy prints on the SAME coin within
            +/- 60 s. Because every comparison lives inside one coin, a model fitted here cannot
            learn "which coin is good" - the door, the narrative, the creator and every off-chain
            factor are constant inside a stratum. It can only learn "which moment".
  FEATURES  only COIN-RELATIVE ones - the mint-local z-scores and the pure ratios. An absolute
            level (age, reserve) is a coin-level fact wearing a moment's clothes, so PURE
            excludes them and PLUS includes them, and the two are read side by side.
  SPLIT     by coin, not by day. Half the coins fit, half are held. A coin never appears in both.
  SCORE     within-stratum AUC on the held half: the chance a random buy of theirs outranks a
            random non-buy moment on the same coin. 0.50 is nothing.

Nothing here is a sentence yet. This step only answers whether their moment is learnable at all.
Their wallets are the instrument (7.4 law 20).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd

SEED = 20260910
L2 = 1e-3

pd.set_option("display.width", 420)
pd.set_option("display.max_rows", 400)

PURE = ["wake_z", "quiet60_z", "buy2_z", "sell2_z", "buy5_z", "sell5_z", "net5_z", "gap_z",
        "n2_z", "n5_z", "n20_z", "n60_z", "vol20_z", "acc_z", "dd_peak_z", "sol60_z",
        "mv1_z", "mv2_z", "mv5_z", "mv20_z", "mv60_z",
        "bshare2", "bshare5", "bshare20", "rt_share20", "acc", "wake"]
PLUS = PURE + ["age", "v", "dd_peak", "n300"]


def logistic_fit(X, y, l2=L2, iters=25):
    """L2-regularised logistic regression by IRLS. Small feature count, so this is exact."""
    n, p = X.shape
    Xb = np.hstack([np.ones((n, 1)), X])
    w = np.zeros(p + 1)
    for _ in range(iters):
        z = Xb @ w
        mu = 1.0 / (1.0 + np.exp(-np.clip(z, -30, 30)))
        s = np.maximum(mu * (1 - mu), 1e-6)
        zz = z + (y - mu) / s
        WX = Xb * s[:, None]
        A = Xb.T @ WX + l2 * n * np.eye(p + 1)
        A[0, 0] -= l2 * n
        b = WX.T @ zz
        wn = np.linalg.solve(A, b)
        if np.max(np.abs(wn - w)) < 1e-7:
            w = wn
            break
        w = wn
    return w


def strat_auc(df):
    """Chance a random case outranks a random control ON THE SAME COIN. 0.5 is nothing."""
    num = 0.0
    den = 0.0
    for r, g in df.groupby("run", sort=False):
        c = g.score.values[g.y.values == 1]
        k = g.score.values[g.y.values == 0]
        if not len(c) or not len(k):
            continue
        ks = np.sort(k)
        lo = np.searchsorted(ks, c, side="left")
        hi = np.searchsorted(ks, c, side="right")
        num += float(((lo + hi) / 2.0).sum())
        den += float(len(c) * len(k))
    return num / den if den else np.nan


def main() -> None:
    t0 = time.time()
    C = pd.read_parquet(data_file("cvx_hot_event_cases.parquet"))
    K = pd.read_parquet(data_file("cvx_hot_event_ctrl.parquet"))
    C["y"] = 1
    K["y"] = 0
    D = pd.concat([C, K], ignore_index=True)
    runs = np.sort(D.run.unique())
    rng = np.random.default_rng(SEED)
    fit_runs = set(rng.choice(runs, size=len(runs) // 2, replace=False).tolist())
    D["fit"] = D.run.isin(fit_runs)
    print("cases %s  controls %s  coins %s  (fit %s / hold %s)  %ds"
          % (f"{len(C):,}", f"{len(K):,}", f"{len(runs):,}", f"{len(fit_runs):,}",
             f"{len(runs) - len(fit_runs):,}", time.time() - t0), flush=True)

    for name, feats in (("PURE  coin-relative only", PURE), ("PLUS  + age / reserve / drawdown", PLUS)):
        cols = [c for c in feats if c in D.columns]
        X = D[cols].to_numpy(dtype=np.float64)
        miss = ~np.isfinite(X)
        Xi = np.where(miss, 0.0, X)
        # a missingness flag only where it is common enough to matter
        flags = [j for j in range(len(cols)) if miss[:, j].mean() > 0.02]
        if flags:
            Xi = np.hstack([Xi, miss[:, flags].astype(np.float64)])
            names = cols + [cols[j] + "_isNaN" for j in flags]
        else:
            names = list(cols)
        # clip the z-scores: an expanding z on a thin coin can be enormous
        Xi = np.clip(Xi, -8.0, 8.0)
        y = D.y.to_numpy(dtype=np.float64)
        f = D.fit.to_numpy()
        mu = Xi[f].mean(axis=0)
        sd = Xi[f].std(axis=0)
        sd[sd < 1e-9] = 1.0
        Z = (Xi - mu) / sd

        w = logistic_fit(Z[f], y[f])
        D["score"] = np.hstack([np.ones((len(Z), 1)), Z]) @ w

        a_fit = strat_auc(D[f])
        a_hold = strat_auc(D[~f])
        print("\n=== %s   (%d features)" % (name, len(names)), flush=True)
        print("  within-coin AUC   fit %.4f   HOLD %.4f" % (a_fit, a_hold), flush=True)

        # what the model actually leans on
        coef = pd.DataFrame(dict(feature=names, weight=w[1:]))
        coef["abs"] = coef.weight.abs()
        coef = coef.sort_values("abs", ascending=False).head(14)
        print(coef[["feature", "weight"]].to_string(index=False), flush=True)

        # lift by score decile, HOLD half, ranked inside each coin
        h = D[~f].copy()
        h["q"] = h.groupby("run").score.rank(pct=True)
        g = h.groupby(pd.cut(h.q, np.arange(0, 1.01, 0.1)), observed=True).agg(
            n=("y", "size"), their_buys=("y", "sum"))
        g["share_of_moments_that_are_theirs"] = (100 * g.their_buys / g.n).round(2)
        base = 100 * float(h.y.mean())
        g["lift"] = (g.share_of_moments_that_are_theirs / base).round(2)
        print("\n  HOLD half, moments ranked inside their own coin (base %.2f %%):" % base,
              flush=True)
        print(g[["n", "their_buys", "share_of_moments_that_are_theirs", "lift"]].to_string(),
              flush=True)
        if name.startswith("PURE"):
            np.save(data_file("cvx_hot_model_w.npy"), w)
            pd.Series(names).to_csv(data_file("cvx_hot_model_feats.csv"), index=False, header=False)
            pd.DataFrame(dict(mu=mu, sd=sd)).to_csv(data_file("cvx_hot_model_scale.csv"), index=False)

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
