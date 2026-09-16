"""Choosing a cut on half the days and scoring it on the other half.

THE KEEP RULE. The days of the base's fires split in two halves; each half (fit) picks the value
with the most SOL among those whose every fit day is positive, and the other half (test) scores
it against the current value there. A change is TAKEN only when both folds move the same way AND
both beat the current value on their test half; the value is the mean of the two picks, snapped
to the grid (a tie to the side nearer the current value). Then, outside this module:
  * the ledger's bars must still hold (top 1 % <= 15 %, biggest coin <= 15 %, days positive) -
    a gain that only moves SOL into upside gaps fails on the capped book;
  * a gain smaller than cut_noise() on a test half is chance, not a finding;
  * the change is booked ONCE on the holdout, one change at a time.

  folds(F)                                  the two (fit, test) halves of F's days
  thresholds(C, days, base, grid)           one term moved at a time, the rest at base
  converge(C, days, base, grid, rounds)     thresholds() until a round takes nothing
  new_terms(C, days, base, cols, qs)        a NEW one-sided cut, from above and below
  axes(books, grids, start, names)          an exit, one axis at a time; books[(a, b, c)] = fires
  cut_noise(F, frac)                        SD of the SOL a random `frac` of the tickets carries,
                                            per half: the size a real cut must beat

THE LADDER (derive 10) builds one conjunction; E runs it on the acted label, D / P / X on money.
  ladder(fam, step, value, root, beam, depth)   beam search on the fit rows: the best sentence
                                            at each depth, one term per family
  walk_ladder(fam, key, make, lab, run, halves, draws)
                                            the ladder on each half, scored on the other, against
                                            the same ladder on `lab` shuffled across whole coins
  coin_shuffle(run, v, rng)                 each coin takes the values of another coin of about
                                            its length (the coin clusters survive the shuffle)
"""
from __future__ import annotations

import numpy as np
import pandas as pd

from .book import fires, ledger, mask, occupy

QS = [0.05, 0.10, 0.20, 0.30, 0.40, 0.50]


def folds(F):
    dl = np.sort(F.day.unique()); h = (len(dl) + 1) // 2
    return [(dl[:h], dl[h:]), (dl[h:], dl[:h])]


def solsum(F, days_set):
    return float(F[F.day.isin(days_set)].y.sum())


def allpos(F, days_set):
    g = F[F.day.isin(days_set)].groupby("day").y.sum().reindex(days_set, fill_value=0.0)
    return bool((g > 0).all())


def _snap(grid, picks, cur):
    m = float(np.mean(picks))
    return min(grid, key=lambda v: (abs(v - m), abs(v - cur)))


def thresholds(C, days, base, grid):
    Fb = fires(C, base)
    fl = folds(Fb)
    out = []; curves = []
    for col, g in grid.items():
        if col not in base:
            continue
        op, cur = base[col]
        res = {}
        for val in g:
            F = fires(C, dict(base, **{col: (op, val)}))
            res[val] = F
            L = ledger(F, days)
            curves.append(dict(term=col, cut="%s %g" % (op, val), **{k: L[k] for k in (
                "nday", "pct", "sol", "solday", "pos", "top1", "maxcoin", "sl", "cap_sol")}))
        picks = []
        for fit, test in fl:
            ok = [v for v in g if allpos(res[v], fit)]
            best = max(ok, key=lambda v: solsum(res[v], fit)) if ok else cur
            picks.append((best, solsum(res[best], test), solsum(Fb, test)))
        same = all(p[0] > cur for p in picks) or all(p[0] < cur for p in picks)
        beats = all(p[1] > p[2] for p in picks)
        take = bool(same and beats)
        out.append(dict(term=col, now="%s %g" % (op, cur),
                        fold1="%g -> test %+.2f vs %+.2f" % picks[0],
                        fold2="%g -> test %+.2f vs %+.2f" % picks[1],
                        take=take, value=_snap(g, [p[0] for p in picks], cur) if take else cur))
    return pd.DataFrame(out), pd.DataFrame(curves)


def converge(C, days, base, grid, rounds=4, veto=None):
    """thresholds() round after round from `base` until nothing is taken. `veto(term, value, F)`
    may refuse a taken change (e.g. the ledger's bars); refused changes are logged."""
    base = dict(base); log = []
    for rnd in range(1, rounds + 1):
        W, _ = thresholds(C, days, base, grid)
        changed = False
        for _, row in W[W["take"]].iterrows():
            new = dict(base, **{row.term: (base[row.term][0], row.value)})
            if veto is not None and veto(row.term, row.value, fires(C, new)):
                log.append((rnd, row.term, row.value, "vetoed"))
                continue
            base = new; changed = True
            log.append((rnd, row.term, row.value, "taken"))
        if not changed:
            break
    return base, pd.DataFrame(log, columns=["round", "term", "value", "outcome"])


