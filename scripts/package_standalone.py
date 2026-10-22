#!/usr/bin/env python3
"""Embed main's original UI, local Rust controllers and scenario in one HTML."""
import base64
import gzip
import json
from pathlib import Path
import re


def package(directory=Path('web/standalone')):
    loader = (directory / 'standalone.js').read_text()
    wasm = base64.b64encode(gzip.compress((directory / 'standalone_bg.wasm').read_bytes(), mtime=0)).decode()
    common = Path('web/common.js').read_text()
    start = common.index('const accessContext')
    end = common.index('export function esc')
    common = common[:start] + '''export const accessToken = () => '';
export async function api(path, body) {
  return JSON.parse(await parent.ttxRequest(window.TTX_ROLE, path, body === undefined ? undefined : JSON.stringify(body)));
}
export function live(onChange) { onChange(); setInterval(onChange, 500); }

''' + common[end:]
    pages = {}
    css = Path('web/style.css').read_text()
    for name in ['master', 'player', 'evaluator']:
        html = Path(f'web/{name}.html').read_text().replace('<link rel="stylesheet" href="style.css">', f'<style>{css}</style>')
        html = re.sub(r"(['\"])\./common\.js[^'\"]*\1", '__COMMON_URL__', html)
        html = html.replace("new URLSearchParams(location.search).get('role')", 'window.TTX_ROLE')
        if name == 'master':
            html = re.sub(r"\$\('participantlinks'\)\.innerHTML = .*?;\n", "$('participantlinks').textContent = 'Local standalone session: choose a role in the tabs above. Remote participants require the server edition.';\n", html)
        pages[name] = html
    data = json.dumps({'common': common, 'pages': pages}).replace('<', '\\u003c')
    bootstrap = '''
const packed=Uint8Array.from(atob(WASM),c=>c.charCodeAt(0));
const bytes=new Uint8Array(await new Response(new Blob([packed]).stream().pipeThrough(new DecompressionStream('gzip'))).arrayBuffer());
await __wbg_init({module_or_path:bytes});
initialize();
window.ttxRequest=request;
const assets=ASSETS;
const commonUrl=URL.createObjectURL(new Blob([assets.common],{type:'text/javascript'}));
const stat=JSON.parse(await request('master','/api/static'));
const tabs=[{id:'master',name:'EXCON',page:'master'},...stat.roles.map(r=>({id:r.id,name:r.name,page:'player'})),{id:'evaluator',name:'Valutazione',page:'evaluator'}];
const frames=document.getElementById('frames'),bar=document.getElementById('tabs');
function select(id) {
 for(const t of tabs) {t.frame.hidden=t.id!==id;t.button.setAttribute('aria-pressed',String(t.id===id));}
}
for(const t of tabs) {
 t.button=document.createElement('button');t.button.textContent=t.name;t.button.dataset.role=t.id;t.button.onclick=()=>select(t.id);bar.append(t.button);
 t.frame=document.createElement('iframe');t.frame.title=t.name;t.frame.dataset.role=t.id;
 const setup='<script>window.TTX_ROLE='+JSON.stringify(t.id)+';<\\/script>';
 t.frame.srcdoc=assets.pages[t.page].replace('<head>','<head>'+setup).replaceAll('__COMMON_URL__',JSON.stringify(commonUrl));
 frames.append(t.frame);
}
select('master');
document.getElementById('status').textContent='Sessione locale · simulazione e mappe offline · nessun backend';
document.getElementById('configure').onclick=()=>{
 const key=document.getElementById('key');
 try {configure_ai(key.value,document.getElementById('model').value);document.getElementById('ai-status').textContent=key.value.trim()?'AI configurata · connessione diretta a OpenRouter':'AI disattivata · modalità manuale';key.value='';}
 catch(e){document.getElementById('ai-status').textContent=String(e);}
};
document.getElementById('clear').onclick=()=>{configure_ai('',document.getElementById('model').value);document.getElementById('key').value='';document.getElementById('ai-status').textContent='AI disattivata · chiave rimossa';};
'''.replace('WASM', json.dumps(wasm)).replace('ASSETS', data)
    html = '''<!doctype html><html lang="it"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>TTX · main standalone</title>
<style>html,body{margin:0;height:100%;font:15px system-ui;background:#f4f5f7;color:#20252b}body{display:flex;flex-direction:column}header,details,nav{padding:10px 16px}header{background:#182b3a;color:white}header h1{font-size:20px;margin:0 0 4px}details{background:white}details p{max-width:1000px}label{display:inline-block;margin:8px 12px 8px 0}input,button{font:inherit;padding:7px}nav{display:flex;gap:6px;overflow:auto}nav button{white-space:nowrap;border:1px solid #acb6be;border-radius:5px;background:white}button[aria-pressed=true]{background:#214e68;color:white}#frames{position:relative;flex:1;min-height:450px}iframe{position:absolute;inset:0;width:100%;height:100%;border:0}iframe[hidden]{display:none}</style></head><body>
<header><h1>PPRD East 4 · main</h1><div id="status">Caricamento esercitazione…</div></header>
<details><summary>OpenRouter · <span id="ai-status">AI disattivata · modalità manuale</span></summary><p>La chiave resta in memoria fino alla ricarica. L’AI richiede Internet. La sessione è locale: tutti i ruoli condividono questo browser. Ricaricare avvia una nuova esercitazione; esporta il debrief per conservarne le evidenze.</p><label>Chiave API <input id="key" type="password" autocomplete="off"></label><label>Modello <input id="model" value="openai/gpt-oss-120b"></label><button id="configure">Applica</button> <button id="clear">Rimuovi chiave</button></details>
<nav id="tabs" aria-label="Ruoli dell’esercitazione"></nav><main id="frames"></main><noscript>Abilita JavaScript e WebAssembly.</noscript>
'''
    html += '<script type="module">' + loader.replace('</script', '<\\/script') + '\n' + bootstrap + '</script></body></html>'
    output = directory / 'ttx.html'
    output.write_text(html)
    print(f'{output}: {output.stat().st_size / 1024 / 1024:.1f} MiB')


if __name__ == '__main__':
    package()
