# App WebAssembly autocontenuta

La build standalone include frontend, scenario Valverde, motore PROPAGATOR,
World, Exercise e orchestrazione AI nello stesso modulo WASM. Non avvia processi
server e non usa HTTP/WebSocket per le operazioni dell’esercitazione.

```sh
./scripts/build_standalone.sh
```

Il risultato distribuibile è **`web/standalone/ttx.html`**: un singolo file con
WASM compresso, scenario, CSS, icona e loader incorporati. Si apre direttamente
nel browser, anche come `file://` e offline. Per distribuire un link basta
ospitare questo file su un hosting statico HTTPS; nessun backend applicativo.
La directory intermedia contiene anche gli asset Trunk, ma non servono per
eseguire `ttx.html`. La build nativa e `scripts/build_frontend.sh` restano disponibili.

## Uso

- A sinistra la situazione completa della simulazione, a destra EXCON, strumenti,
  valutatore e player. Il divisorio è regolabile; **Simulazione** nasconde la mappa.
- Le schede cambiano vista senza cambiare esercitazione o perdere le bozze.
  **Tutti i player** mostra contemporaneamente i tre pannelli dei partecipanti.
- **OpenRouter** permette di impostare chiave, modello di interpretazione e modello
  di traduzione. **Applica** svuota il campo della chiave; **Rimuovi chiave** disattiva
  l’AI. La configurazione è condivisa anche con clock e consegne programmate.
- La chiave vive soltanto in memoria, separata da Exercise: non è scritta in
  storage, URL, log, snapshot o debrief. Le richieste AI usano direttamente
  `https://openrouter.ai/api/v1/chat/completions`, con timeout di 90 secondi.
- Senza chiave o connessione si usa il percorso manuale EXCON. L’AI richiede
  Internet; la simulazione e i controlli non lo richiedono.
- Il rollback usa gli snapshot reali del motore. Il debrief JSON si scarica dagli
  strumenti EXCON. Ricaricare/chiudere la pagina perde la sessione corrente.

Ogni apertura dell’app crea una sessione indipendente. I ruoli permettono a un
collega di provare tutti i flussi sul proprio computer; non sincronizzano computer
diversi. La configurazione incorporata usa seme 1, 32 realizzazioni, cutoff 0.5 e
modello operativo abilitato.

La visualizzazione della simulazione in questa build è la mappa operativa 2D.
Il renderer Bevy PBR della versione nativa non è incluso. Tutti gli effetti
fisici provengono comunque dallo stesso World, senza un modello semplificato.
La fisica avanza per passi con cessione del controllo al browser; i forecast
vengono calcolati su checkpoint propri. I calcoli più grandi possono impegnare
il thread della pagina: non si promette la velocità del motore nativo parallelo.

## Struttura

`frontend --features standalone` include `scenario`, `sim`, `exercise` e i
controller condivisi di `server`. Il nome di questo crate è storico: sul target
WASM sono esclusi Rocket, socket, file server, runtime Tokio e trasporto HTTP.
`server::browser` instrada le operazioni direttamente alle stesse funzioni
usate dalla versione nativa; mantiene autorizzazioni dei ruoli e validazione
JSON. `browser_runtime` usa future locali e timer del browser. I clock usano
`web-time`, che sul target nativo riutilizza gli orologi standard.

Il rendering legge le viste del modello. Le decisioni rimangono in World,
le approvazioni in Exercise, e la preparazione privata conserva revisione,
conferma esplicita e invalidazione al rollback. Le capacità per ruolo regolano
le viste; nel laboratorio il tester può deliberatamente selezionare ogni ruolo.

## Verifica invisibile

```sh
cargo test --release --offline -p server -p exercise -p llm
node scripts/qa/standalone.cjs
```

La fixture genera il riferimento nativo con
`cargo run --release --offline -p exercise --example standalone_reference`,
apre soltanto Chrome headless su `file://`, disabilita la rete e usa comandi reali:
inject, interpretazione manuale privata, approvazione, risoluzione di un minuto,
forecast privato e pubblicazione revisionata, rollback. Confronta esattamente
lo stato fisico WASM con il riferimento nativo accoppiato per scenario/seme/comandi.
Verifica pannelli simultanei, mappa nascondibile, export e schermata tablet.

La seconda parte intercetta esclusivamente il fetch diretto OpenRouter con una
risposta fittizia: verifica header della chiave, configurazione e rimozione,
modelli e assenza di credenziali nel debrief. Non chiama modelli reali e non
costituisce un collaudo live del servizio OpenRouter o delle sue politiche CORS.
Immagini e manifest sono salvati in `runs/standalone-review/` e ispezionati senza
portare finestre in primo piano.

## GitHub Pages e CI

Il workflow `.github/workflows/pages.yml` esegue su pull request verso `main`,
sui push a `main` e `dev` e su avvio manuale. Usa Rust 1.92.0, Trunk 0.21.14 e il
lockfile del repository; esegue i test nativi, costruisce WASM e collauda il
file finale offline in Chromium headless, con riferimento nativo accoppiato.
Non richiede una chiave OpenRouter nei secret GitHub: le chiamate AI della
fixture sono simulate, e la chiave viene inserita da ogni tester nell’app.

La directory pubblicata contiene soltanto `index.html` (copia del singolo
`ttx.html` collaudato) e `.nojekyll`. Non contiene sorgenti, `.env`, cassette,
registrazioni o risultati di esercitazioni. L’app non carica asset da percorsi
assoluti e funziona anche nel sottopercorso di un sito Pages di progetto.

Quando si decide di pubblicare:

1. In **Settings → Pages → Build and deployment → Source** selezionare
   **GitHub Actions**.
2. Portare le modifiche su `main`: il job di deploy parte soltanto dopo build
   e fixture riuscite. Le pull request eseguono la verifica senza pubblicare.
3. Il sito previsto per questo repository è
   `https://mirkodandrea.github.io/european-civil-protection-forum/`.

Configurazione basata sul [workflow Pages ufficiale GitHub](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).
La preparazione locale non modifica le impostazioni del repository remoto e non
esegue commit, push o deploy. La prima esecuzione su runner Linux resta da verificare
su GitHub; il collaudo locale non certifica un’esecuzione remota mai avvenuta.

Per ripetere localmente il collaudo sul file predisposto per Pages:

```sh
mkdir -p _site
cp web/standalone/ttx.html _site/index.html
TTX_STANDALONE_FILE=_site/index.html node scripts/qa/standalone.cjs
```

`TTX_BROWSER_CHANNEL=chromium` usa il Chromium distribuito con Playwright;
in assenza di override la fixture usa Chrome, come gli altri harness del repo.
