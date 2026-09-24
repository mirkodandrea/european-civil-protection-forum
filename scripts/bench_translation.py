#!/usr/bin/env python3
"""Benchmark translation models on OpenRouter for the PPRD East languages.

  python3 scripts/bench_translation.py [--models a,b,c] [--langs hy,ka,...] [--judge MODEL]

For each model and language: translate a fixed set of exercise phrases from
English, check that zone codes, numbers and place names survive, and let a
judge model (a different, stronger model) score adequacy and fluency 1-5.
Writes runs/bench_translation.json and prints a markdown table.
Reads OPENROUTER_API_KEY from .env. Costs a few cents.
"""
import argparse, json, os, re, statistics, sys, time, urllib.request
from concurrent.futures import ThreadPoolExecutor

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
for line in open(os.path.join(ROOT, ".env")):
    if "=" in line and not line.startswith("#"):
        k, v = line.strip().split("=", 1)
        os.environ.setdefault(k, v.strip("\"'"))
KEY = os.environ["OPENROUTER_API_KEY"]
BASE = os.environ.get("OPENROUTER_BASE_URL", "https://openrouter.ai/api/v1")

LANGS = {"hy": "Armenian", "ka": "Georgian", "az": "Azerbaijani", "ro": "Romanian (Moldova)", "uk": "Ukrainian", "ru": "Russian", "it": "Italian"}
MODELS = ["tencent/hy-mt2-30b-a3b", "tencent/hy-mt2-7b", "google/gemma-4-31b-it", "google/gemini-3.1-flash-lite",
          "qwen/qwen3.8-flash", "mistralai/mistral-small-2603", "openai/gpt-5.6-luna", "deepseek/deepseek-v4.1-flash"]
JUDGE = "anthropic/claude-sonnet-5"

# (text, tokens that must survive verbatim)
PHRASES = [
    ("Warn zone F4 and evacuate the Pineta Primary School to the Valverde Sports Centre.", ["F4", "Pineta", "Valverde"]),
    ("Close the Northern Road to civilian traffic.", []),
    ("Two crews cut a fire line along F4:N; the third crew holds the east edge.", ["F4:N"]),
    ("120 pupils and staff are still inside the school, and the town has only 2 buses.", ["120", "2"]),
    ("The wind has increased to 55 km/h and a second fire has started in the farmland south-east of Pineta.", ["55", "Pineta"]),
    ("Request two water-bombing aircraft through the EU Civil Protection Mechanism, arrival in 120 minutes.", ["120"]),
    ("Which crews do you mean, and where exactly should they go?", []),
    ("Power is down at the hospital. Do you have a generator available?", []),
    ("Urgent: fire at the fuel station, evacuate 300 people now.", ["300"]),
    ("Situation report at 14:30: the fire lines hold north of Pineta; all children are safe.", ["14:30", "Pineta"]),
]


def chat(model, messages, max_tokens=1500, retries=2):
    body = json.dumps({"model": model, "messages": messages, "temperature": 0, "max_tokens": max_tokens}).encode()
    for a in range(retries + 1):
        t0 = time.time()
        try:
            req = urllib.request.Request(BASE + "/chat/completions", body, {"Authorization": "Bearer " + KEY, "Content-Type": "application/json"})
            r = json.load(urllib.request.urlopen(req, timeout=90))
            return (r["choices"][0]["message"].get("content") or "").strip(), time.time() - t0
        except Exception as e:
            err = str(e)
            time.sleep(1 + a)
    return "ERROR: " + err, 0.0


def translate(model, text, lang):
    prompt = (f"Translate the following emergency-exercise message from English into {LANGS[lang]}. "
              "Keep zone codes (like F4, F4:N), place names, numbers and times exactly as written. Output only the translation.\n\n" + text)
    return chat(model, [{"role": "user", "content": prompt}])


def judge(judge_model, lang, pairs):
    listing = "\n".join(f"{i+1}. EN: {s}\n   {LANGS[lang]}: {t}" for i, (s, t) in enumerate(pairs))
    prompt = (f"You are a professional EN->{LANGS[lang]} translator reviewing translations of civil-protection exercise messages. "
              "For each numbered pair score ADEQUACY (meaning preserved, nothing added or lost) and FLUENCY (natural, correct grammar and script) from 1 to 5. "
              'Reply with JSON only: {"scores":[{"n":1,"adequacy":5,"fluency":4}, ...]}\n\n' + listing)
    out, _ = chat(judge_model, [{"role": "user", "content": prompt}], max_tokens=1500)
    m = re.search(r"\{.*\}", out, re.S)
    try:
        return json.loads(m.group(0))["scores"]
    except Exception:
        return []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--models", default=",".join(MODELS))
    ap.add_argument("--langs", default=",".join(LANGS))
    ap.add_argument("--judge", default=JUDGE)
    a = ap.parse_args()
    models, langs = a.models.split(","), a.langs.split(",")
    jobs = [(m, l, i) for m in models for l in langs for i in range(len(PHRASES))]
    with ThreadPoolExecutor(8) as ex:
        res = list(ex.map(lambda j: translate(j[0], PHRASES[j[2]][0], j[1]), jobs))
    tr = {j: r for j, r in zip(jobs, res)}
    cells = {}
    def score(ml):
        m, l = ml
        pairs = [(PHRASES[i][0], tr[(m, l, i)][0]) for i in range(len(PHRASES))]
        return ml, pairs, judge(a.judge, l, pairs)
    with ThreadPoolExecutor(8) as ex:
        for (m, l), pairs, sc in ex.map(score, [(m, l) for m in models for l in langs]):
            keep = [all(t in pairs[i][1] for t in PHRASES[i][1]) for i in range(len(PHRASES))]
            ade = [s.get("adequacy", 0) for s in sc]; flu = [s.get("fluency", 0) for s in sc]
            errs = sum(p[1].startswith("ERROR") or not p[1] for p in pairs)
            lat = [tr[(m, l, i)][1] for i in range(len(PHRASES)) if tr[(m, l, i)][1]]
            cells[f"{m}|{l}"] = dict(adequacy=statistics.mean(ade) if ade else 0, fluency=statistics.mean(flu) if flu else 0,
                                     kept=sum(keep) / len(keep), errors=errs, latency=statistics.median(lat) if lat else 0, pairs=pairs)
    json.dump(cells, open(os.path.join(ROOT, "runs", "bench_translation.json"), "w"), ensure_ascii=False, indent=1)
    print(f"judge: {a.judge}; adequacy/fluency 1-5 (mean of {len(PHRASES)}), kept = share of phrases with codes/numbers/names intact\n")
    print("| Model | " + " | ".join(langs) + " | kept | median s |\n|---|" + "---|" * (len(langs) + 2))
    for m in models:
        c = [cells[f"{m}|{l}"] for l in langs]
        print(f"| {m} | " + " | ".join(f"{x['adequacy']:.1f}/{x['fluency']:.1f}" + (f" ({x['errors']} err)" if x['errors'] else "") for x in c)
              + f" | {statistics.mean(x['kept'] for x in c):.0%} | {statistics.median(x['latency'] for x in c):.1f} |")

main()
