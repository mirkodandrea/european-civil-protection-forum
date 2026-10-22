# Versioni standalone su GitHub Pages

La [pagina iniziale](https://mirkodandrea.github.io/european-civil-protection-forum/)
permette di aprire o scaricare due app HTML indipendenti:

- [main](https://mirkodandrea.github.io/european-civil-protection-forum/main/):
  interfaccia HTML/JavaScript originale, renderer Bevy 3D originale, mappe 2D, scenario
  `synthetic_valley.json`, modello storico predefinito del branch.
- [dev](https://mirkodandrea.github.io/european-civil-protection-forum/dev/):
  interfaccia Rust/Yew, renderer Bevy 3D, scenario `valverde_factory.json`,
  modello operativo abilitato.

Entrambe usano il modello e i controller del rispettivo branch, con seme 1,
32 realizzazioni e cutoff 0.5. Non sono collegate tra loro. Ogni apertura
crea una sessione locale in memoria; ricaricare la pagina la azzera. EXCON,
partecipanti e valutatore condividono una sola sessione nelle schede dei ruoli.
Per partecipanti su dispositivi diversi serve la versione con server.

Il file `web/standalone/ttx.html` contiene codice WASM, scenario, interfaccia e
asset necessari. Può essere aperto con `file://` senza rete; usare un browser
moderno con WebAssembly e DecompressionStream. Le viste 3D richiedono
anche supporto GPU/WebGL. L’AI è opzionale, richiede rete e contatta direttamente
OpenRouter con la chiave inserita nell’app. La chiave resta in memoria e non
entra nel debrief. Senza chiave è disponibile il flusso manuale EXCON.
Esportare il debrief JSON per conservare le evidenze; non è un salvataggio
ripristinabile della sessione.

## Build e verifica

Installare il target `wasm32-unknown-unknown`. Su main installare
`wasm-bindgen-cli` 0.2.129; su dev Trunk 0.21.14. Poi:

```sh
OPENROUTER_API_KEY='' TTX_LLM_CASSETTE='' ./scripts/build_standalone.sh
cargo test --locked --release -p server -p exercise -p llm
npm ci --prefix scripts/record
scripts/record/node_modules/.bin/playwright install chromium
TTX_BROWSER_CHANNEL=chromium node scripts/qa/standalone.cjs
```

La verifica è headless e offline: attraversa inject, risposta, approvazione,
risoluzione e rollback con i controller reali, confronta lo stato fisico con
una sequenza nativa equivalente e salva immagini e manifest in
`runs/standalone-review/`. Su dev verifica anche previsione e AI con rete simulata.
Tenere directory `CARGO_TARGET_DIR` diverse se si compilano contemporaneamente
i due branch in worktree separati.

Le build pubbliche impostano chiavi e cassette vuote. Lo script locale
`run_pages_local.sh` di dev può incorporare la chiave `.env`: il suo artefatto
è destinato esclusivamente all’uso locale e non va caricato su Pages.

## Pubblicazione

`.github/workflows/pages.yml` è presente su entrambi i branch. Un push su main
o dev o un avvio manuale costruisce e verifica entrambi i branch, poi compone
un unico sito con `scripts/stage_pages.py`. Le pull request verificano il codice
proposto senza pubblicare. Le pubblicazioni sono serializzate tra tutti i branch;
un errore di build o verifica impedisce il deploy dell’intero sito.
Ogni sottocartella include `version.json` con branch, commit e run GitHub.
La CI usa Chrome preinstallato sul runner Ubuntu, con ANGLE SwiftShader per
verificare il 3D senza una GPU fisica. Le prove browser sono conservate negli
artefatti della run per sette giorni.
