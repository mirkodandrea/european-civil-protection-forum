#!/usr/bin/env python3
"""Player-UI translations beyond Italian/English: find what is missing and fill it.

Sources needing a translation in every player language (fr de es pt ro el uk hy
ka az):

* player keys of `crates/frontend/src/i18n.rs` (`en()`), unless one of the
  per-language catalogues (`i18n_lang`, `i18n_reply`, ...) already has them;
* English texts of `pair(lang, "it", "en")` calls in `crates/frontend/src`.

Translations live in `crates/frontend/src/i18n_more.json`
({lang: {source: text}}, source = key id or English text). The Rust test
`i18n_more::tests` fails on any gap, so this script is the way to fill one.

    scripts/i18n/player_catalogue.py --check   # list gaps, exit 1 if any
    scripts/i18n/player_catalogue.py           # translate gaps with OpenRouter (.env key)
"""
import json
import os
import re
import sys
import urllib.request

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
SRC = os.path.join(ROOT, "crates", "frontend", "src")
CATALOGUE = os.path.join(SRC, "i18n_more.json")
LANGS = {
    "fr": "French", "de": "German", "es": "Spanish", "pt": "Portuguese (European)",
    "ro": "Romanian", "el": "Greek", "uk": "Ukrainian", "hy": "Armenian",
    "ka": "Georgian", "az": "Azerbaijani",
}
TABLES = ["i18n_lang.rs", "i18n_reply.rs", "i18n_help.rs", "i18n_function.rs",
          "i18n_place.rs", "i18n_lobby.rs", "i18n_review.rs"]
STR = r'"((?:[^"\\]|\\.)*)"'


def unescape(s):
    return json.loads('"' + s + '"')


def sources():
    """{source: English text} needing a translation per language."""
    text = open(os.path.join(SRC, "i18n.rs"), encoding="utf8").read()
    en_body = text[text.index("fn en(key"):]
    en = {unescape(k): unescape(v) for k, v in re.findall(r'^\s*' + STR + r'\s*=>\s*' + STR, en_body, re.M)}
    covered = set()
    for f in TABLES:
        body = open(os.path.join(SRC, f), encoding="utf8").read()
        m = re.search(r'KEYS: \[&str; \d+\] = \[(.*?)\];', body, re.S)
        covered |= {unescape(k) for k in re.findall(STR, m.group(1))}
    out = {k: v for k, v in en.items() if k not in covered}
    for name in sorted(os.listdir(SRC)):
        if not name.endswith(".rs"):
            continue
        body = open(os.path.join(SRC, name), encoding="utf8").read()
        for _it, en_text in re.findall(r'\bpair\(\s*[^,()]+,\s*' + STR + r'\s*,\s*' + STR + r'\s*,?\s*\)', body):
            out[unescape(en_text)] = unescape(en_text)
    return out


def gaps(catalogue, src):
    return {lang: {k: v for k, v in src.items() if not catalogue.get(lang, {}).get(k)} for lang in LANGS}


def env_key():
    if os.environ.get("OPENROUTER_API_KEY"):
        return os.environ["OPENROUTER_API_KEY"]
    path = os.path.join(ROOT, ".env")
    if os.path.exists(path):
        for line in open(path, encoding="utf8"):
            if line.startswith("OPENROUTER_API_KEY="):
                return line.split("=", 1)[1].strip()
    sys.exit("OPENROUTER_API_KEY missing (environment or .env)")


def translate(key, lang, items):
    system = (
        f"You translate the user interface of a civil-protection table-top exercise (TTX) web app "
        f"for participants (mayors, fire commanders, civil-protection officers) into {LANGS[lang]}. "
        "Short, natural UI wording, consistent terminology, formal register. Keep every '{}' placeholder, "
        "symbol (·, →, ×, …), leading/trailing spaces and punctuation exactly. Keep zone codes, 'AI' and "
        "'Ctrl+Enter'. 'Inject' is an exercise message from exercise control: keep the word 'inject' as a loanword "
        "(never a medical injection). Answer with ONE JSON object "
        "mapping each input key to its translation, nothing else."
    )
    body = json.dumps({
        "model": os.environ.get("TTX_I18N_MODEL", "google/gemini-3.1-flash-lite"),
        "temperature": 0,
        "response_format": {"type": "json_object"},
        "messages": [{"role": "system", "content": system},
                     {"role": "user", "content": json.dumps(items, ensure_ascii=False)}],
    }).encode()
    req = urllib.request.Request("https://openrouter.ai/api/v1/chat/completions", body, {
        "Authorization": "Bearer " + key, "Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=180) as r:
        content = json.load(r)["choices"][0]["message"]["content"]
    content = content.strip().removeprefix("```json").removesuffix("```").strip()
    out = json.loads(content)
    good = {}
    for k, en in items.items():
        t = out.get(k)
        if isinstance(t, str) and t.strip() and t.count("{}") == en.count("{}"):
            good[k] = t
        else:
            print(f"  {lang}/{k}: unusable answer {t!r}", file=sys.stderr)
    return good


def main():
    catalogue = json.load(open(CATALOGUE, encoding="utf8")) if os.path.exists(CATALOGUE) else {}
    src = sources()
    missing = gaps(catalogue, src)
    total = sum(len(v) for v in missing.values())
    if "--check" in sys.argv:
        for lang, items in missing.items():
            for k in items:
                print(f"{lang}\t{k}")
        print(f"{total} missing translations ({len(src)} sources)", file=sys.stderr)
        sys.exit(1 if total else 0)
    key = env_key()
    for lang, items in missing.items():
        if not items:
            continue
        print(f"{lang}: {len(items)} strings", file=sys.stderr)
        catalogue.setdefault(lang, {}).update(translate(key, lang, items))
    catalogue = {lang: dict(sorted(catalogue.get(lang, {}).items())) for lang in LANGS}
    with open(CATALOGUE, "w", encoding="utf8") as f:
        json.dump(catalogue, f, ensure_ascii=False, indent=1)
        f.write("\n")
    left = sum(len(v) for v in gaps(catalogue, src).values())
    print(f"done; {left} still missing", file=sys.stderr)
    sys.exit(1 if left else 0)


if __name__ == "__main__":
    main()
