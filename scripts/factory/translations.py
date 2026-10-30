"""Static translations of the map labels of the shipped territories.

Players read places, buildings, roads and borghi in their own language; the
EXCON and the commands keep the source names. The recipes call `fill` before
writing a territory: translations of the previous output whose source text is
still in the territory are carried over, and the labels still missing are
drafted by the LLM (`translate-territory`, key in `.env`). Labels already
written are never changed: correct them in the file or in the scenario editor
("Traduzioni" tab).

Run alone it fills the shipped territories in place:

    python3 scripts/factory/translations.py [TERRITORY ...] [--lang xx ...]
"""
import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SHIPPED = ['valverde_factory', 'selvariva_basin', 'roccavento_island', 'synthetic_valley']


def _strings(v, out):
    if isinstance(v, str):
        out.add(v)
    elif isinstance(v, list):
        for x in v:
            if isinstance(x, (str, list, dict)):
                _strings(x, out)
    elif isinstance(v, dict):
        for k, x in v.items():
            if k != 'translations':
                _strings(x, out)
    return out


def map_texts(s):
    """Names drawn on the maps, as `Scenario::map_texts` lists them."""
    names = [p['name'] for p in s.get('places', [])]
    names += [b['name'] for b in s.get('buildings', []) if b.get('name')]
    names += [r['name'] for r in s.get('roads', [])]
    names += [d['name'] for d in s.get('districts', [])]
    names += [x['name'] for x in s.get('sectors', [])]
    return sorted({n for n in names if n.strip()})


def translate(path, langs=()):
    """`{lang: {source: text}}` for the labels the file at `path` does not translate yet."""
    out = subprocess.run(
        ['cargo', 'run', '--quiet', '--release', '-p', 'app', '--bin', 'translate-territory', '--', str(path), *langs],
        cwd=ROOT, check=True, capture_output=True, text=True)
    print(out.stderr, end='')
    return json.loads(out.stdout)


def merge(s, new):
    """Adds `new` to the territory's translations, keeping what is written; returns how many."""
    table = s.setdefault('translations', {})
    added = 0
    for lang, names in new.items():
        own = table.setdefault(lang, {})
        for source, text in names.items():
            if source not in own:
                own[source] = text
                added += 1
    s['translations'] = {lang: dict(sorted(t.items())) for lang, t in sorted(table.items()) if t}
    return added


def fill(s, previous=None, llm=True, langs=()):
    """Gives a freshly generated territory `s` its translations: those of
    `previous` (the file it replaces) whose source text still appears, then,
    with `llm`, the missing map labels. Without `llm` the gaps are reported."""
    if previous and Path(previous).exists():
        old = json.loads(Path(previous).read_text()).get('translations', {})
        present = _strings(s, set())
        merge(s, {lang: {k: v for k, v in t.items() if k in present} for lang, t in old.items()})
    if llm:
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / 'territory.json'
            path.write_text(json.dumps(s, ensure_ascii=False, separators=(',', ':')))
            print('translations:', merge(s, translate(path, langs)), 'labels drafted')
    labels = map_texts(s)
    gaps = {lang: n for lang in (langs or LANGUAGES)
            if (n := sum(1 for t in labels if t not in s.get('translations', {}).get(lang, {})))}
    if gaps:
        print('untranslated map labels:', gaps, '(run scripts/factory/translations.py)')
    return s


LANGUAGES = ['en', 'it', 'fr', 'de', 'es', 'pt', 'ro', 'el', 'uk', 'hy', 'ka', 'az']


def patch(path, langs=()):
    """Fills the territory at `path` in place and keeps its manifest honest."""
    s = json.loads(path.read_text())
    added = merge(s, translate(path, langs))
    text = json.dumps(s, ensure_ascii=False, separators=(',', ':')) + '\n'
    path.write_text(text)
    manifest = path.with_suffix('.manifest.json')
    if manifest.exists():
        m = json.loads(manifest.read_text())
        m['scenario_sha256'] = hashlib.sha256(text.encode()).hexdigest()
        manifest.write_text(json.dumps(m, ensure_ascii=False, indent=2) + '\n')
    return added


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('territories', nargs='*', default=SHIPPED)
    ap.add_argument('--lang', action='append', default=[])
    args = ap.parse_args()
    for name in args.territories:
        path = ROOT / 'data' / 'scenarios' / f'{name}.json'
        print(name, patch(path, args.lang), 'labels added')


if __name__ == '__main__':
    main()
