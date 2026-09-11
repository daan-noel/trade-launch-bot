"""Rule 1 update, step G1: rule 1 as a spec over the candidate tables (cvx_r1u_cand.py), and its loaders.

The booking engine is the toolkit's (toolkit/book.py): a variant is a set of one-sided cuts, the
mask is applied BEFORE occupancy, then one position per coin at a time with re-entry after the
exit fill, exactly as cvx_hot_perm2.run().
"""
from __future__ import annotations

import _paths  # noqa: F401  - the toolkit and study-kernel on sys.path

from toolkit import book
from toolkit.book import capped, fires, ledger, mask, occupy, reprice  # noqa: F401
from kernel import B_DEFAULT as B  # noqa: F401

PREFIX = "cvx_r1u_cand"

# rule 1 before the update (hot-tape-rule-1.md, evidence 1.17): (column, ">=" or "<=", value)
RULE1 = {
    "ssize": (">=", 1.0), "nb5": (">=", 15), "buys2": (">=", 2.0), "stall": ("<=", 20.0),
    "shold": ("<=", 30.0), "age": (">=", 158.0), "hold_n": (">=", 368),
}
# rule 1 after the update (evidence 1.20): the 2 s buy term dropped, the reserve cap added
RULE1_UPDATED = dict(RULE1, buys2=(">=", 0.0), vres=("<=", 100.0))


def cand(name):
    return book.load(PREFIX, name)
