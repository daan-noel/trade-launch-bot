"""Hit-rate method test, step 1: a fake trader whose rules we know, measured by the counter.

The fake trader reads the real tape (09-01 .. 09-06) and buys by two known rules plus a few
random buys (its "unknown" logic). It skips some chances on purpose, holds one position a coin,
holds at most 5 coins, and is switched off 10:00-12:00 UTC every day. The counter then measures
the true rules, near-miss rules and random rules. The method is right if the true rules score
their known take rate and every wrong rule scores lower.

  python hr_synth.py
"""
import heapq

import numpy as np

from hr_core import Tape, lags, measure, old_lift, reaction_window

RNG = np.random.default_rng(7)
CAP = 5
LAG_SLOTS = (1, 2, 3)          # its reaction: 1-3 slots after the chance
W = 3                          # the true reaction window
HOLD = (5.0, 60.0)             # seconds held, uniform
OFF = (36000, 43200)           # switched off 10:00-12:00 UTC each day
TAKE = {"A": 0.80, "B": 0.70, "noise": 1.0}
NOISE_PER_DAY = 60
import os
GAP_OFF = float(os.environ.get("GAP_OFF", 1800))  # the counter's inactivity gap: no trade for this long = off
BUILD, CHECK = [0, 2, 4], [1, 3, 5]

T = Tape()
pub = np.ones(T.n, bool)
mv, g = T.move(), T.gap(pub)
nb2 = T.buys_in(pub, 2.0)
E = T.buy & (g >= 0.4) & (mv >= 0.019) & T.first_here
RULE_A = E & (T.v < 40) & (T.sol >= 2.0)
RULE_B = (nb2 >= 20) & (T.v >= 72) & (T.v < 78)

ev = []
for tag, C in (("A", RULE_A), ("B", RULE_B)):
    for k in T.edges(C, pub, W):
        ev.append((T.t[k], k, tag))
nz = RNG.choice(np.flatnonzero(T.buy), size=NOISE_PER_DAY * 6, replace=False)
ev += [(T.t[k], k, "noise") for k in nz]
ev.sort()

# ---- the fake trader -------------------------------------------------------------------------
busy = {}                      # coin -> busy until t (decision .. exit)
exits = []                     # heap of exit times of open positions
entries, positions, truth = [], [], {"A": [0, 0], "B": [0, 0]}
for t, k, tag in ev:
    c = T.coin[k]
    while exits and exits[0] <= t:
        heapq.heappop(exits)
    if busy.get(c, -1) > t or len(exits) >= CAP or OFF[0] <= t % 86400 < OFF[1]:
        continue
    took = RNG.random() < TAKE[tag]
    if tag in truth:
        truth[tag][0] += 1
        truth[tag][1] += took
    if not took:
        continue
    s_e = T.slot[k] + RNG.choice(LAG_SLOTS)
    t_e = T.t[k] + 0.4 * (s_e - T.slot[k])
    end = T.cend[c - 1]
    pos = min(np.searchsorted(T.ks, np.int64(c) * (1 << 32) + s_e, side="right"), end)
    t_x = t_e + RNG.uniform(*HOLD)
    pos_x = max(min(np.searchsorted(T.kt, c * 1e7 + t_x, side="right"), end), pos)
    busy[c] = t_x
    heapq.heappush(exits, t_x)
    entries.append((c, pos, s_e, t_e, tag))
    positions.append((c, pos, pos_x, t_e, t_x))

e_coin = np.array([e[0] for e in entries], np.int64)
e_pos = np.array([e[1] for e in entries], np.int64)
e_slot = np.array([e[2] for e in entries], np.int64)
e_tag = np.array([e[4] for e in entries])
ENT = (e_coin, e_pos, e_slot)
P = tuple(np.array([p[i] for p in positions]) for i in range(5))
# active spans, inferred the way the real trader's are: runs of its trades with no gap > 15 min
tt = np.sort(np.concatenate((P[3], P[4])))
cut = np.flatnonzero(np.diff(tt) > GAP_OFF)
ACT = (tt[np.concatenate(([0], cut + 1))], tt[np.concatenate((cut, [len(tt) - 1]))])

print("fake trader: %d entries (%s), %.0f a day" % (
    len(entries), ", ".join("%s %d" % (x, (e_tag == x).sum()) for x in ("A", "B", "noise")),
    len(entries) / 6))
for x in ("A", "B"):
    print("  truth %s: took %d of %d eligible chances = %.1f %%" % (
        x, truth[x][1], truth[x][0], 100 * truth[x][1] / truth[x][0]))

# ---- W from its own entries ------------------------------------------------------------------
lg = lags(T, T.edges(RULE_A, pub, W), ENT)
print("\nreaction (A chances): lag slots p50 %d  p95 %d  max %d, spike end W = %d  (true 1-3)" % (
    np.percentile(lg, 50), np.percentile(lg, 95), lg.max(), reaction_window(lg)))

