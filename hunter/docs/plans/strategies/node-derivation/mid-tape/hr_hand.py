"""Hit-rate method test, step 2: count coins one print at a time, sharing no code with hr_core.

Reads the tape rows of a coin, recomputes every fact with a plain loop (quiet gap, move, new
wallet, buys in 2 s), walks the chances and the fake trader's positions by hand, and compares the
chance list and the hit list with what hr_core counted (saved by hr_synth.py). Every coin must
match exactly.

  python hr_hand.py
"""
import math
import random
from collections import deque

import numpy as np
import pyarrow.parquet as pq

import _paths  # noqa: F401
from _paths import DATA

W, CAP = 3, 5
S = np.load(DATA / "hr_synth_trader.npz")
coin_col = pq.read_table(str(DATA / "hr_tape.parquet"), columns=["coin"]).column("coin").to_numpy()
TAB = pq.ParquetFile(str(DATA / "hr_tape.parquet"))
FULL = TAB.read(columns=["slot", "t", "buy", "sol", "v", "wh"])
positions = list(zip(S["p_coin"], S["p_in"], S["p_out"], S["t_in"], S["t_out"]))
entries = list(zip(S["e_coin"], S["e_pos"], S["e_slot"]))
active = list(zip(S["act_lo"], S["act_hi"]))
slot0 = int(S["slot0"])


def rules(rows):
    """Rule A and rule B at every print of one coin, by a plain loop."""
    out, seen, buys = [], set(), deque()
    prev_t = prev_v = None
    for r in rows:
        gap = math.inf if prev_t is None else r["t"] - prev_t
        vb = r["v"] if prev_v is None else prev_v
        move = (r["v"] / vb) ** 2 - 1
        new = r["wh"] not in seen
        seen.add(r["wh"])
        if r["buy"]:
            buys.append(r["t"])
        while buys and buys[0] < r["t"] - 2.0:
            buys.popleft()
        nb = len(buys)
        a = r["buy"] and gap >= 0.4 and move >= 0.019 and new and r["v"] < 40 and r["sol"] >= 2.0
        b = nb >= 20 and 72 <= r["v"] < 78
        out.append((a, b))
        prev_t, prev_v = r["t"], r["v"]
    return out


def count(c, lo, rows, flags):
    """Chances and hits of one rule on one coin, walked by hand."""
    ch = []
    prev, last_true = False, None
    for i, (r, f) in enumerate(zip(rows, flags)):
        g = lo + i
        s = r["slot"] - slot0
        if f and not prev and (last_true is None or s - last_true > W):
            holding = any(pc == c and pi <= g < po for pc, pi, po, _, _ in positions)
            openn = sum(1 for _, _, _, ti, to in positions if ti <= r["t"] < to)
            live = any(a <= r["t"] <= b for a, b in active)
            if not holding and openn < CAP and live:
                ch.append((g, s))
        if f:
            last_true = s
        prev = f
    hits, flight = set(), set()
    for ec, ep, es in entries:
        if ec != c:
            continue
        inside = [g for g, s in ch if g < ep and s >= es - W]
        if inside:
            hits.add(inside[0])
            flight.update(inside[1:])
    counted = [g for g, _ in ch if g not in flight]
    return counted, sorted(hits)


def check(c):
    idx = np.flatnonzero(coin_col == c)
    lo, hi = int(idx[0]), int(idx[-1]) + 1
    rows = FULL.slice(lo, hi - lo).to_pylist()
    fl = rules(rows)
    ok = True
    for j, nm in enumerate(("A", "B")):
        counted, hits = count(c, lo, rows, [f[j] for f in fl])
        k, kh = S[nm + "_k"], S[nm + "_hit"]
        m = (k >= lo) & (k < hi)
        mine_k, mine_h = list(k[m]), sorted(k[m][kh[m]])
        same = counted == mine_k and hits == mine_h
        ok &= same
        if not same:
            print("  MISMATCH coin %d rule %s: hand %d/%d, counter %d/%d" % (
                c, nm, len(hits), len(counted), len(mine_h), len(mine_k)))
    return ok, hi - lo, rows, fl, lo


random.seed(3)
traded = sorted(set(int(x) for x in S["e_coin"]))
chance_coins = sorted(set(int(coin_col[x]) for x in np.concatenate((S["A_k"], S["B_k"]))))
pool = [c for c in set(traded) | set(chance_coins)]
sample = random.sample(pool, 300)
n_ok = n_rows = 0
for c in sample:
    ok, n, *_ = check(c)
    n_ok += ok
    n_rows += n
print("hand count vs counter: %d of %d coins identical (%s prints walked)" % (
    n_ok, len(sample), f"{n_rows:,}"))

# one small coin laid out, for reading
for c in traded:
    idx = np.flatnonzero(coin_col == c)
    if 25 <= len(idx) <= 60 and c in set(int(coin_col[x]) for x in S["A_k"]):
        ok, n, rows, fl, lo = check(c)
        ents = {ep: es for ec, ep, es in entries if ec == c}
        k, kh = S["A_k"], S["A_hit"]
        chances = dict(zip(k, kh))
        print("\ncoin %d, rule A, %d prints (%s):" % (c, n, "matches" if ok else "MISMATCH"))
        print("  #   sec     side  SOL    vsol  A?  note")
        t0 = rows[0]["t"]
        for i, (r, f) in enumerate(zip(rows, fl)):
            g = lo + i
            note = ""
            if g in chances:
                note = "CHANCE -> HIT" if chances[g] else "CHANCE -> miss"
            if g in ents:
                note += "   <- fake trader buys just before this print"
            print("  %-3d %6.1f  %-4s %6.2f  %5.1f  %-3s %s" % (
                i, r["t"] - t0, "buy" if r["buy"] else "sell", r["sol"], r["v"],
                "yes" if f[0] else "", note))
        break
