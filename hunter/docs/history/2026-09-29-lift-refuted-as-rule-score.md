# Lift refuted as the score of a trader's rule (2026-09-29)

**Symptom.** Every "is this his logic" verdict in the strategy docs - the derive 5.1 trigger scan,
the 6.1 E ladder's acted share against a base, the inventory's "his lift" statuses, the case files'
peak lifts - read a per-moment lift: the share of his buys where a spelling holds over the share
of all moments where it holds. 8dtx2t's frozen E read "4.65 % acted on a 0.70 % base" and a lift of
16.6, which looks strong.

**Cause.** Lift is hit rate divided by his base rate, so it carries no information about how far a
spelling is from his rule. On 8dtx2t's base (about 1.7 % of market moments are his), a lift of 2 is
a hit rate of about 3 %: he skips 97 of every 100 chances. Tested on a fake trader with known rules
(evidence 5.8), lift put both narrower pieces of the true rule above it (557 and 663 against 571)
and a wrong rule at a 12.5 % hit rate above a true rule at 69 % (73 against 34). Counted per
moment, a state rule's market count also inflates 14x against its rising edges.

**Fix.** Hit rate (hits / chances) and cover (hits / his entries), counted on every coin while he is
free, a flicker inside his reaction window as one chance, W read as the end of his reaction spike.
The fake trader's rules come back at 80.4 % against 80.5 % and 69.2 % against 69.1 %; a hand count
agrees on 300 of 300 coins. 8dtx2t's E reads a 0.77 % hit rate at 60.3 % cover. Every lift number
about a trader's logic is out of the docs; a verdict that rested on one alone reads "unscored:
needs a hit-rate read (derive 5.0)". The numbers are in git before this date.

**The rule this produced.** A class or a conjunction is scored by hit rate and cover, never by lift:
[derive 5.0](../plans/strategies/_!___derive.md). Open work: [hit-rate-method.md](../roadmap/hit-rate-method.md).
