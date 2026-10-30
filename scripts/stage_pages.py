#!/usr/bin/env python3
"""Assemble an atomic Pages site from two independently tested branch builds."""
from html import escape
import json
from pathlib import Path
import sys


def stage(directory: Path):
    cards = []
    for branch in ('main', 'dev'):
        app = directory / branch / 'index.html'
        if not app.is_file() or app.stat().st_size == 0:
            raise ValueError(f'Missing standalone app: {app}')
        version = json.loads((directory / branch / 'version.json').read_text())
        if version['branch'] != branch:
            raise ValueError(f'Wrong branch metadata for {branch}')
        commit = escape(version['commit'])
        cards.append(f'<article><h2>{branch}</h2><p>Versione standalone del branch <strong>{branch}</strong>.</p><p class="revision">Commit <code>{commit[:12]}</code></p><a href="./{branch}/">Apri {branch}</a> <a class="download" href="./{branch}/index.html" download="ttx-{branch}.html">Scarica HTML</a><p class="benches">Sviluppo: <a class="bench" href="./{branch}/?simbench=1">Banco simulatore</a> <a class="bench" href="./{branch}/?bench=1">Banco di prova</a></p></article>')
    html = '''<!doctype html><html lang="it"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>PPRD East 4 · TTX standalone</title><style>
body{font:17px/1.5 system-ui,sans-serif;color:#182b3a;background:#edf2f5;margin:0;padding:48px 24px}main{max-width:900px;margin:auto}h1{line-height:1.2}section{display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:24px;margin:32px 0}article{background:white;border:1px solid #ccd7de;border-radius:12px;padding:28px}h2{margin-top:0}a{display:inline-block;background:#214e68;color:white;padding:10px 16px;border-radius:6px;text-decoration:none;margin:4px 8px 4px 0}.download{background:#e6edf2;color:#214e68}.revision{font-size:14px;color:#526674}.benches{font-size:14px;color:#526674;margin:12px 0 0}.bench{background:#e6edf2;color:#214e68;padding:6px 12px;font-size:14px}footer{font-size:15px;color:#526674}</style></head><body><main><p>PPRD East 4 · Work in progress</p><h1>Esercitazione nel browser</h1><p>Scegli la versione da avviare. Ogni app esegue la simulazione e tutti i ruoli localmente, senza backend.</p><section>'''+''.join(cards)+'''</section><footer>Il file HTML scaricato funziona offline. L’AI opzionale richiede Internet e una chiave OpenRouter personale. Le sessioni delle due versioni sono indipendenti; ricaricare la pagina avvia una nuova esercitazione.</footer></main></body></html>'''
    (directory / 'index.html').write_text(html)
    (directory / '.nojekyll').touch()


if __name__ == '__main__':
    stage(Path(sys.argv[1]))
