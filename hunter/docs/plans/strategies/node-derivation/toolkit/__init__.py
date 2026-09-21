"""The node-derivation toolkit: the method of ../../_!___derive.md as functions, for any wallet.

Importing the package puts the shared study kernel (../../study-kernel: kernel.py, tape.py,
cvx.py) on sys.path, so every module prices through the one kernel.

  paths        where the tapes, the lake, the roster and this folder's data live
  tapes        load a tape (study or holdout) with its instrument wallets marked
  facts        per-coin fact arrays at every print (windows, holders, the seller's history)
  portrait     step 2: the trader as a person - his shape, and the 20 trades to read print by print
  seat         a member's episodes at the RACE and FOLLOW seats; our landing vs theirs; 5.2's
               leftover with its random-print control; 5.4's still-vs-answered race split
  own_exit     step 3: his OWN exit, read from his own sells - the starting X of every later book
  trigger      excess intensity: which print class a wallet reacts to, and at what lag
  contrast     within-coin stratified rank: the prints it acts on against those it ignores
  exits        the exit families; every branch resolves to a print index
  hazard       an actor's closing hazard by profit x time held, on a selected pool
  candidates   one table per tape: every trigger print, its facts and its exit outcome
  book         a variant = a mask + occupancy; the ledger every result is judged by
  walkforward  choosing a cut on half the days and scoring it on the other half
  graduation   the exits that land on the curve's completing print
  lake_export  sealed lake days converted to the study tape's format (a holdout tape)
"""
from __future__ import annotations

import sys

from .paths import SHARED

if str(SHARED) not in sys.path:
    sys.path.insert(0, str(SHARED))
