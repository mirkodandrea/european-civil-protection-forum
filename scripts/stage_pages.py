#!/usr/bin/env python3
"""Assemble an atomic Pages site from two independently tested branch builds.

Usage: stage_pages.py SITE_DIRECTORY

SITE_DIRECTORY/<branch>/ (branch = main, dev) must contain the standalone app
`index.html` and `version.json`; it may contain `release.json`, the branch's own
description copied from the repository root by the build. The script writes the
landing page SITE_DIRECTORY/index.html. See docs/standalone-pages.md.
"""
from datetime import datetime
from html import escape
import json
from pathlib import Path
import re
import sys

BRANCHES = ('main', 'dev')

# Used only when a build carries no release.json (e.g. a branch that predates it).
DEFAULT_MODES = [
    {'query': '', 'name': 'Full exercise',
     'description': 'Launcher, EXCON console, every player role and the 3D simulator in one browser window.'},
    {'query': '?bench=1', 'name': 'Interaction bench',
     'description': 'EXCON and player screens side by side against a fake server, to try the message flow between users.'},
    {'query': '?simbench=1', 'name': 'Simulator bench',
     'description': '3D simulator next to the EXCON control console: run the clock and apply actions at once. Nothing is saved.'},
]

QUERY = re.compile(r'^(\?[A-Za-z0-9_.=&-]+)?$')
COMMIT = re.compile(r'^[0-9a-f]{7,40}$')
REPOSITORY = re.compile(r'^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')


def _text(release, key, path, required=False):
    value = release.get(key)
    if value is None and not required:
        return ''
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f'{path}: "{key}" must be a non-empty string')
    return value.strip()


def load_release(path: Path):
    """The branch's own title, description, highlights and modes, or None if absent."""
    if not path.is_file():
        return None
    try:
        release = json.loads(path.read_text())
    except json.JSONDecodeError as e:
        raise ValueError(f'{path}: invalid JSON ({e})') from e
    if not isinstance(release, dict):
        raise ValueError(f'{path}: expected a JSON object')
    out = {
        'title': _text(release, 'title', path, required=True),
        'description': _text(release, 'description', path, required=True),
        'status': _text(release, 'status', path),
        'scenario': _text(release, 'scenario', path),
    }
    highlights = release.get('highlights', [])
    if not isinstance(highlights, list) or not all(isinstance(h, str) and h.strip() for h in highlights):
        raise ValueError(f'{path}: "highlights" must be a list of non-empty strings')
    out['highlights'] = [h.strip() for h in highlights]
    modes = release.get('modes', DEFAULT_MODES)
    if not isinstance(modes, list) or not modes:
        raise ValueError(f'{path}: "modes" must be a non-empty list')
    out['modes'] = []
    for i, mode in enumerate(modes):
        where = f'{path}: modes[{i}]'
        if not isinstance(mode, dict):
            raise ValueError(f'{where} must be an object')
        query = mode.get('query', '')
        if not isinstance(query, str) or not QUERY.match(query):
            raise ValueError(f'{where}: "query" must be empty or like "?bench=1"')
        out['modes'].append({'query': query, 'name': _text(mode, 'name', where, required=True),
                             'description': _text(mode, 'description', where, required=True)})
    return out


def _date(value):
    """ISO timestamp -> (machine value, human text), or None."""
    if not isinstance(value, str) or not value:
        return None
    try:
        when = datetime.fromisoformat(value.replace('Z', '+00:00'))
    except ValueError:
        return None
    return when.isoformat(), f'{when.day} {when:%b %Y}'


def _card(branch, version, release):
    fallback = release is None
    if fallback:
        release = {'title': f'Branch {branch}', 'status': '', 'scenario': '', 'highlights': [],
                   'description': 'This build carries no release notes (release.json is missing), '
                                  'so only the standard modes are listed.',
                   'modes': DEFAULT_MODES}
    base = f'./{branch}/'
    commit = str(version.get('commit', ''))
    repo = str(version.get('repository', ''))
    repo_url = f'https://github.com/{repo}' if REPOSITORY.match(repo) else ''

    facts = []
    updated = _date(version.get('committed_at')) or _date(version.get('built_at'))
    facts.append(('Last updated', f'<time datetime="{escape(updated[0])}">{escape(updated[1])}</time>'
                  if updated else 'not recorded'))
    short = escape(commit[:12]) or 'unknown'
    if repo_url and COMMIT.match(commit):
        facts.append(('Commit', f'<a href="{repo_url}/commit/{commit}"><code>{short}</code></a>'))
    else:
        facts.append(('Commit', f'<code>{short}</code>'))
    number, run = str(version.get('run_number', '')), str(version.get('run', ''))
    label = f'#{number}' if number.isdigit() else (f'run {run}' if run.isdigit() else 'not recorded')
    if repo_url and run.isdigit():
        label = f'<a href="{repo_url}/actions/runs/{run}">{escape(label)}</a>'
    else:
        label = escape(label)
    built = _date(version.get('built_at'))
    if built:
        label += f' · <time datetime="{escape(built[0])}">{escape(built[1])}</time>'
    facts.append(('Build', label))

    status = f'<span class="status">{escape(release["status"])}</span>' if release['status'] else ''
    scenario = (f'<p class="scenario"><strong>Scenario:</strong> {escape(release["scenario"])}</p>'
                if release['scenario'] else '')
    highlights = ''.join(f'<li>{escape(h)}</li>' for h in release['highlights'])
    highlights = f'<ul class="highlights">{highlights}</ul>' if highlights else ''
    modes = ''.join(
        f'<li><a href="{escape(base + m["query"])}">{escape(m["name"])}</a>'
        f'<code>{escape("/" + branch + "/" + m["query"])}</code>'
        f'<p>{escape(m["description"])}</p></li>'
        for m in release['modes'])
    meta = ''.join(f'<div><dt>{k}</dt><dd>{v}</dd></div>' for k, v in facts)
    heading = f'v-{branch}'
    return f'''<article aria-labelledby="{heading}" class="{'fallback' if fallback else ''}">
<p class="eyebrow"><span class="branch">/{branch}/</span>{status}</p>
<h2 id="{heading}">{escape(release['title'])}</h2>
<p class="description">{escape(release['description'])}</p>{scenario}{highlights}
<dl class="facts">{meta}</dl>
<h3>Modes</h3>
<ul class="modes">{modes}</ul>
<div class="actions"><a class="primary" href="{base}">Open {branch}</a><a class="secondary" href="{base}index.html" download="ttx-{branch}.html">Download HTML</a></div>
</article>'''


