"""Rule 1c: rule 1's entry loosened one term at a time, for more trades a day (evidence 1.26).

The candidates, fills, seat, cost and occupancy are r1b_exit's evaluator (it books rule 1's
reference ticket for ticket with rule 1's exit); both of rule 1's exits are booked on every step:
  bracket  tp +20 %, stop -60 %, clock 90 s          (Flip-Catch - Bracket, rule 1)
  room     0.4 x the room to the wall, -60 %, 90 s   (Flip-Catch - Room, rule 1b)

A step loosens one term of the entry and keeps the others. The trades it ADDS are judged on their
own, so a looser entry never rides on the tickets rule 1 already had.

THE BARS, fixed before any step was booked (study_exact, 0.2 SOL, 115 ms; each exit on its own):
  1. The added trades (tickets of the new book that the old book does not have) net > 0.
  2. Every study day with an added trade is positive on the added trades.
  3. Both halves of those days positive on the added trades.
  4. The added trades' top 1 % at most 15 % of their net.
  5. The added trades net > 0 at the 200 ms seat (both legs) as well.
  6. The new book's SOL beats the old book's (a ticket the looser entry displaces through
     occupancy is counted).
  7. No added trade exits on the graduation print (a price the tape cannot see).
  8. The new book passes every ship bar (r1_exact.bars_fail: every day up, both halves, body,
     top 1 % and the biggest coin <= 15 %) that the old book passes under that exit, and is no
     worse on one the old book misses (the room exit's study top 1 % is 15.5, evidence 1.24).
A step passes when bars 1-8 hold under BOTH exits. A term walks outward from rule 1's value, each
step judged against rule 1, and it stops at the first step that fails or whose own slice (the
step against the previous one) loses SOL under either exit; the term keeps its last passing step.

Combination: the passing terms join best first (net SOL a day, both exits summed); each is judged
by the same bars against the combination without it, walking back to a tighter step when its
own step fails there. The holdout is not read: it chose rule 1b's exit (evidence 1.24), so only
the days after 09-10 certify rule 1c.

  python r1c_loosen.py book         book every candidate under both exits at 115 and 200 ms (cached)
  python r1c_loosen.py walk         step 1: each term alone -> the per-term tables
  python r1c_loosen.py combine      step 2: the passing terms combined -> data/r1c_rule.json
  python r1c_loosen.py grid         every step of every term against rule 1, read only (it
                                    chooses nothing; the walk stops at a term's first failure)
  python r1c_loosen.py slices       each first step's band alone (the loosened band, every other
                                    term at rule 1) as its own rule with its own occupancy, so it
                                    displaces nothing: read only
"""
from __future__ import annotations

import _paths  # noqa: F401
from _paths import data_file

import json
import sys
from dataclasses import replace

import numpy as np
import pandas as pd

import r1_exact as rx
import r1b_exit as rb
from toolkit.book import ledger, mask

TAPE = "study_exact"
EXITS = {"bracket": rb.BASE, "room": rb.RULE_B}
LAGS = (115, 200)
SPEC = dict(rb.SPEC)
# rule 1's value first, then outward; every step sits inside r1_exact.FLOOR
STEPS = {
    "ssize": [1.0, 0.75, 0.5],
    "nb5": [15, 14, 13, 12, 10, 8],
    "stall": [20.0, 30.0, 45.0, 60.0, 90.0],
    "shold": [30.0, 45.0, 60.0, 90.0, 120.0, 180.0, 300.0],
    "age": [158.0, 120.0, 90.0, 60.0],
    "hold_n": [368, 300, 250, 200, 150, 100, 50, 0],
    "vres": [100.0, 104.0, 107.2, 110.0, 116.0],
}


def book_path(ex: str, lag: int):
    return data_file("r1c_book_%s_%s_%d.parquet" % (TAPE, ex, lag))


def book() -> None:
    T = rx.Tape(TAPE)
    C = pd.read_parquet(data_file("r1b_cands_%s.parquet" % TAPE)).sort_values(["run", "k"]).reset_index(drop=True)
    coins = {r: rb.Coin(T, int(r)) for r in np.unique(C.run.to_numpy())}
    for lag in LAGS:
        sem = replace(rb.SEM, lag_ms=lag)
        for ex, spec in EXITS.items():
            D = rb.book(T, C, spec, coins, sem)
            D.to_parquet(book_path(ex, lag), index=False)
            print(ex, lag, "booked", len(D), flush=True)
    ref = pd.read_parquet(data_file("r1_ref_%s.parquet" % TAPE))
    F = rx.occupy(pd.read_parquet(book_path("bracket", 115)), mask(pd.read_parquet(book_path("bracket", 115)), SPEC))
    print("rule 1 through this book", len(F), "tickets; the reference", len(ref),
          "same trigger", len(F.assign(mint=T.mints[F.run.to_numpy()]).merge(ref, on=["mint", "k"])))
    json.dump({"days": T.days}, open(data_file("r1c_book_%s.json" % TAPE), "w"))


