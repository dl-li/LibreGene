#!/usr/bin/env python3
"""Fix `methylation` flags in comm_only_enzymes.json using supplier data.

Rule (supplier table wins, CpG ignored — the engine does not model CpG):
- any provider reporting dam/dcm/EcoKI(EK)  -> "sensitive"
- else any explicit informative value       -> "none"
- else (all empty / Not Determined)         -> leave unchanged
Enzymes flagged methylation_dependent (DpnI) are never touched.
"""
import json, re

DB = "backend/libregene-core/data/comm_only_enzymes.json"
PROV = "backend/libregene-core/data/enzyme_providers.json"

UNINFORMATIVE = {"", "not determined", "nd"}


def parse_meth(s):
    low = s.lower()
    f = set()
    if "dam" in low:
        f.add("dam")
    if "dcm" in low:
        f.add("dcm")
    if re.search(r"\bek\b", low):
        f.add("ecoki")
    return f


db = json.load(open(DB))
prov = json.load(open(PROV))["enzymes"]

to_sensitive, to_none, skipped = [], [], []
for rec in db:
    if rec.get("methylation_dependent"):
        continue
    entry = prov.get(rec["name"])
    if not entry:
        continue
    vals = [p.get("methylation", "") for p in entry["providers"].values()]
    systems = set().union(*(parse_meth(v) for v in vals)) if vals else set()
    cur = rec.get("methylation", "none")
    if systems:
        new = "sensitive"
    elif any(v.strip().lower() not in UNINFORMATIVE for v in vals):
        new = "none"
    else:
        if cur != "none":
            skipped.append(rec["name"])
        continue
    if new != cur:
        (to_sensitive if new == "sensitive" else to_none).append(rec["name"])
        rec["methylation"] = new

with open(DB, "w") as f:
    json.dump(db, f, ensure_ascii=False, indent=2)
    f.write("\n")

print(f"-> sensitive ({len(to_sensitive)}):", ", ".join(sorted(to_sensitive)))
print(f"-> none ({len(to_none)}):", ", ".join(sorted(to_none)))
print(f"left as-is (ND/empty, still sensitive) ({len(skipped)}):", ", ".join(sorted(skipped)))