STYLE = '''
*{box-sizing:border-box}
body{font:16px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif;color:#182b3a;background:#edf2f5;margin:0;padding:40px 20px}
main{max-width:1120px;margin:auto}
.kicker{margin:0;color:#526674;font-size:14px;letter-spacing:.04em;text-transform:uppercase}
h1{line-height:1.2;margin:4px 0 8px;font-size:32px}
.lead{max-width:760px;margin:0;color:#33495a}
section.versions{display:grid;grid-template-columns:repeat(auto-fit,minmax(420px,1fr));gap:24px;margin:32px 0}
article{background:#fff;border:1px solid #ccd7de;border-radius:12px;padding:28px;display:flex;flex-direction:column}
article.fallback{border-style:dashed}
.eyebrow{display:flex;gap:8px;align-items:center;margin:0 0 6px}
.branch{font:600 14px ui-monospace,SFMono-Regular,Menlo,monospace;background:#182b3a;color:#fff;border-radius:4px;padding:2px 8px}
.status{font-size:13px;font-weight:600;background:#e3f0e6;color:#1f5a32;border-radius:999px;padding:2px 10px}
h2{margin:0 0 8px;font-size:24px;line-height:1.25}
h3{font-size:15px;text-transform:uppercase;letter-spacing:.04em;color:#526674;margin:20px 0 8px}
.description{margin:0 0 8px}
.scenario{margin:0 0 8px;color:#33495a}
.highlights{margin:4px 0 0;padding-left:20px;color:#33495a}
.highlights li{margin:2px 0}
.facts{display:flex;flex-wrap:wrap;gap:8px 24px;margin:16px 0 0;padding:12px 0 0;border-top:1px solid #e1e8ed;font-size:14px}
.facts dt{color:#526674}
.facts dd{margin:0;font-weight:600}
.facts a{color:#214e68}
.modes{list-style:none;margin:0;padding:0;display:grid;gap:8px}
.modes li{border:1px solid #e1e8ed;border-radius:8px;padding:10px 12px;display:grid;grid-template-columns:auto 1fr;gap:2px 10px;align-items:baseline}
.modes a{font-weight:600;color:#214e68}
.modes code{justify-self:end;font-size:13px;color:#526674}
.modes p{grid-column:1/-1;margin:0;font-size:14px;color:#33495a}
.actions{margin-top:auto;padding-top:20px;display:flex;flex-wrap:wrap;gap:8px}
.actions a{display:inline-block;padding:10px 16px;border-radius:6px;text-decoration:none;font-weight:600}
.primary{background:#214e68;color:#fff}
.secondary{background:#e6edf2;color:#214e68}
a:focus-visible{outline:3px solid #f0a020;outline-offset:2px}
code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace}
footer{font-size:14px;color:#526674;max-width:860px}
footer p{margin:6px 0}
@media(max-width:520px){section.versions{grid-template-columns:1fr}.modes li{grid-template-columns:1fr}.modes code{justify-self:start}}
'''


def stage(directory: Path):
    cards = []
    for branch in BRANCHES:
        app = directory / branch / 'index.html'
        if not app.is_file() or app.stat().st_size == 0:
            raise ValueError(f'Missing standalone app: {app}')
        version = json.loads((directory / branch / 'version.json').read_text())
        if version.get('branch') != branch:
            raise ValueError(f'Wrong branch metadata for {branch}')
        cards.append(_card(branch, version, load_release(directory / branch / 'release.json')))
    html = f'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>PPRD East 4 · Wildfire TTX in the browser</title><style>{STYLE}</style></head><body><main>
<p class="kicker">PPRD East 4 · Work in progress</p>
<h1>Wildfire table-top exercise in the browser</h1>
<p class="lead">Two versions are published side by side. Each one runs the simulation and every role locally, with no backend. <strong>main</strong> is the version the team is testing; <strong>dev</strong> is the next one, still in development. Each card is written by its own branch.</p>
<section class="versions" aria-label="Published versions">{''.join(cards)}</section>
<footer>
<p>The downloaded HTML file also works offline; add a mode to its address (for example <code>ttx-main.html?simbench=1</code>) to open it in that mode. The optional AI needs Internet access and a personal OpenRouter key.</p>
<p>Both versions run on this same site and share the browser’s saved data. When switching between them, start a new session from the launcher instead of resuming one saved by the other version.</p>
</footer>
</main></body></html>
'''
    (directory / 'index.html').write_text(html)
    (directory / '.nojekyll').touch()


if __name__ == '__main__':
    stage(Path(sys.argv[1]))
