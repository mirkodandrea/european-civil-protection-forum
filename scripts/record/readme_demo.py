#!/usr/bin/env python3
"""Regenerate the README's demo timeline and outcome tables from a recording.

Reads docs/media/ttx_demo_captions.json (narration marks), scripts/record/
narration.json (who / what it shows) and docs/media/ttx_demo_log.jsonl (the
run's event log), and rewrites the blocks between
<!-- demo:timeline --> … <!-- /demo:timeline --> and
<!-- demo:outcome --> … <!-- /demo:outcome --> in README.md.

  python3 scripts/record/readme_demo.py
"""
import json
import os
import re

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
media = os.path.join(ROOT, "docs", "media")
marks = json.load(open(os.path.join(media, "ttx_demo_captions.json")))
spec = {l["id"]: l for l in json.load(open(os.path.join(ROOT, "scripts", "record", "narration.json")))["lines"]}
log = [json.loads(l) for l in open(os.path.join(media, "ttx_demo_log.jsonl"))]

rows = ["| Video | Who | What happens (narration) | What it shows |", "|---|---|---|---|"]
for m in marks:
    s = round(m["t"] / 1000)
    l = spec.get(m["id"], {})
    rows.append(f"| {s // 60}:{s % 60:02d} | {l.get('who', '')} | {m['text']} | {l.get('shows', '')} |")
timeline = "\n".join(rows)

turns = [e for e in log if e["kind"] == "turn_resolved"]


def roads_cut(upto):
    cut = []
    for e in turns[: upto + 1]:
        cut += [ev["text"].replace(" is cut by the fire", "") for ev in e["data"]["events"] if ev["kind"] == "road_cut"]
    return ", ".join(cut) or "—"


def arrivals(e):
    a = [ev["text"] for ev in e["data"]["events"] if ev["kind"] == "reinforcement_arrived"]
    return "; ".join(a) or "—"


head = "| | " + " | ".join(f"After turn {e['data']['turn']} ({e['data']['stats']['clock'][-5:]})" for e in turns) + " |"
sep = "|---|" + "---|" * len(turns)
st = [e["data"]["stats"] for e in turns]
lines = [
    head,
    sep,
    "| Burned area | " + " | ".join(f"{s['burned_ha']:.1f} ha" for s in st) + " |",
    "| Fire lines / water drops | " + " | ".join(f"{s['line_km']:.2f} km / {s['drops']}" for s in st) + " |",
    "| People evacuated / sheltered | " + " | ".join(f"{s['evacuated_people']} / {s['sheltered']}" for s in st) + " |",
    "| Buildings threatened | " + " | ".join(str(s["threatened"]) for s in st) + " |",
    "| Buildings damaged / destroyed | " + " | ".join(f"{s['damaged']} / {s['destroyed']}" for s in st) + " |",
    "| People at risk | " + " | ".join(str(s["people_at_risk"]) for s in st) + " |",
    "| Roads cut by fire | " + " | ".join(roads_cut(i) for i in range(len(turns))) + " |",
    "| Reinforcements arrived | " + " | ".join(arrivals(e) for e in turns) + " |",
]
outcome = "\n".join(lines)

path = os.path.join(ROOT, "README.md")
text = open(path).read()
for name, body in (("timeline", timeline), ("outcome", outcome)):
    pat = re.compile(rf"<!-- demo:{name} -->.*?<!-- /demo:{name} -->", re.S)
    if not pat.search(text):
        raise SystemExit(f"README.md has no <!-- demo:{name} --> block")
    text = pat.sub(f"<!-- demo:{name} -->\n{body}\n<!-- /demo:{name} -->", text)
open(path, "w").write(text)
print(f"README demo: {len(marks)} timeline rows, {len(turns)} turns")
