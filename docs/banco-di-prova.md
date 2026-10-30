# Banco di prova (workflow test bench)

Pagina di sviluppo per provare a mano i flussi di informazione tra EXCON e
player, senza cliccare in mille posti: per ogni flusso mostra affiancate la
colonna EXCON e le colonne dei player coinvolti, più l’elenco dei passi con
quello corrente evidenziato.

- **Componenti reali, server simulato.** Le colonne usano gli stessi
  componenti della console (`Risposte`, `Proposal`, `SendDialog`) e la stessa
  schermata del player (`PlayerView`: elenco inject, thread, chiarimenti,
  conversazioni collegate, composizione della risposta con suggerimenti AI).
  Il server è un finto server in memoria (`crates/frontend/src/bench/fake.rs`)
  che risponde agli stessi percorsi e con le stesse forme JSON. Niente HTTP,
  niente `exercise`, niente simulatore, niente LLM: i suggerimenti AI sono
  costanti. Ogni verifica di azione risulta valida (non c’è simulatore).
- **Flussi piccoli e indipendenti** (`bench/workflows.rs`), 3–4 passi ciascuno:
  1. *Inject → narrativa*: EXCON invia, il Sindaco apre e risponde con una
     narrativa, EXCON approva in Risposte.
  2. *Suggerimenti AI al player*: la Polizia scrive la narrativa, arrivano i
     suggerimenti automatici, ne associa uno (una sola azione per risposta),
     indica il fondamento e invia; EXCON vede l’azione.
  3. *EXCON: suggerisci azioni*: su una narrativa senza azioni EXCON chiede i
     suggerimenti, li aggiunge, approva; il Sindaco vede l’esito.
  4. *Chiarimento*: EXCON chiede, il Sindaco risponde nel thread, EXCON chiude.
  5. *Conversazione tra player*: Sindaco e Polizia discutono l’inject, la
     risposta nasce dalla conversazione, EXCON la ispeziona nel visore delle
     conversazioni e decide.
  6. *Decisione sulle azioni*: EXCON approva, approva in parte, modifica o
     rifiuta due azioni; la Polizia vede l’esito per azione.
- Lo stato dei passi è calcolato dal finto server e dai campi del player, mai
  memorizzato. «Ricomincia» riparte dal seme del flusso; cambiare scheda
  riparte dal seme del nuovo flusso. La lingua (IT/EN) è quella della console
  EXCON (`i18n_excon`), condivisa con la console in questo browser.
- Attributi stabili per l’automazione: `data-bench=<id>`, `data-bench-tab`,
  `data-bench-step=<n>` con `data-status=pending|active|done`,
  `data-bench-col=excon|<ruolo>`, `data-bench-send`, `data-bench-reset`,
  `data-bench-lang`.

## Avvio

Feature cargo `bench`, disattivata per default nelle build server; la build
standalone (`scripts/build_standalone.sh`, anche su Pages) la include, quindi
`?bench=1` funziona anche nelle versioni pubblicate.

```bash
./scripts/dev_bench.sh                 # Trunk + live reload, standalone+bench
# → http://127.0.0.1:8080/?bench=1     (un flusso: &b=inject|suggest|excon-suggest|clarify|conversation|decide)
PORT=8091 ./scripts/dev_bench.sh       # altra porta

TTX_BENCH=1 ./scripts/build_frontend.sh   # build server con bench.html
cargo run --release -p app -- --headless  # → http://127.0.0.1:8080/bench.html
```

Sul server la pagina non usa il backend: il finto server gira nel browser.
`./scripts/build_frontend.sh` senza `TTX_BENCH` rimuove `bench.html`.

## Verifica

```bash
cargo test -p frontend --features bench          # finto server e passi
TTX_BENCH_URL=http://127.0.0.1:8080/ node scripts/qa/bench.cjs
```

`scripts/qa/bench.cjs` percorre ogni flusso con i pulsanti reali e controlla
che ogni `[data-bench-step]` arrivi a `done`; salva le schermate in
`runs/bench-review/` (`TTX_QA_OUT`, `TTX_BENCH_SHOT` per la panoramica).

## Aggiungere un flusso

Un `BenchDef` in `workflows.rs`: colonne, seme (`fn(&mut Fake)`), passi con la
loro condizione. Se il flusso usa una richiesta nuova, aggiungerla a
`Fake::handle` con la stessa forma JSON del server. Se una parte della UI è
ancora un metodo di `App`, estrarla come in `player_view.rs` invece di copiarla.
