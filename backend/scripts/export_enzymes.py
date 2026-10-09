#!/usr/bin/env python3
"""Export common restriction enzymes from BioPython as JSON, with full classification."""
import json, sys

from Bio.Restriction import CommOnly

def classify_cut_type(enz):
    """Determine cut type from BioPython's overhang classification."""
    if enz.is_blunt():
        return "blunt"
    if enz.is_5overhang():
        return "5overhang"
    if enz.is_3overhang():
        return "3overhang"
    return "unknown"

def classify_type_iis(enz, rec_len):
    """A Type IIS restriction enzyme cuts outside its recognition sequence.

    This means either fst5 is outside [0, rec_len) or fst3 causes the bottom
    cut to fall outside, or both.
    """
    fst5 = enz.fst5
    fst3 = enz.fst3
    # If either cut falls outside the recognition bounds, it's Type IIS-like
    if fst5 is None and fst3 is None:
        return False
    # Top cut outside rec: fst5 < 0 or fst5 >= rec_len
    # Bottom cut outside rec: fst3 < -rec_len or fst3 > 0
    # (for palindromic enzymes, fst3 is typically negative and within [-rec_len, 0))
    top_outside = fst5 < 0 or fst5 >= rec_len
    bot_outside = fst3 < -rec_len or fst3 > 0
    if top_outside or bot_outside:
        return True
    return False

# Manual corrections to Biopython/REBASE data, cross-checked against NEB/REBASE
# canonical cut notations:
# - AbaSI: REBASE simplifies the site to "C", but NEB (R0665) gives
#   CNNNNNNNNNNN/NNNNNNNNNG — a 22 bp window (C + 20N + G, the G being the
#   second modified C on the opposite strand). Cut geometry 11/9 with a 2-nt
#   3' overhang is unchanged.
# - MlyI/SchI: Bio.Restriction.elucidate() truncates the "^" marker for
#   remote cutters; GAGTC(5/5) blunt per NEB.
# - TspRI/TscAI: Biopython reports ovhg=10, but the overhang sequence
#   NNCASTGNN is 9 nt (9-nt 3' overhang per REBASE/literature).
OVERRIDES = {
    "AbaSI": {
        "site": "CNNNNNNNNNNNNNNNNNNNNG",
        "fst5": 12,
        "fst3": -12,
        "is_palindromic": True,
        "elucidate": "CNNNNNNNNN_NN^NNNNNNNNNG",
    },
    "MlyI": {"elucidate": "GAGTCNNNNN^_"},
    "SchI": {"elucidate": "GAGTCNNNNN^_"},
    "TspRI": {"overhang_len": 9},
    "TscAI": {"overhang_len": 9},
    # FspEI is correct (CC(12/16)) but its 2-bp site is too dense to display.
    "FspEI": {"hidden": True},
}

results = []
for enz in CommOnly:
    name = str(enz)
    site = str(enz.site)
    rec_len = len(site)
    fst5 = enz.fst5
    fst3 = enz.fst3
    scd5 = enz.scd5
    scd3 = enz.scd3

    # Determine methylation status from MRO
    # Meth_Dep  → methylation-sensitive (blocked by methylation)
    # Meth_Undep → undepleted = NOT blocked (includes unaffected and dependent)
    # Separate "dependent" (needs methylation) is only detectable for enzymes
    # whose recognition site IS a known methylation target (e.g., DpnI).
    mro_names = [c.__name__ for c in type(enz).__mro__]
    is_meth_dep = "Meth_Dep" in mro_names

    # Methylation-dependent (requires methylation to cut): known by name.
    # DpnI only cuts Dam-methylated GATC; the PvuRts1I-family enzymes
    # (AbaSI, FspEI, MspJI, SgeI, LpnPI) only cut at modified cytosines.
    is_meth_dependent = name in ("DpnI", "AbaSI", "FspEI", "MspJI", "SgeI", "LpnPI")

    entry = {
        "name": name,
        "site": site,
        "fst5": fst5,
        "fst3": fst3,
        "scd5": scd5,
        "scd3": scd3,
        "is_palindromic": enz.is_palindromic(),
        "cut_type": classify_cut_type(enz),
        "overhang_len": enz.ovhg,
        "is_cut_twice": enz.cut_twice(),
        "methylation": "sensitive" if is_meth_dep else "none",
        "methylation_dependent": is_meth_dependent,
        "elucidate": enz.elucidate(),
    }
    entry.update(OVERRIDES.get(name, {}))
    results.append(entry)

# Print stats
cut_types = {}
for r in results:
    ct = r["cut_type"]
    cut_types[ct] = cut_types.get(ct, 0) + 1
print(f"Total enzymes exported: {len(results)}", file=sys.stderr)
for ct, cnt in sorted(cut_types.items()):
    print(f"  {ct}: {cnt}", file=sys.stderr)

json.dump(results, sys.stdout, indent=2)
print("Wrote enzyme JSON to stdout — redirect to libregene-core/data/comm_only_enzymes.json (run from backend/)", file=sys.stderr)
