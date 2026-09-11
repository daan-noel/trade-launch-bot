"""Hot-tape node, step 1: HOW SLOW ARE THEY? The go/no-go gate.

Evidence 1.4 says the seat term that dominates everything is being sequenced AFTER a print
(-5.59 pp) rather than the 115 ms itself (-0.77 pp). The six hot-tape wallets agree with each
other within 2 s at a lift of 6-9x over a time-shuffled null, so they are reacting to a shared,
public, tape-visible trigger.

That makes one number decide the node: the delay between the trigger and THEIR order landing.
If they take 0.5-2 s, our 115 ms lands first and we get the RACE seat (+2.77 %/trade, 8/8 days).
If they take under ~0.15 s, we are behind them forever and the node is closed.

Measured four ways, three of them free of any assumption about WHAT the trigger is:

  A  WITHIN-CLUSTER SPREAD. When 2+ of the six buy the same coin within 2 s, the spread between
     first and last arrival is a lower bound on the slowest one's reaction. No trigger needed.
  B  POSITION IN THE INTER-PRINT GAP. If they are print-reactive they land SOON after a print;
     if they are not, their position inside the gap is uniform. Compared against the coin's own
     gap distribution at the same moment, so a busy coin is not confused with a fast trader.
  C  DELAY FROM FOUR CANDIDATE TRIGGERS, each a public print definition.
  D  THE OPERATIONAL TEST. Fire at the trigger, fill at the last print landed by trigger+115 ms,
     and ask whether that fill index is strictly BEFORE their print. That is the RACE seat,
     asked directly.

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

from kernel import fill_idx
from cvx import DAY0, show
from tape import Tape

HUNTER = _paths.HUNTER
LOCAL = HUNTER / "_local"
NODE_NAME = "hot-tape re-entry"
LOOKBACK = 15.0          # seconds a trigger may sit before the buy

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


def q(a, name, unit="s"):
    a = np.asarray(a, dtype=float)
    a = a[np.isfinite(a)]
    if not len(a):
        return dict(what=name, n=0)
    p = np.percentile(a, [10, 25, 50, 75, 90])
    return dict(what=name, n=len(a), p10=round(p[0], 3), p25=round(p[1], 3),
                p50=round(p[2], 3), p75=round(p[3], 3), p90=round(p[4], 3),
                mean=round(float(a.mean()), 3), over_115ms=round(100 * float((a > 0.115).mean()), 1))


def main() -> None:
    t0 = time.time()
    lab = node_ids()
    NODE = list(lab)
    T = Tape(str(data_file("cvx_prints.parquet")),
             cols=["mint", "slot", "t_ms", "reserve_lamports", "amount_lamports", "side",
                   "wallet_id", "build"])
    T.day = ((T.t - DAY0) // 86400).astype(np.int16)
    is_node = np.isin(T.wallet, NODE)
    print("tape %s prints  node %d wallets  %ds"
          % (f"{T.n:,}", len(lab), time.time() - t0), flush=True)

    runs = np.unique(T.run_of[np.nonzero(is_node)[0]])

    spread, npair, gap_before, gap_coin, pos_in_gap = [], [], [], [], []
    trig = {k: [] for k in ("big1", "leg_up", "flush", "burst_start")}
    win = {k: [] for k in trig}
    nprints_between = {k: [] for k in trig}
    per_wallet = {nm: {k: [] for k in trig} for nm in lab.values()}

    for r in runs:
        r = int(r)
        a, b = T.start[r], T.end[r]
        n = b - a
        if n < 5:
            continue
        t = T.t[a:b]; v = T.v[a:b]; sol = T.sol[a:b]; side = T.side[a:b]; wal = T.wallet[a:b]
        loc = np.nonzero(np.isin(wal, NODE) & (side == 1))[0]
        if not len(loc):
            continue
        vbef = v - sol * np.where(side == 1, 1.0, -1.0)
        with np.errstate(divide="ignore", invalid="ignore"):
            mv = (v / vbef) ** 2 - 1.0            # price move caused by each print
        dt = np.diff(t, prepend=t[0])

        # trigger candidate masks, all public and all strictly earlier than the buy
        pub = ~np.isin(wal, NODE)
        m_big1 = pub & (side == 1) & (sol >= 1.0)
        m_legup = pub & (mv >= 0.01)
        m_flush = pub & (mv <= -0.02)
        m_burst = pub & (dt >= 0.4)
        masks = dict(big1=m_big1, leg_up=m_legup, flush=m_flush, burst_start=m_burst)
        idx = {k: np.nonzero(mk)[0] for k, mk in masks.items()}

        # ---- A: within-cluster spread ---------------------------------------------------
        ct = t[loc]; cw = wal[loc]
        i = 0
        while i < len(loc):
            j = i
            while j + 1 < len(loc) and ct[j + 1] - ct[i] <= 2.0:
                j += 1
            if j > i and len(np.unique(cw[i:j + 1])) >= 2:
                spread.append(ct[j] - ct[i])
                npair.append(len(np.unique(cw[i:j + 1])))
            i = j + 1

        # ---- B, C, D --------------------------------------------------------------------
        for k in loc:
            k = int(k)
            if k == 0:
                continue
            gap_before.append(t[k] - t[k - 1])
            lo = np.searchsorted(t, t[k] - 20.0, side="left")
            if k - lo >= 5:
                gaps = np.diff(t[lo:k + 1])
                gap_coin.append(float(np.median(gaps)))
                pos_in_gap.append(float((t[k] - t[k - 1]) / max(np.median(gaps), 1e-6)))
            for key, ii in idx.items():
                p = ii[(ii < k) & (t[ii] >= t[k] - LOOKBACK)]
                if not len(p):
                    continue
                jtrig = int(p[-1])
                d = float(t[k] - t[jtrig])
                trig[key].append(d)
                nprints_between[key].append(k - jtrig - 1)
                win[key].append(1.0 if fill_idx(t, jtrig, 0.115) < k else 0.0)
                per_wallet[lab[int(wal[k])]][key].append(d)
        if r % 4000 == 0:
            print("  run %d  buys seen %s  %ds"
                  % (r, f"{len(gap_before):,}", time.time() - t0), flush=True)

    print("\n=== A. WITHIN-CLUSTER ARRIVAL SPREAD (no trigger assumed)")
    print("  clusters with 2+ distinct wallets inside 2 s: %s" % f"{len(spread):,}", flush=True)
    show([q(spread, "first arrival -> last arrival, seconds")], "the slowest one is at least this slow")
    if spread:
        s = np.asarray(spread)
        print("  share of clusters whose spread exceeds 115 ms: %.1f %%"
              % (100 * float((s > 0.115).mean())), flush=True)
        print("  share exceeding 400 ms (one slot): %.1f %%   1 s: %.1f %%"
              % (100 * float((s > 0.4).mean()), 100 * float((s > 1.0).mean())), flush=True)

    print("\n=== B. ARE THEY PRINT-REACTIVE AT ALL?")
    show([q(gap_before, "seconds from the PREVIOUS print to their buy"),
          q(gap_coin, "the coin's own median inter-print gap at that moment"),
          q(pos_in_gap, "their gap divided by the coin's median gap")],
         "if they were print-reactive this ratio would sit far below 1")

    print("\n=== C. DELAY FROM EACH CANDIDATE TRIGGER, and D. WOULD WE BE FIRST?")
    rows = []
    for key in trig:
        row = q(trig[key], key)
        if row.get("n"):
            row["prints_between_p50"] = int(np.median(nprints_between[key]))
            row["WE_FILL_FIRST_pct"] = round(100 * float(np.mean(win[key])), 1)
        rows.append(row)
    show(rows, "delay trigger -> their buy, and the share where our 115 ms fill lands BEFORE them")

    print("\n=== per wallet, delay from the nearest big public buy (>= 1 SOL)")
    rows = []
    for nm, dd in per_wallet.items():
        row = q(dd["big1"], nm)
        rows.append(row)
    show(rows, "reaction delay by wallet")

    print("\ndone %ds" % (time.time() - t0), flush=True)


if __name__ == "__main__":
    main()
