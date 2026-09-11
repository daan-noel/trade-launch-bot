"""Hot-tape node, step 2: name the trigger, WITHIN THE MINT.

The design point that makes this different from every earlier instrument step. If a wallet buys
one mint several times, those decisions share EVERYTHING about the mint - the Telegram channel,
the narrative, the creator, the launch build, the meta, the hour. All constant. So comparing
their buy moments against non-buy moments ON THE SAME MINT means any surviving difference must
be an on-chain, observable, timing state. Off-chain data is excluded by construction, not by
assumption, and the DOOR is held fixed so the EVENT is measured alone.

cvx_hottape.py compared their buys against a sample of the whole tape and therefore mixed "they
chose this coin" with "they chose this second". That is the error this script exists to remove.

  CASES     every node buy print
  CONTROLS  non-buy prints on the SAME mint within +/- 60 s of a case, 4 per case
  STRATUM   the mint. Within each stratum a case is scored by its percentile rank among that
            stratum's controls, so 0.50 is "no information" and the deviation is the signal.
            Stratified rank, i.e. a Wilcoxon conditioned on the coin.

TWO BASES for every feature, because a single ruler cannot serve 11,765 coins:
  ABS   the raw value
  LOC   the coin's own z-score - (value - the coin's prior mean) / the coin's prior sd, built
        from an EXPANDING window over that coin's earlier prints only. A -5 % dip on a coin that
        dips 15 % a minute is nothing; on a calm coin it is an event.

EVERY feature is NODE-BLIND, counts and timing included. The first pass blinded only the SOL
windows and left `gap`, `n2`, `n20`, `n60` and `vol20` counting ALL prints - and since the six
agree with each other within 2 s at a lift of 6-9x (1.5), those five features were partly
measuring "another of the six just printed". Counts, gaps and the volatility window are now
built on the PUBLIC sub-tape only. Every feature is built from prints 0..i-1 and the reserve
v[i-1], so a buy is never inside its own window.

Their wallets are the instrument. Nothing here becomes a term (7.4 law 20).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import psycopg2
import pyarrow.parquet as pq

from cvx import DAY0, show
from tape import Tape

HUNTER = _paths.HUNTER
LOCAL = HUNTER / "_local"
NODE_NAME = "hot-tape re-entry"
N_CTRL = 4
CTRL_WIN = 60.0
SEED = 20260910
ROUTERS = ("Axiom", "Terminal", "GMGN", "Photon", "Bloom", "Trojan", "BullX", "Padre", "DFlow")

pd.set_option("display.width", 400)
pd.set_option("display.max_rows", 400)
pd.set_option("display.max_columns", 40)


def load_url() -> str:
    for line in (HUNTER / ".env").read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing")


def node_ids() -> dict[int, str]:
    R = pd.read_csv(LOCAL / "solo-traders.csv",
                    usecols=["wallet", "wallet_address", "node", "use_it"])
    R = R[(R.node == NODE_NAME) & (R.use_it == "yes")]
    conn = psycopg2.connect(load_url())
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SELECT id, left(address, 6) FROM public.wallet_dict WHERE address = ANY(%s)",
                (list(R.wallet_address),))
    rows = cur.fetchall()
    conn.close()
    return {int(i): str(p) for i, p in rows}


def expanding_z(x):
    """(x - prior mean) / prior sd, using only that coin's EARLIER prints. NaN before n=8."""
    n = len(x)
    xf = np.where(np.isfinite(x), x, 0.0)
    ok = np.isfinite(x).astype(np.float64)
    c1 = np.concatenate(([0.0], np.cumsum(xf)))
    c2 = np.concatenate(([0.0], np.cumsum(xf * xf)))
    cn = np.concatenate(([0.0], np.cumsum(ok)))
    cnt = cn[:n]                       # count of finite values strictly before i
    s1 = c1[:n]; s2 = c2[:n]
    with np.errstate(divide="ignore", invalid="ignore"):
        mu = s1 / cnt
        var = s2 / cnt - mu * mu
        sd = np.sqrt(np.maximum(var, 0.0))
        z = (x - mu) / sd
    z[cnt < 8] = np.nan
    z[~np.isfinite(z)] = np.nan
    return z


def pct_from_reserve(a, b):
    with np.errstate(divide="ignore", invalid="ignore"):
        return (np.where(b > 0, (a / b) ** 2 - 1.0, np.nan)) * 100.0