def new_terms(C, days, base, cols, qs=QS):
    m0 = mask(C, base)
    Fb = occupy(C, m0)
    fl = folds(Fb)
    out = []; curves = []
    for col in cols:
        x = C[col].to_numpy()
        vals = Fb[col].dropna()
        cuts = [(">=", float(vals.quantile(q))) for q in qs] + \
               [("<=", float(vals.quantile(1 - q))) for q in qs]
        res = {}
        for op, val in cuts:
            m = m0 & ((x >= val) if op == ">=" else (x <= val))
            F = occupy(C, m)
            res[(op, val)] = F
            L = ledger(F, days)
            curves.append(dict(term=col, cut="%s %.3g" % (op, val), **{k: L[k] for k in (
                "nday", "pct", "sol", "solday", "pos", "top1", "cap_sol", "sl")}))
        picks = []
        for fit, test in fl:
            best, bs = None, solsum(Fb, fit)
            for c_, F in res.items():
                if allpos(F, fit) and solsum(F, fit) > bs:
                    best, bs = c_, solsum(F, fit)
            picks.append((best, solsum(res[best], test) if best else solsum(Fb, test),
                          solsum(Fb, test)))
        ops = {p[0][0] if p[0] else None for p in picks}
        take = None not in ops and len(ops) == 1 and all(p[1] > p[2] for p in picks)
        fmt = lambda p: "%s -> test %+.2f vs %+.2f" % (
            ("%s %.3g" % p[0]) if p[0] else "none", p[1], p[2])
        out.append(dict(term=col, fold1=fmt(picks[0]), fold2=fmt(picks[1]), take=take,
                        gain1=round(picks[0][1] - picks[0][2], 2),
                        gain2=round(picks[1][1] - picks[1][2], 2)))
    return pd.DataFrame(out), pd.DataFrame(curves)


def axes(books, grids, start, names, rounds=3):
    """books[(v0, v1, ...)] = occupied fires of that exit; one axis at a time from `start`."""
    any_F = next(iter(books.values()))
    fl = folds(any_F)
    S = {key: [solsum(F, fl[0][0]), solsum(F, fl[0][1])] for key, F in books.items()}
    POS = {key: [allpos(F, fl[0][0]), allpos(F, fl[0][1])] for key, F in books.items()}
    cur = list(start); log = []
    for rnd in range(1, rounds + 1):
        changed = False
        for ai, g in enumerate(grids):
            picks = []
            for fit, test in ((0, 1), (1, 0)):
                best, bs = None, -1e9
                for val in g:
                    c = list(cur); c[ai] = val; key = tuple(c)
                    if POS[key][fit] and S[key][fit] > bs:
                        best, bs = val, S[key][fit]
                if best is None:            # no value has every fit day positive: keep
                    best = cur[ai]
                c = list(cur); c[ai] = best
                picks.append((best, S[tuple(c)][test], S[tuple(cur)][test]))
            same = all(p[0] > cur[ai] for p in picks) or all(p[0] < cur[ai] for p in picks)
            beats = all(p[1] > p[2] for p in picks)
            val = _snap(g, [p[0] for p in picks], cur[ai]) if same and beats else cur[ai]
            log.append(dict(round=rnd, axis=names[ai], now=cur[ai],
                            fold1="%g -> %+.2f vs %+.2f" % picks[0],
                            fold2="%g -> %+.2f vs %+.2f" % picks[1],
                            result=("take %g" % val) if val != cur[ai] else "keep"))
            if val != cur[ai]:
                cur[ai] = val; changed = True
        if not changed:
            break
    return tuple(cur), pd.DataFrame(log)


def cut_noise(F, frac=0.05, draws=2000, seed=20260911):
    """Per half of the days: the SD of the SOL carried by a random `frac` of its tickets. A cut
    that removes that share of tickets moves the test half by about this much by chance."""
    rng = np.random.default_rng(seed)
    out = []
    for _, test in folds(F):
        y = F[F.day.isin(test)].y.to_numpy()
        m = max(1, int(round(frac * len(y))))
        sums = np.array([y[rng.choice(len(y), m, replace=False)].sum() for _ in range(draws)])
        out.append(round(float(sums.std()), 3))
    return out


