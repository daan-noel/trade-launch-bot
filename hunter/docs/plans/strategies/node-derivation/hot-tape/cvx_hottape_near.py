"""Hot-tape re-entry, loop [4]: WHERE does the rule fire, relative to where they buy?

Loop [2] fired a pure tape-state event on the whole tape - no wallet appears in the firing
condition - and captured a +0.29 % price move where the node's 1.10 % NET margin implies
+3.65 %. That is a 3.4 point gap on the move itself, and there are only two ways to be
short: we pick different COINS, or we pick a different MOMENT on the same coin.

This script measures which. Their trades are used ONLY as a ruler here - a thermometer, not
a term (strategy 7.4 law 20). Nothing measured below can enter a sentence.

  1. what share of the rule's fires land on a coin the node never touches
  2. on the coins it does touch, how far in time the fire sits from the nearest node buy
  3. the book split by that distance, on the same clock-20 exit
  4. the reverse read: of THEIR buys, how many does the rule fire on at all

If the near fires pay and the far ones do not, the event is under-specified and something
is left in this node. If both are red, the event is simply not what they trade on.
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path
from _paths import data_file

import time
from pathlib import Path

import numpy as np
import pandas as pd
import psycopg2

from cvx import B, DAY0, cell, show
from kernel import book
from tape import Tape

HUNTER = _paths.HUNTER
LOCAL = HUNTER / "_local"
NODE_NAME = "hot-tape re-entry"
E1 = (-20.0, -5.0, 2.0, 20, 120.0, 42.43, 85.0)
EXIT = dict(cap=20.0)
GRID = {
    "cap10": dict(cap=10.0), "cap20": dict(cap=20.0), "cap60": dict(cap=60.0),
    "tp5_c30": dict(tp=5.0, cap=30.0), "tp10_c60": dict(tp=10.0, cap=60.0),
    "tp20_c300": dict(tp=20.0, cap=300.0),
    "tr20_c120": dict(trail=20.0, cap=120.0),
    "shipped": dict(arm=21.0, trail=36.0, stop=43.75, cap=1200.0),
}

pd.set_option("display.width", 400)
pd.set_option("display.max_rows", 300)


def load_url() -> str:
    for line in (HUNTER / ".env").read_text(encoding="utf-8").splitlines():
        s = line.strip()
        if s.startswith("DATABASE_URL=") and not s.startswith("#"):
            return s.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("DATABASE_URL missing")


def node_ids() -> dict[int, str]:
    roster = pd.read_csv(
        LOCAL / "solo-traders.csv", usecols=["wallet", "wallet_address", "node", "use_it"]
    )
    roster = roster[(roster.node == NODE_NAME) & (roster.use_it == "yes")]
    conn = psycopg2.connect(load_url())
    conn.autocommit = True
    cur = conn.cursor()
    cur.execute("SELECT id, left(address, 6) FROM public.wallet_dict WHERE address = ANY(%s)",
                (list(roster.wallet_address),))
    rows = cur.fetchall()
    conn.close()
    return {int(i): str(p) for i, p in rows}


def pct_from_reserve(v_now, v_then):
    with np.errstate(divide="ignore", invalid="ignore"):
        r = np.where(v_then > 0, (v_now / v_then) ** 2 - 1.0, np.nan)
    return r * 100.0


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    days = (T.t.max() - T.t.min()) / 86400.0

    tok = pd.read_parquet(data_file("cvx_tok.parquet")).drop_duplicates("mint").set_index("mint")
    tok["c_s"] = (pd.to_datetime(tok.created_at, utc=True, format="ISO8601")
                  - pd.Timestamp(0, tz="UTC")).dt.total_seconds()
    c_s = tok.reindex(T.mints).c_s.to_numpy()

    is_node_buy = np.isin(T.wallet, NODE) & (T.side == 1)
    touched = set(int(x) for x in np.unique(T.run_of[np.isin(T.wallet, NODE)]))
    print("tape prints %s  tokens %s  days %.2f  node buys %s on %s coins  %ds"
          % (f"{T.n:,}", f"{len(T.mints):,}", days, f"{int(is_node_buy.sum()):,}",
             f"{len(touched):,}", time.time() - t0), flush=True)

    dd_c, fall_c, acc_c, n20_c, age_lo, v_lo, v_hi = E1
    rows = []
    n_their_buys = 0
    n_their_buys_rule_on = 0

    for r in range(len(T.start)):
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 10 or not np.isfinite(c_s[r]):
            continue
        t = T.t[a:b]
        v = T.v[a:b]
        age = t - c_s[r]
        if age[-1] < age_lo:
            continue
        sol = T.sol[a:b]
        side = T.side[a:b]
        sl = T.slot[a:b]
        idx = np.arange(n)
        buy = side == 1
        cb = np.concatenate(([0.0], np.cumsum(np.where(buy, sol, 0.0))))
        j5 = np.searchsorted(t, t - 5.0, side="right")
        j10 = np.searchsorted(t, t - 10.0, side="right")
        j20 = np.searchsorted(t, t - 20.0, side="right")
        buy5 = cb[idx] - cb[j5]
        buy5p = cb[j5] - cb[j10]
        n20 = idx - j20
        vprev = np.concatenate(([np.nan], v[:-1]))
        fall5 = pct_from_reserve(vprev, v[np.maximum(j5 - 1, 0)])
        dd = pct_from_reserve(vprev, np.concatenate(([np.nan],
                                                     np.maximum.accumulate(v)[:-1])))
        with np.errstate(divide="ignore", invalid="ignore"):
            acc = np.where(buy5p > 1e-9, buy5 / buy5p, np.nan)

        m = ((dd <= dd_c) & (fall5 <= fall_c) & (acc >= acc_c) & (n20 >= n20_c)
             & (age >= age_lo) & (vprev >= v_lo) & (vprev <= v_hi))
        m = np.nan_to_num(m, nan=False).astype(bool)

        their = np.nonzero(is_node_buy[a:b])[0]
        if len(their):
            n_their_buys += len(their)
            n_their_buys_rule_on += int(m[their].sum())

        if not m.any():
            continue
        their_t = t[their] if len(their) else None
        last = -1
        for k in np.nonzero(m)[0]:
            k = int(k) - 1
            if k <= last or k < 1 or k >= n - 1:
                continue
            y, ei, xi, reason = book(t, v, sl, k, EXIT, lag=0.115, B=B)
            ys = [book(t, v, sl, k, g, lag=0.115, B=B)[0] for g in GRID.values()]
            if their_t is None:
                gap = np.inf
                signed = np.nan
            else:
                j = int(np.searchsorted(their_t, t[k]))
                cands = []
                if j < len(their_t):
                    cands.append(their_t[j] - t[k])
                if j > 0:
                    cands.append(their_t[j - 1] - t[k])
                signed = min(cands, key=abs)
                gap = abs(signed)
            rows.append(tuple([y, r, int(T.day[a + k]), (v[xi] / v[ei]) ** 2 - 1.0,
                               r in touched, float(gap), float(signed)] + ys))
            last = xi

        if r % 30000 == 0:
            print("  run %d  fires %s  %ds" % (r, f"{len(rows):,}", time.time() - t0), flush=True)

    d = pd.DataFrame(rows, columns=["y", "run", "day", "gross", "touched", "gap", "signed"]
                     + ["y_" + g for g in GRID])
    d.to_parquet(data_file("cvx_hottape_near.parquet"), index=False)

    print("\n=== 1. WHOSE COINS DOES THE RULE FIRE ON")
    print("  fires %s on %s coins" % (f"{len(d):,}", f"{d.run.nunique():,}"), flush=True)
    print("  on a coin the node NEVER touches: %.1f %% of fires, %.1f %% of coins"
          % (100 * float((~d.touched).mean()),
             100 * float(d[~d.touched].run.nunique()) / d.run.nunique()), flush=True)

    print("\n=== 2. HOW FAR THE FIRE SITS FROM THEIR NEAREST BUY (touched coins only)")
    dt = d[d.touched]
    if len(dt):
        q = dt.gap.quantile([0.1, 0.25, 0.5, 0.75, 0.9])
        print("  seconds  p10 %.1f  p25 %.1f  p50 %.1f  p75 %.1f  p90 %.1f"
              % tuple(q), flush=True)
        print("  within 5 s of one of their buys: %.1f %%  |  within 60 s: %.1f %%"
              % (100 * float((dt.gap <= 5).mean()), 100 * float((dt.gap <= 60).mean())),
              flush=True)
        print("  when close, do we arrive BEFORE them? %.1f %% of fires within 60 s"
              % (100 * float((dt[dt.gap <= 60].signed > 0).mean())), flush=True)

    print("\n=== 3. THE BOOK SPLIT BY THAT DISTANCE (clock 20 s, same exit throughout)")
    out = []
    def row(sub, name):
        if not len(sub):
            return dict(cut=name, n=0)
        c = cell(sub, days=days)
        per_day = sub.groupby("day").run.nunique()
        return dict(cut=name, n=c["n"], mints=c["mints"], sol=c["sol"], pct=c["pct"],
                    pos=c["pos"], win=c["win"],
                    move_pct=round(100 * float(sub.gross.mean()), 2),
                    floor="%d/%d" % (int((per_day >= 50).sum()), len(per_day)))
    out.append(row(d, "every fire"))
    out.append(row(d[~d.touched], "coin they never touch"))
    out.append(row(d[d.touched], "coin they do touch"))
    for lo, hi, nm in ((0, 2, "within 2 s of their buy"), (2, 5, "2-5 s"), (5, 30, "5-30 s"),
                       (30, 120, "30-120 s"), (120, 1e9, "over 120 s away")):
        out.append(row(dt[(dt.gap >= lo) & (dt.gap < hi)], nm))
    out.append(row(dt[(dt.gap <= 60) & (dt.signed > 0)], "within 60 s and BEFORE them"))
    out.append(row(dt[(dt.gap <= 60) & (dt.signed < 0)], "within 60 s and AFTER them"))
    show(out, "the rule's book by distance from the node's own buys")

    print("\n=== 4. THE REVERSE READ: how much of THEIR book does the rule even see")
    print("  their buys on the tape        %s" % f"{n_their_buys:,}", flush=True)
    print("  of those, the rule fires on   %s  (%.2f %%)"
          % (f"{n_their_buys_rule_on:,}", 100 * n_their_buys_rule_on / max(n_their_buys, 1)),
          flush=True)
    # ---- the last question this node gets: with a PERFECT oracle for coin and second,
    # is there any exit that pays the toll? Their wallets are the oracle - unshippable by
    # law 20, so this is a CEILING, not a candidate.
    ceil = dt[dt.gap <= 2.0]
    out = []
    for g in GRID:
        for nm, sub in (("every fire", d), ("within 2 s of their buy", ceil)):
            if not len(sub):
                continue
            c = cell(sub.assign(y=sub["y_" + g]), days=days)
            out.append(dict(exit=g, population=nm, n=c["n"], mints=c["mints"], sol=c["sol"],
                            pct=c["pct"], pos=c["pos"], win=c["win"], worst=c["worst"]))
    show(out, "THE CEILING: perfect coin and perfect second, every exit")

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
