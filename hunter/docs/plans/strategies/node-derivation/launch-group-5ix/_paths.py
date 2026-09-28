"""Puts node-derivation (toolkit) and study-kernel (kernel) on sys.path."""
import sys
from pathlib import Path

_ND = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(_ND))
sys.path.insert(0, str(_ND.parent / "study-kernel"))
