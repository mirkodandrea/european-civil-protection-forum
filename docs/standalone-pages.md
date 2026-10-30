# Versioni standalone su GitHub Pages

La [pagina iniziale](https://mirkodandrea.github.io/european-civil-protection-forum/)
(in inglese, per il pubblico internazionale) mostra una scheda per ciascuna delle
due app HTML indipendenti:

- [main](https://mirkodandrea.github.io/european-civil-protection-forum/main/):
  la versione che il team sta testando (scenario Valverde).
- [dev](https://mirkodandrea.github.io/european-civil-protection-forum/dev/):
  la versione successiva, in sviluppo (isola di Roccavento, gestione per
  borghi/distretti con `EvacuateDistrict` a partenze cadenzate).

Ogni scheda riporta titolo, descrizione, scenario e novità scritti dal branch
stesso in `release.json` (vedi sotto), data dell’ultimo commit, commit e numero
di build, un link diretto a ogni modalità e i pulsanti “Open” e “Download HTML”.

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
npm ci --prefix scripts/qa
scripts/qa/node_modules/.bin/playwright install chromium
TTX_BROWSER_CHANNEL=chromium node scripts/qa/standalone.cjs
TTX_BROWSER_CHANNEL=chromium node scripts/qa/standalone-settings.cjs
```

La verifica è headless e offline: attraversa inject, risposta, approvazione,
risoluzione e rollback con i controller reali, confronta lo stato fisico con
una sequenza nativa equivalente e salva immagini e manifest in
`runs/standalone-review/`. Su dev verifica anche previsione e AI con rete simulata.
Tenere directory `CARGO_TARGET_DIR` diverse se si compilano contemporaneamente
i due branch in worktree separati.

## Modalità (query string)

La stessa app si apre in modalità diverse a seconda dell’indirizzo; vale anche
per il file scaricato (`ttx-main.html?simbench=1`):

| Indirizzo | Modalità | A cosa serve |
|---|---|---|
| nessun parametro | Esercitazione completa | launcher, console EXCON, tutti i ruoli e simulatore 3D in una finestra |
| `?bench=1` | [Banco di prova](banco-di-prova.md) | schermate EXCON e partecipante affiancate su un server finto, per provare il flusso di messaggi (feature `bench`) |
| `?simbench=1` (`&scn=ID`) | [Banco simulatore](banco-simulatore.md) | simulatore 3D con la console Controllo EXCON; clock avviato, in pausa o a salti; nulla viene salvato |
| `?editor=1` | Editor scenario 3D | bozza di scenario completo, territorio incluso, senza esercitazione ([scenari](scenari.md#editor-scenario-3d)) |

Senza questi parametri l’app non usa i banchi. `/main/` e `/dev/` sono sullo
stesso sito e condividono il `localStorage` (chiavi `ttx.standalone.*`): passando
da una versione all’altra conviene iniziare una nuova sessione dal launcher.

## Pagina iniziale e `release.json`

`release.json`, nella radice del repository, descrive la scheda del proprio
branch. La build lo copia in `_branch/` accanto a `version.json`;
`scripts/stage_pages.py` lo legge da `_site/<branch>/release.json`. Campi:

- `title` e `description` (obbligatori): titolo leggibile e una o due frasi;
- `status` (facoltativo): etichetta breve, es. `Playtest`, `In development`;
- `scenario` (facoltativo): scenario/territorio principale;
- `highlights` (facoltativo): elenco di novità, una riga ciascuna;
- `modes` (facoltativo): elenco di `{query, name, description}`, con `query`
  vuota o del tipo `?bench=1`. Se manca: esercitazione, `?bench=1`, `?simbench=1`.

Un file non valido blocca la pubblicazione con un errore esplicito. Se il file
manca (branch precedente a questa pagina) la scheda è generica, con “Branch X”
e le modalità predefinite. Tutto il testo è in inglese.

Su **dev**, dopo il merge di main, adattare il file al contenuto di dev, ad esempio:

```json
{
  "title": "Roccavento island · borghi and evacuation",
  "status": "In development",
  "description": "The next version: a wildfire on the Roccavento island, where settlements are managed borgo by borgo and districts are evacuated with paced departures.",
  "scenario": "Roccavento island, organised in borghi and districts",
  "highlights": [
    "EvacuateDistrict: evacuate a district with paced departures instead of all at once",
    "Scenario editor: Borghi tab and editable districts in scenario packs",
    "Launcher and session headers show commit, build number and build date"
  ],
  "modes": []
}
```

Al posto di `"modes": []` copiare l’elenco `modes` di main (le quattro modalità,
se dev le supporta ancora) oppure togliere la chiave per avere le tre predefinite;
un elenco vuoto non è accettato.

Aggiornare `release.json` quando cambia ciò che il branch offre (scenario,
novità, modalità). Test: `python3 -m unittest scripts/test_stage_pages.py`.

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
Ogni sottocartella include `version.json` (branch, commit, run e numero di run
GitHub, repository, data del commit `committed_at` e della build `built_at`;
i campi assenti nelle build più vecchie sono mostrati come “not recorded”) e,
se presente nel branch, `release.json`.
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