def features(t, v, sol, side, age, mine, router):
    """State strictly before each print. Counts, gaps and flow all on the PUBLIC sub-tape."""
    n = len(t)
    i = np.arange(n)
    buy = side == 1
    pub = ~mine
    pubf = pub.astype(np.float64)

    def cum(mask):
        return np.concatenate(([0.0], np.cumsum(np.where(mask, sol, 0.0))))

    cbx, csx = cum(buy & pub), cum((~buy) & pub)
    cnt = np.concatenate(([0.0], np.cumsum(pubf)))          # PUBLIC prints before i
    cnt_rt = np.concatenate(([0.0], np.cumsum((router & pub).astype(np.float64))))

    j1 = np.searchsorted(t, t - 1.0, side="right")
    j2 = np.searchsorted(t, t - 2.0, side="right")
    j5 = np.searchsorted(t, t - 5.0, side="right")
    j10 = np.searchsorted(t, t - 10.0, side="right")
    j20 = np.searchsorted(t, t - 20.0, side="right")
    j60 = np.searchsorted(t, t - 60.0, side="right")
    j300 = np.searchsorted(t, t - 300.0, side="right")

    vprev = np.concatenate(([np.nan], v[:-1]))
    vb = lambda jj: v[np.maximum(jj - 1, 0)]
    peak = np.concatenate(([np.nan], np.maximum.accumulate(v)[:-1]))

    buy5 = cbx[i] - cbx[j5]; sell5 = csx[i] - csx[j5]
    buy2 = cbx[i] - cbx[j2]; sell2 = csx[i] - csx[j2]
    buy20 = cbx[i] - cbx[j20]; sell20 = csx[i] - csx[j20]
    buy5p = cbx[j5] - cbx[j10]

    # time since the previous PUBLIC print
    pub_idx = np.nonzero(pub)[0]
    t_pub = t[pub_idx] if len(pub_idx) else np.array([np.inf])
    prev_pub = np.searchsorted(t_pub, t, side="left") - 1
    gap_pub = np.where(prev_pub >= 0, t - t_pub[np.maximum(prev_pub, 0)], np.nan)

    # per-print price move, PUBLIC prints only, for the volatility window
    d = np.concatenate(([np.nan], pct_from_reserve(v[1:], v[:-1])))
    d2 = np.where(np.isfinite(d) & pub, d * d, 0.0)
    cd = np.concatenate(([0.0], np.cumsum(d2)))

    with np.errstate(divide="ignore", invalid="ignore"):
        f = {}
        f["age"] = age
        f["v"] = vprev
        f["dd_peak"] = pct_from_reserve(vprev, peak)
        f["mv1"] = pct_from_reserve(vprev, vb(j1))
        f["mv2"] = pct_from_reserve(vprev, vb(j2))
        f["mv5"] = pct_from_reserve(vprev, vb(j5))
        f["mv20"] = pct_from_reserve(vprev, vb(j20))
        f["mv60"] = pct_from_reserve(vprev, vb(j60))
        f["n2"] = cnt[i] - cnt[j2]
        f["n5"] = cnt[i] - cnt[j5]
        f["n20"] = cnt[i] - cnt[j20]
        f["n60"] = cnt[i] - cnt[j60]
        f["n300"] = cnt[i] - cnt[j300]
        f["gap"] = gap_pub
        f["buy2"] = buy2
        f["buy5"] = buy5
        f["sell2"] = sell2
        f["sell5"] = sell5
        f["net5"] = buy5 - sell5
        f["bshare2"] = np.where(buy2 + sell2 > 0, buy2 / (buy2 + sell2), np.nan)
        f["bshare5"] = np.where(buy5 + sell5 > 0, buy5 / (buy5 + sell5), np.nan)
        f["bshare20"] = np.where(buy20 + sell20 > 0, buy20 / (buy20 + sell20), np.nan)
        f["acc"] = np.where(buy5p > 1e-9, buy5 / buy5p, np.nan)
        f["rt_share20"] = np.where(cnt[i] - cnt[j20] > 0,
                                   (cnt_rt[i] - cnt_rt[j20]) / (cnt[i] - cnt[j20]), np.nan)
        f["mv5_minus_mv20"] = f["mv5"] - f["mv20"]
        f["vol20"] = np.sqrt(np.maximum((cd[i] - cd[j20])
                                        / np.maximum(cnt[i] - cnt[j20], 1), 0.0))
        # the LULL-then-IMPULSE contrast, in the coin's own units: how much busier the last
        # two seconds are than the last minute has been
        rate2 = (cnt[i] - cnt[j2]) / 2.0
        rate60 = (cnt[i] - cnt[j60]) / 60.0
        f["wake"] = np.where(rate60 > 0, rate2 / rate60, np.nan)
        f["quiet60"] = rate60
        f["sol60"] = (cbx[i] - cbx[j60]) + (csx[i] - csx[j60])
    return f