class Books:
    def __init__(self):
        self.days = json.load(open(data_file("r1c_book_%s.json" % TAPE)))["days"]
        self.D = {(ex, lag): pd.read_parquet(book_path(ex, lag)) for ex in EXITS for lag in LAGS}

    def fires(self, spec, ex, lag=115):
        D = self.D[(ex, lag)]
        return rx.occupy(D, mask(D, spec))


def keys(F):
    return pd.MultiIndex.from_arrays([F.run.to_numpy(), F.k.to_numpy()])


def added(F1, F0):
    return F1[~keys(F1).isin(keys(F0))]


def book_worse(F1, F0, days) -> str:
    """Bar 8: the ship bars F1 fails that F0 passes, and the tail bars F0 misses that F1 worsens."""
    old = rx.bars_fail(F0, days)
    bad = [b for b in rx.bars_fail(F1, days).split(", ") if b and b.split(" ")[0] not in old]
    L0, L1 = ledger(F0, days), ledger(F1, days)
    bad += ["%s %s > %s" % (k, L1[k], L0[k]) for k in ("top1", "maxcoin") if k in old and L1[k] > L0[k]]
    return ", ".join(bad)


def judge(B: Books, new: dict, old: dict) -> dict:
    """The bars for `new` against `old`, per exit; returns the row and the failures."""
    row = {}; fails = []
    for ex in EXITS:
        F0, F1 = B.fires(old, ex), B.fires(new, ex)
        A = added(F1, F0)
        dsol = float(F1.y.sum() - F0.y.sum())
        a200 = added(B.fires(new, ex, 200), B.fires(old, ex, 200))
        L = ledger(A, B.days) if len(A) else {}
        row.update({
            ex + "_add_nday": round(len(A) / B.days, 1),
            ex + "_add_pct": L.get("pct", np.nan),
            ex + "_add_solday": round(float(A.y.sum()) / B.days, 3),
            ex + "_lost_n": len(F0) - (len(F1) - len(A)),
            ex + "_net_solday": round(dsol / B.days, 3),
            ex + "_add_days": L.get("pos", "0/0"),
            ex + "_add_top1": L.get("top1", np.nan),
            ex + "_add200_sol": round(float(a200.y.sum()), 2),
        })
        if not len(A):
            fails.append(ex + ": adds nothing"); continue
        f = []
        if not A.y.sum() > 0: f.append("added net")
        p, n = L["pos"].split("/")
        if p != n: f.append("added days %s" % L["pos"])
        if not (L["h1"] > 0 and L["h2"] > 0): f.append("added halves")
        if not (L["top1"] <= 15): f.append("added top1 %s" % L["top1"])
        if not a200.y.sum() > 0: f.append("added at 200 ms")
        if not dsol > 0: f.append("net")
        if A.grad.any(): f.append("graduation %d" % int(A.grad.sum()))
        why = book_worse(F1, F0, B.days)
        if why: f.append("book " + why)
        if f:
            fails.append(ex + ": " + ", ".join(f))
    row["fails"] = "; ".join(fails)
    return row


def walk_term(B: Books, base: dict, term: str, steps: list, out: list | None = None):
    """Walk one term outward from its value in `base`; returns the last passing step (or None)."""
    op = base[term][0]
    kept = None; prev = base
    for val in steps[1:]:
        new = dict(base, **{term: (op, val)})
        r = judge(B, new, base)
        sl = {ex: float(B.fires(new, ex).y.sum() - B.fires(prev, ex).y.sum()) for ex in EXITS}
        if not r["fails"] and min(sl.values()) <= 0:
            r["fails"] = "slice loses: " + ", ".join("%s %+.2f" % kv for kv in sl.items())
        r = {"term": term, "step": val, **r, "slice_sol": " / ".join("%+.2f" % v for v in sl.values())}
        if out is not None:
            out.append(r)
        if r["fails"]:
            break
        kept = val; prev = new
    return kept


def walk() -> None:
    pd.set_option("display.width", 400); pd.set_option("display.max_columns", 40)
    pd.set_option("display.max_colwidth", 200)
    B = Books()
    for ex in EXITS:
        print("rule 1,", ex, ledger(B.fires(SPEC, ex), B.days))
    rows = []; kept = {}
    for term, steps in STEPS.items():
        kept[term] = walk_term(B, SPEC, term, steps, rows)
    W = pd.DataFrame(rows)
    print(W.to_string(index=False))
    W.to_csv(data_file("r1c_walk_%s.csv" % TAPE), index=False)
    print("kept", kept)
    json.dump({k: v for k, v in kept.items() if v is not None}, open(data_file("r1c_walk_%s.json" % TAPE), "w"))


