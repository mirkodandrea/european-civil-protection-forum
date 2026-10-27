#!/usr/bin/env python3
"""Package a Trunk standalone build as one offline-capable HTML document."""
import base64
import gzip
from pathlib import Path
import re
import sys


def package(directory: Path) -> Path:
    source = (directory / 'index.html').read_text()
    script_path = re.search(r"import init, \* as bindings from '([^']+)'", source)
    wasm_path = re.search(r"module_or_path: '([^']+)'", source)
    if not script_path or not wasm_path:
        raise ValueError('Unrecognized Trunk loader; refusing to produce an incomplete app')
    loader = (directory / Path(script_path[1]).name).read_text()
    entry = re.search(r'export default (\w+);', loader) or re.search(r'(\w+) as default', loader)
    if not entry:
        raise ValueError('Unrecognized wasm-bindgen initializer')
    wasm = (directory / Path(wasm_path[1]).name).read_bytes()
    encoded = base64.b64encode(gzip.compress(wasm, mtime=0)).decode()
    css = '\n'.join(p.read_text() for p in sorted(directory.glob('*.css')))
    icon = base64.b64encode(Path('crates/frontend/assets/favicon.ico').read_bytes()).decode()
    bootstrap = f'''
const encoded = "{encoded}";
const packed = Uint8Array.from(atob(encoded), c => c.charCodeAt(0));
const stream = new Blob([packed]).stream().pipeThrough(new DecompressionStream('gzip'));
const bytes = new Uint8Array(await new Response(stream).arrayBuffer());
await {entry[1]}({{module_or_path: bytes}});
dispatchEvent(new CustomEvent('TrunkApplicationStarted'));
'''
    loader = loader.replace('</script', '<\\/script')
    html = f'''<!doctype html>
<html lang="it"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>PPRD East 4 · TTX autonomo</title><link rel="icon" href="data:image/x-icon;base64,{icon}">
<style>{css}</style></head><body><main id="loading" style="padding:32px"><h1>Caricamento esercitazione…</h1><p>Inizializzazione locale di WebAssembly e scenario.</p></main><noscript>Abilita JavaScript e WebAssembly.</noscript>
<script type="module">{loader}{bootstrap}</script></body></html>'''
    output = directory / 'ttx.html'
    output.write_text(html)
    print(f'{output}: {output.stat().st_size / 1024 / 1024:.1f} MiB, WASM + scenario + CSS + loader embedded')
    return output


if __name__ == '__main__':
    package(Path(sys.argv[1]))
