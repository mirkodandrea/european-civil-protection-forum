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
crea una sessione locale; su dev viene salvata nel local storage e ripresa al
ricaricamento (vedi “Persistenza di dev”). EXCON,
partecipanti e valutatore condividono una sola sessione nelle schede dei ruoli.
Per partecipanti su dispositivi diversi serve la versione con server.

Il file `web/standalone/ttx.html` contiene codice WASM, scenario, interfaccia e
asset necessari. Può essere aperto con `file://` senza rete; usare un browser
moderno con WebAssembly e DecompressionStream. Le viste 3D richiedono
anche supporto GPU/WebGL. L’AI è opzionale, richiede rete e contatta direttamente
OpenRouter con la chiave inserita nell’app. Su dev la chiave viene salvata nel
local storage del browser, riutilizzata
alle aperture successive e cancellata con “Rimuovi chiave”; se il browser blocca
il salvataggio, resta valida solo nella pagina corrente. Non entra nel debrief.
La vista 3D di dev è disattivata inizialmente e si avvia alla prima attivazione
di “Simulazione”, con vegetazione e particelle ridotte e senza anti-aliasing SMAA. Senza chiave è
disponibile il flusso manuale EXCON.
Esportare il debrief JSON per conservare le evidenze; la sessione si riprende
automaticamente solo nello stesso browser.

All’apertura dev mostra un launcher prima di costruire esercitazione e
interfaccia: riprendere la sessione salvata, scegliere lo scenario di una nuova
esercitazione (la sessione salvata viene eliminata), impostare la lingua della
console EXCON e la chiave OpenRouter, o cancellare i dati salvati. Ogni nuova
esercitazione chiede la lingua nella scheda di ciascun ruolo (quella dello
scenario è proposta); l’EXCON avvia quando tutti hanno scelto, o con “Avvia
comunque”.
“Salva e inizia nuova sessione” dalle Impostazioni riparte direttamente,
senza launcher.

## Build e verifica

Installare il target `wasm32-unknown-unknown` e Trunk 0.21.14 (stesso
build su main e dev). Poi:

```sh
OPENROUTER_API_KEY='' TTX_LLM_CASSETTE='' ./scripts/build_standalone.sh
cargo test --locked --release -p server -p exercise -p llm
npm ci --prefix scripts/record
scripts/record/node_modules/.bin/playwright install chromium
TTX_BROWSER_CHANNEL=chromium node scripts/qa/standalone.cjs
TTX_BROWSER_CHANNEL=chromium node scripts/qa/standalone-settings.cjs
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
o dev o un avvio manuale esegue i test del modello e costruisce entrambi i branch, poi compone
un unico sito con `scripts/stage_pages.py`. Le pull request verificano il codice
proposto senza pubblicare. Le pubblicazioni sono serializzate tra tutti i branch;
un errore nei test o nella build impedisce il deploy dell’intero sito.
La verifica browser estesa può essere richiesta dall’avvio manuale del workflow
con `verify_standalone=true`; se richiesta, anche il suo esito blocca il deploy.
Ogni sottocartella include `version.json` con branch, commit e run GitHub.
La verifica browser opzionale usa Chrome preinstallato sul runner Ubuntu, con ANGLE SwiftShader per
verificare il 3D senza una GPU fisica. Le prove browser sono conservate negli
artefatti della run per sette giorni.

## Persistenza di dev

La versione standalone di dev salva nel `localStorage` del browser:

- `ttx.standalone.session`: la sessione in corso. Non è uno snapshot dello stato,
  ma un *journal*: gli injects di partenza, le richieste che hanno cambiato lo
  stato (`server::browser::request`, tranne `presence`), le risposte dell’AI
  (tape in memoria, indipendente dal modello) e il tempo simulato raggiunto.
  Al caricamento le richieste vengono rigiocate su un’esercitazione nuova, con le
  risposte AI dal tape e il clock reale sospeso (`flow::REPLAYING`,
  `flow::advance_to`); al termine il clock è in pausa. Se un passo non è più
  accettato la sessione viene scartata con un avviso, senza export.
  Si salva dopo ogni richiesta e ogni secondo se il tempo è avanzato.
- `ttx.standalone.injects`: libreria di injects modificabili; una nuova sessione
  parte da qui (altrimenti dagli injects dello scenario). Una sessione salvata
  conserva i propri injects.
- `ttx.standalone.openrouter-key`: chiave OpenRouter.

“Impostazioni” contiene l’editor degli injects (aggiungi, elimina, import/export
JSON, “Salva e inizia nuova sessione”) e bottoni espliciti, con conferma, per
eliminare la sessione, la libreria injects, la chiave o tutto.

## Persistenza nella versione con server

`ttx` salva sessione e libreria injects in `--state-dir` (default `runs/state`;
`--no-state` la disattiva, `--fresh` scarta la sessione salvata all’avvio):

- `session.json`: stesso schema del browser, ma il journal registra le
  richieste POST accettate (fairing `persist::JournalFairing`) con il
  partecipante che le ha fatte. All’avvio vengono rigiocate sulle vere rotte con
  un client Rocket locale (`persist::restore`) prima di mettersi in ascolto. Una
  sessione che non corrisponde a scenario/seed/opzioni, o non più riproducibile,
  viene scartata con un avviso nel log.
- `injects.json`: libreria injects usata da una nuova esercitazione.

Endpoint EXCON: `GET /api/master/storage`, `POST /api/master/storage/injects`
(`new_session` opzionale) e `POST /api/master/storage/delete` (`session`,
`injects`, `all`). Nella UI sono in Strumenti → Impostazioni.
Cancellare la sessione riavvia l’esercitazione sul posto per tutti i
partecipanti.