def ladder(fam, step, value, root, beam=5, depth=15):
    """Beam search for one conjunction. Candidate c has family fam[c] (one term per family).
    step(state, c) -> the state with c added; value(state) -> its score on the fit rows, None when
    a floor breaks. Keeps the `beam` best sentences a depth; returns the best (terms, value) at
    each depth, depth 0 = the root."""
    B = [((), root)]
    path = [((), value(root))]
    for _ in range(depth):
        seen = set(); nxt = []
        for bi, (terms, st) in enumerate(B):
            used = {fam[c] for c in terms}
            for c in range(len(fam)):
                if fam[c] in used:
                    continue
                key = tuple(sorted(terms + (c,)))
                if key in seen:
                    continue
                seen.add(key)
                v = value(step(st, c))
                if v is not None:
                    nxt.append((v, key, bi, c))
        if not nxt:
            break
        nxt.sort(key=lambda r: -r[0])
        B = [(key, step(B[bi][1], c)) for _, key, bi, c in nxt[:beam]]
        path.append((nxt[0][1], nxt[0][0]))
    return path


def coin_shuffle(run, v, rng, chunk=10):
    """Coins sorted by row count, cut in chunks of `chunk`; inside a chunk each coin takes another
    coin's values in their order, cycled or cut to its own length. Rows sorted by (run, k), on
    v's last axis (a matrix of outcomes, one row per exit, moves with one coin mapping)."""
    u, start, cnt = np.unique(run, return_index=True, return_counts=True)
    order = np.argsort(cnt, kind="stable")
    donor = order.copy()
    for a in range(0, len(order), chunk):
        donor[a:a + chunk] = rng.permutation(order[a:a + chunk])
    idx = np.empty(len(run), dtype=np.int64)
    for r, d in zip(order, donor):
        idx[start[r]:start[r] + cnt[r]] = np.resize(np.arange(start[d], start[d] + cnt[d]), cnt[r])
    return v[..., idx]


def walk_ladder(fam, key, make, lab, run, halves, draws=20, beam=5, depth=15, seed=20260915):
    """The ladder built on each half of the days and scored on the other. make(lab, rows) ->
    (step, value, root) on those rows, value(state, floor=True). A depth counts in a fold when its
    best sentence's test gain over the root beats the 95th percentile of the same ladder's gain
    at that depth on `lab` shuffled across whole coins (`draws` times). The best sentence at a
    depth need not hold the previous one's terms (the beam: a term may wait for its partner), so
    a depth that fails does not stop the ladder. The finished sentence is the deepest depth that
    counts in both folds with both holding the same (family, side): one idea read at two grains
    (derive 10 families) is one term. `step` (the lift over the previous depth) is a read.
    key[c] = (fact, side, value). Returns (per-fold path tables, that depth, both folds' terms
    there); depth 0 is no sentence."""
    rng = np.random.default_rng(seed)

    def one(lb):
        res = []
        for fit, test in ((halves[0], halves[1]), (halves[1], halves[0])):
            st_f, val_f, root_f = make(lb, fit)
            p = ladder(fam, st_f, val_f, root_f, beam, depth)
            st_t, val_t, root_t = make(lb, test)
            tv = []
            for terms, _ in p:
                s = root_t
                for c in terms:
                    s = st_t(s, c)
                tv.append(val_t(s, floor=False))
            res.append((p, np.array(tv, dtype=float)))
        return res

    real = one(lab)
    null = [[] for _ in range(2)]
    for _ in range(draws):
        for f, (p, tv) in enumerate(one(coin_shuffle(run, lab, rng))):
            null[f].append(tv[1:] - tv[0])
    tabs = []; ok = []
    for f, (p, tv) in enumerate(real):
        gain = tv[1:] - tv[0]
        q = [np.nanquantile([n[j] for n in null[f] if len(n) > j], 0.95)
             if any(len(n) > j for n in null[f]) else 0.0 for j in range(len(gain))]
        rows = [dict(depth=0, terms="(root)", fit=round(p[0][1], 4), test=round(tv[0], 4))]
        for j in range(len(gain)):
            rows.append(dict(depth=j + 1, terms=" & ".join("%s %s %.4g" % key[c] for c in p[j + 1][0]),
                             fit=round(p[j + 1][1], 4), test=round(tv[j + 1], 4),
                             step=round(tv[j + 1] - tv[j], 4), gain=round(gain[j], 4),
                             chance95=round(q[j], 4), beats=bool(gain[j] > q[j])))
        tabs.append(pd.DataFrame(rows)); ok.append([r["beats"] for r in rows[1:]])
    kset = [[{(fam[c], key[c][1]) for c in p[j][0]} for j in range(len(p))] for p, _ in real]
    d = max([j for j in range(1, min(len(ok[0]), len(ok[1])) + 1)
             if ok[0][j - 1] and ok[1][j - 1] and kset[0][j] == kset[1][j]], default=0)
    return tabs, d, [real[0][0][d][0], real[1][0][d][0]]