# ---- the rules measured ----------------------------------------------------------------------
rand_mask = T.buy & (RNG.random(T.n) < len(T.edges(RULE_A, pub, W)) / T.buy.sum())
RULES = [
    ("A  (true rule)", RULE_A),
    ("B  (true rule)", RULE_B),
    ("A or B", RULE_A | RULE_B),
    ("A minus 'new wallet'", T.buy & (g >= 0.4) & (mv >= 0.019) & (T.v < 40) & (T.sol >= 2.0)),
    ("A minus 'quiet'", T.buy & (mv >= 0.019) & T.first_here & (T.v < 40) & (T.sol >= 2.0)),
    ("A with sol >= 1.5 (looser)", E & (T.v < 40) & (T.sol >= 1.5)),
    ("A with sol >= 3.0 (narrower)", E & (T.v < 40) & (T.sol >= 3.0)),
    ("A with v < 45 (looser)", E & (T.v < 45) & (T.sol >= 2.0)),
    ("A with v < 35 (narrower)", E & (T.v < 35) & (T.sol >= 2.0)),
    ("A shifted: v in [40, 50)", E & (T.v >= 40) & (T.v < 50) & (T.sol >= 2.0)),
    ("B with buys >= 15", (nb2 >= 15) & (T.v >= 72) & (T.v < 78)),
    ("B shifted: v in [62, 68)", (nb2 >= 20) & (T.v >= 62) & (T.v < 68)),
    ("random buys", rand_mask),
    ("the wide event E", E),
]


def row(name, C):
    ch = T.edges(C, pub, W)
    raw = measure(T, ch, ENT, P, W)
    full = measure(T, ch, ENT, P, W, cap=CAP, active=ACT)
    b = measure(T, ch, ENT, P, W, cap=CAP, active=ACT, days=BUILD)
    c = measure(T, ch, ENT, P, W, cap=CAP, active=ACT, days=CHECK)
    lift = old_lift(T, C, pub, ENT)[0]
    print("%-30s %8d %6d %7.1f %7.1f %7.1f %7.1f %7.1f %8.2f" % (
        name, full["chances"], full["hits"], 100 * raw["hit_rate"], 100 * full["hit_rate"],
        100 * b["hit_rate"], 100 * c["hit_rate"], 100 * full["cover"], lift))
    return full


print("\n%-30s %8s %6s %7s %7s %7s %7s %7s %8s" % (
    "rule", "chances", "hits", "hit raw", "hit %", "build", "check", "cover%", "old lift"))
res = {n: row(n, C) for n, C in RULES}

# ---- W curve and the shifted-time null -------------------------------------------------------
print("\nhit rate by W (slots):")
for n, C in (RULES[0], RULES[12]):
    ch = T.edges(C, pub, W)
    print("  %-16s" % n[:16], " ".join("W%d %5.1f" % (w, 100 * measure(T, ch, ENT, P, w, cap=CAP,
                                                                     active=ACT)["hit_rate"])
                                        for w in (0, 1, 2, 3, 5, 10, 20, 50)))
ch = T.edges(RULE_A, pub, W)
sh = []
for _ in range(5):
    c = T.coin[ch]
    lo, hi = T.cstart[c - 1], T.cend[c - 1]
    k2 = np.sort(lo + (RNG.random(len(ch)) * (hi - lo)).astype(np.int64))
    sh.append(measure(T, k2, ENT, P, W, cap=CAP, active=ACT)["hit_rate"])
print("A chances moved to random prints on the same coin: hit rate %.1f %% (5 draws, max %.1f %%)" % (
    100 * np.mean(sh), 100 * np.max(sh)))
for nm in ("A  (true rule)", "B  (true rule)"):
    print(nm, "exclusions:", res[nm]["excluded"])
f = res["A  (true rule)"]
print("A exclusions:", f["excluded"])

# ---- saved for the hand count (hr_hand.py) ---------------------------------------------------
from _paths import DATA  # noqa: E402
out = {"e_coin": e_coin, "e_pos": e_pos, "e_slot": e_slot, "act_lo": ACT[0], "act_hi": ACT[1],
       "slot0": T.slot0}
for i, nm in enumerate(("p_coin", "p_in", "p_out", "t_in", "t_out")):
    out[nm] = P[i]
for nm, C in (("A", RULE_A), ("B", RULE_B)):
    m = measure(T, T.edges(C, pub, W), ENT, P, W, cap=CAP, active=ACT)
    out[nm + "_k"], out[nm + "_hit"] = m["k"], m["k_hit"]
np.savez(DATA / "hr_synth_trader.npz", **out)