FEATS = ["age", "v", "dd_peak", "mv1", "mv2", "mv5", "mv20", "mv60", "n2", "n5", "n20", "n60",
         "n300", "gap", "buy2", "buy5", "sell2", "sell5", "net5", "bshare2", "bshare5",
         "bshare20", "acc", "rt_share20", "mv5_minus_mv20", "vol20", "wake", "quiet60", "sol60"]
LOCAL_Z = ["dd_peak", "mv1", "mv2", "mv5", "mv20", "mv60", "n2", "n5", "n20", "n60", "gap",
           "buy2", "buy5", "sell2", "sell5", "net5", "acc", "vol20", "wake", "quiet60",
           "sol60"]


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)

    ix = pq.read_table(str(data_file("cvx_ix.parquet")), columns=["tool"])
    tool = ix.column("tool").to_pandas()
    router_all = tool.isin(ROUTERS).to_numpy()
    assert len(router_all) == T.n, "cvx_ix.parquet is not aligned with the tape"

    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()

    is_node = np.isin(T.wallet, NODE)
    runs = np.unique(T.run_of[np.nonzero(is_node)[0]])
    rng = np.random.default_rng(SEED)
    print("tape %s prints  node coins %s  %ds"
          % (f"{T.n:,}", f"{len(runs):,}", time.time() - t0), flush=True)

    cols = FEATS + [c + "_z" for c in LOCAL_Z]
    case_rows, ctrl_rows = [], []

    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 20 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]
        mine = is_node[a:b]
        cases = np.nonzero(mine & (side == 1))[0]
        if not len(cases):
            continue
        f = features(t, v, sol, side, t - c_s[r], mine, router_all[a:b])
        for c in LOCAL_Z:
            f[c + "_z"] = expanding_z(f[c])
        M = np.column_stack([f[c] for c in cols]).astype(np.float32)

        # controls: prints on the SAME coin near a case, never a node print
        near = np.zeros(n, dtype=bool)
        for k in cases:
            lo = np.searchsorted(t, t[k] - CTRL_WIN, side="left")
            hi = np.searchsorted(t, t[k] + CTRL_WIN, side="right")
            near[lo:hi] = True
        near &= ~mine
        pool = np.nonzero(near)[0]
        if len(pool) < 4:
            continue
        want = min(len(pool), N_CTRL * len(cases))
        pick = rng.choice(pool, size=want, replace=False)

        case_rows.append(np.column_stack([np.full(len(cases), r, dtype=np.float32), M[cases]]))
        ctrl_rows.append(np.column_stack([np.full(len(pick), r, dtype=np.float32), M[pick]]))
        if r % 3000 == 0:
            print("  run %d  cases %s  %ds"
                  % (r, f"{sum(len(x) for x in case_rows):,}", time.time() - t0), flush=True)

    C = pd.DataFrame(np.vstack(case_rows), columns=["run"] + cols)
    K = pd.DataFrame(np.vstack(ctrl_rows), columns=["run"] + cols)
    C.to_parquet(data_file("cvx_hot_event_cases.parquet"), index=False)
    K.to_parquet(data_file("cvx_hot_event_ctrl.parquet"), index=False)
    print("\ncases %s  controls %s  strata %s  %ds"
          % (f"{len(C):,}", f"{len(K):,}", f"{C.run.nunique():,}", time.time() - t0), flush=True)

    # ---- stratified rank: a case's percentile among its OWN coin's controls -------------
    print("\nscoring %d features by stratified rank ... " % len(cols), flush=True)
    out = []
    kg = {r: g for r, g in K.groupby("run", sort=False)}
    for col in cols:
        num = 0.0; den = 0
        for r, gc in C.groupby("run", sort=False):
            gk = kg.get(r)
            if gk is None:
                continue
            cv = gc[col].to_numpy(); kv = gk[col].to_numpy()
            kv = kv[np.isfinite(kv)]
            if len(kv) < 3:
                continue
            cv = cv[np.isfinite(cv)]
            if not len(cv):
                continue
            kv = np.sort(kv)
            lo = np.searchsorted(kv, cv, side="left")
            hi = np.searchsorted(kv, cv, side="right")
            num += float(((lo + hi) / 2.0 / len(kv)).sum())
            den += len(cv)
        if den:
            mp = num / den
            out.append(dict(feature=col, cases_scored=den, mean_pctile=round(mp, 4),
                            deviation=round(abs(mp - 0.5), 4),
                            direction="HIGH" if mp > 0.5 else "low"))
    d = pd.DataFrame(out).sort_values("deviation", ascending=False)
    print("\n=== stratified rank of a node buy among non-buy prints ON THE SAME COIN")
    print("    0.5000 = the feature carries no information about WHEN they buy")
    print(d.to_string(index=False), flush=True)
    d.to_csv(data_file("cvx_hot_event_rank.csv"), index=False)
    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
