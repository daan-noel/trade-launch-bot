"""Hot-tape node, step 0: is their buying a SESSION, and do the six agree?

The user's structural claim: these wallets watch time-windowed state continuously and buy
SEVERAL times once the conditions hold. If that is true it changes the unit of analysis and it
is the only anchor that can reach the PEER seat (evidence 1.4): a state true for seconds, not a
condition that becomes true at the print we react to.

Three things measured here, and nothing is guessed:

  1. SESSION STRUCTURE. Per (wallet, coin), the gaps between consecutive buys. If buying is
     Poisson the gap distribution is exponential and there are no sessions. If it clusters, the
     gaps are bimodal - short gaps inside a session, long gaps between them - and the SESSION,
     not the buy, is the unit whose start we have to name.
  2. THE DWELL BUDGET. How long a session lasts and how many buys it holds. A session that lasts
     seconds gives a rule seconds of room to arrive; a session that lasts 200 ms does not, and
     the node is unreachable whatever the state turns out to be.
  3. AGREEMENT. When one of the six opens a session on a coin, do the others open one near it?
     Six independent readers firing on the same seconds is evidence of a SHARED OBSERVABLE
     trigger - which is what makes the trigger findable at all. Measured against a
     time-shuffled null on the same coins, because these six live on the busiest tape and any
     two busy traders overlap by accident.

Their wallets are the instrument here. Nothing measured becomes a term (7.4 law 20).
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import psycopg2

from cvx import DAY0, show
from tape import Tape

HUNTER = _paths.HUNTER
LOCAL = HUNTER / "_local"
NODE_NAME = "hot-tape re-entry"
GAPS = (0.5, 1, 2, 5, 10, 20, 30, 60, 120, 300)
SEED = 20260910

pd.set_option("display.width", 400)
pd.set_option("display.max_rows", 300)
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


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    print("tape %s prints  node %d wallets  %ds"
          % (f"{T.n:,}", len(lab), time.time() - t0), flush=True)

    buy = T.side == 1
    rows = []
    for wid, nm in lab.items():
        m = (T.wallet == wid) & buy
        i = np.nonzero(m)[0]
        rows.append((nm, i))
    # ---- 1. the gap distribution between consecutive buys on the SAME coin ----------------
    out = []
    allgaps = {}
    for nm, i in rows:
        r = T.run_of[i]
        t = T.t[i]
        same = r[1:] == r[:-1]
        g = (t[1:] - t[:-1])[same]
        allgaps[nm] = g
        q = np.percentile(g, [10, 25, 50, 75, 90]) if len(g) else [np.nan] * 5
        out.append(dict(w=nm, buys=len(i), coins=len(np.unique(r)),
                        consecutive_pairs=len(g),
                        p10=round(q[0], 2), p25=round(q[1], 2), p50=round(q[2], 2),
                        p75=round(q[3], 2), p90=round(q[4], 2),
                        under_2s=round(100 * float((g < 2).mean()), 1) if len(g) else None,
                        under_10s=round(100 * float((g < 10).mean()), 1) if len(g) else None,
                        over_60s=round(100 * float((g > 60).mean()), 1) if len(g) else None))
    show(out, "gap between consecutive buys by the SAME wallet on the SAME coin, seconds")

    print("\n=== is the gap distribution BIMODAL (sessions) or exponential (no sessions)?")
    print("  an exponential with the same mean puts this share under each cut;")
    print("  a clustered process puts far MORE under the short cuts and more in the long tail.")
    hdr = "  %-8s %8s " % ("wallet", "mean_s") + " ".join("%7s" % ("<%gs" % c) for c in GAPS)
    print(hdr, flush=True)
    for nm, g in allgaps.items():
        if not len(g):
            continue
        mu = float(g.mean())
        obs = " ".join("%6.1f%%" % (100 * float((g < c).mean())) for c in GAPS)
        exp = " ".join("%6.1f%%" % (100 * (1 - np.exp(-c / mu))) for c in GAPS)
        print("  %-8s %8.1f %s   observed" % (nm, mu, obs), flush=True)
        print("  %-8s %8s %s   exponential null" % ("", "", exp), flush=True)

    # ---- 2. sessions, cut at the gap the distribution itself names -----------------------
    print("\n=== session structure at several session-break gaps", flush=True)
    out = []
    for brk in (5.0, 10.0, 20.0, 30.0, 60.0):
        n_sess = 0; n_buys = 0; durs = []; sizes = []; sols = []
        for wid, nm in lab.items():
            m = (T.wallet == wid) & buy
            i = np.nonzero(m)[0]
            r = T.run_of[i]; t = T.t[i]; s = T.sol[i]
            newrun = np.concatenate(([True], r[1:] != r[:-1]))
            newsess = np.concatenate(([True], (t[1:] - t[:-1]) > brk)) | newrun
            sid = np.cumsum(newsess)
            df = pd.DataFrame(dict(sid=sid, t=t, s=s))
            g = df.groupby("sid").agg(n=("t", "size"), dur=("t", lambda x: x.max() - x.min()),
                                      sol=("s", "sum"))
            n_sess += len(g); n_buys += len(i)
            durs.append(g.dur.values); sizes.append(g.n.values); sols.append(g.sol.values)
        durs = np.concatenate(durs); sizes = np.concatenate(sizes); sols = np.concatenate(sols)
        out.append(dict(break_gap_s=brk, sessions=n_sess, buys=n_buys,
                        buys_per_session=round(n_buys / n_sess, 2),
                        multi_buy_pct=round(100 * float((sizes > 1).mean()), 1),
                        dur_p50=round(float(np.median(durs)), 1),
                        dur_p90=round(float(np.percentile(durs, 90)), 1),
                        dur_p50_multi=round(float(np.median(durs[sizes > 1])), 1),
                        sol_p50=round(float(np.median(sols)), 3)))
    show(out, "sessions: how much room a rule has to arrive")

    # ---- 3. do the six agree on the same seconds? ----------------------------------------
    print("\n=== agreement: when one opens a session, do the others buy near it?", flush=True)
    rng = np.random.default_rng(SEED)
    # session opens per wallet at a 20 s break
    opens = {}
    for wid, nm in lab.items():
        m = (T.wallet == wid) & buy
        i = np.nonzero(m)[0]
        r = T.run_of[i]; t = T.t[i]
        newrun = np.concatenate(([True], r[1:] != r[:-1]))
        newsess = np.concatenate(([True], (t[1:] - t[:-1]) > 20.0)) | newrun
        opens[nm] = (r[newsess], t[newsess])
    # all node buys by coin, for the lookup
    allbuy = np.nonzero(np.isin(T.wallet, NODE) & buy)[0]
    by_coin: dict[int, dict[str, np.ndarray]] = {}
    lab_of = {wid: nm for wid, nm in lab.items()}
    for i in allbuy:
        r = int(T.run_of[i])
        by_coin.setdefault(r, {}).setdefault(lab_of[int(T.wallet[i])], []).append(T.t[i])
    for r in by_coin:
        for w in by_coin[r]:
            by_coin[r][w] = np.sort(np.asarray(by_coin[r][w]))
    # coin time spans, for the shuffled null
    span = {}
    for r in by_coin:
        a, b = T.start[r], T.end[r]
        span[r] = (T.t[a], T.t[b - 1])

    out = []
    for nm, (rs, ts) in opens.items():
        for W in (2.0, 10.0, 60.0):
            hit = 0; hit0 = 0; n = 0
            for r, t in zip(rs, ts):
                r = int(r)
                others = {k: v for k, v in by_coin.get(r, {}).items() if k != nm}
                if not others:
                    continue
                n += 1
                near = any(np.any(np.abs(v - t) <= W) for v in others.values())
                lo, hi = span[r]
                tr = rng.uniform(lo, hi)
                near0 = any(np.any(np.abs(v - tr) <= W) for v in others.values())
                hit += int(near); hit0 += int(near0)
            if n:
                out.append(dict(w=nm, window_s=W, opens_on_shared_coins=n,
                                another_buys_within=round(100 * hit / n, 1),
                                time_shuffled_null=round(100 * hit0 / n, 1),
                                lift=round((hit / n) / (hit0 / n), 2) if hit0 else None))
    show(out, "agreement between the six, against a same-coin time-shuffled null")

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