def grid() -> None:
    pd.set_option("display.width", 400); pd.set_option("display.max_columns", 40)
    B = Books()
    base = {ex: B.fires(SPEC, ex) for ex in EXITS}
    rows = []
    for term, steps in STEPS.items():
        for val in steps[1:]:
            new = dict(SPEC, **{term: (SPEC[term][0], val)})
            r = {"term": term, "step": val}
            for ex in EXITS:
                F1 = B.fires(new, ex); A = added(F1, base[ex])
                L = ledger(A, B.days) if len(A) else {}
                r.update({ex + "_add_nday": round(len(A) / B.days, 1), ex + "_add_pct": L.get("pct", np.nan),
                          ex + "_add_days": L.get("pos", "0/0"),
                          ex + "_lost_nday": round((len(base[ex]) - (len(F1) - len(A))) / B.days, 1),
                          ex + "_net_solday": round(float(F1.y.sum() - base[ex].y.sum()) / B.days, 3),
                          ex + "_grad": int(A.grad.sum()) if len(A) else 0})
            rows.append(r)
    G = pd.DataFrame(rows)
    print(G.to_string(index=False))
    G.to_csv(data_file("r1c_grid_%s.csv" % TAPE), index=False)


def slices() -> None:
    B = Books()
    for term, steps in STEPS.items():
        op, val = SPEC[term]; step = steps[1]
        for ex in EXITS:
            D = B.D[(ex, 115)]
            x = D[term].to_numpy()
            band = (x < val) & (x >= step) if op == ">=" else (x > val) & (x <= step)
            others = mask(D, {k: v for k, v in SPEC.items() if k != term})
            F = rx.occupy(D, others & band)
            L = ledger(F, B.days) if len(F) else {}
            # a band ticket whose hold overlaps a rule 1 position on the same coin: two positions
            R = B.fires(SPEC, ex)
            iv = {}
            for r_, a_, b_ in zip(R.run.to_numpy(), R.t_us.to_numpy(), R.t_x.to_numpy()):
                iv.setdefault(r_, []).append((a_, b_))
            ov = sum(any(a_ < tx and t < b_ for a_, b_ in iv.get(r_, ()))
                     for r_, t, tx in zip(F.run.to_numpy(), F.t_us.to_numpy(), F.t_x.to_numpy()))
            print("%-6s %-7s band %s..%s  %s grad %d overlap %.0f %%" % (term, ex, val, step,
                  {k: L.get(k) for k in ("nday", "pct", "solday", "pos", "top1", "maxcoin", "body",
                                         "cap_sol", "h1", "h2")},
                  int(F.grad.sum()) if len(F) else 0, 100 * ov / max(len(F), 1)))


def combine() -> None:
    pd.set_option("display.width", 400); pd.set_option("display.max_columns", 40)
    pd.set_option("display.max_colwidth", 200)
    B = Books()
    kept = json.load(open(data_file("r1c_walk_%s.json" % TAPE)))
    gain = {}
    for term, val in kept.items():
        new = dict(SPEC, **{term: (SPEC[term][0], val)})
        gain[term] = sum(float(B.fires(new, ex).y.sum() - B.fires(SPEC, ex).y.sum()) for ex in EXITS)
    order = sorted(gain, key=gain.get, reverse=True)
    print("order", {t: round(gain[t] / B.days, 3) for t in order})
    spec = dict(SPEC); rows = []
    for term in order:
        steps = STEPS[term]
        tries = steps[1:steps.index(kept[term]) + 1][::-1]   # the walk's step, then tighter ones
        for val in tries:
            new = dict(spec, **{term: (spec[term][0], val)})
            r = judge(B, new, spec)
            rows.append({"term": term, "step": val, **r})
            if not r["fails"]:
                spec = new
                break
    print(pd.DataFrame(rows).to_string(index=False))
    print("\n=== rule 1c entry", {k: v[1] for k, v in spec.items()})
    for ex in EXITS:
        for lag in LAGS:
            F = B.fires(spec, ex, lag)
            lo, hi, _ = rx.bootstrap(F)
            print("  %-7s %d ms rule 1c %s ci %+.2f..%+.2f" % (ex, lag, ledger(F, B.days), lo, hi))
            print("  %-7s %d ms rule 1  %s" % (ex, lag, ledger(B.fires(SPEC, ex, lag), B.days)))
    data_file("r1c_rule.json").write_text(json.dumps(
        {"entry": {k: list(v) for k, v in spec.items()}, "exits": EXITS, "tape": TAPE}))


if __name__ == "__main__":
    {"book": book, "walk": walk, "combine": combine, "grid": grid, "slices": slices}[sys.argv[1]](*sys.argv[2:])
