# 2026-09-10 - C8 +47.54 is keep+ep50 lookahead, not documented-project

## Symptom

Campaign C8 ranks `documented x token-silence x creator x trail40` at **+47.54 SOL**,
1,622 trades, 7/7 days. The sentence spells a public metadata door and a 10-slot gap.
Kernel, seat, cost and occupancy all reproduce.

## Cause

Two silent filters, then rank-by-SOL.

**The sidecar is a different study's universe.** `cvx_meta_export.py` fetches URIs for
`keep & ep50 >= 1` only (18,583 mints). `cvx_conj4.py` does
`cvx_meta.reindex(T.mints)` and `ok.fillna(0) == 1`. Tokens never in that file cannot
pass D. Every one of the 1,020 "documented" tokens completes a >= 50 % episode later
in the week. Honesty law 15 (universe SQL hides a term) already forbids this; the join
is the SQL.

**The event has a vacuous base case.** `gap_tok = 10**6 if i == 0`, so the first print
of the token always counts as ">= 10 empty slots." 818 / 1,622 occupancy trades are
`k == 0` and book +46.08 of the +47.54.

Funnel coverage (sidecar rows vs tape tokens) is not printed. Missing meta reads as a
selective door.

## Fix

Do not implement. Same kernel, legal D:

| door | n | SOL |
| --- | ---: | ---: |
| spelled documented | 1,622 | +47.54 |
| keep-create, age < 60 s | 44,548 | -1,007.34 |

Recorded as strategy 7.4 laws 30 and 31, workflow section 11, evidence 6.13.

## The rule this produced

A sidecar left-join with missing = False is a universe filter; an event that is true at
`i == 0` by construction is not the named event.
`[_!___strategy.md](../plans/strategies/_!___strategy.md)` 7.4.
