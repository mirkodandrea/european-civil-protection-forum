# Banco simulatore

Pagina di sviluppo dello standalone per iterare sulle azioni EXCON nel
simulatore: a sinistra la vista 3D, a destra la console Controllo dell’EXCON
(metà schermo ciascuna). Il tempo si avvia, si mette in pausa o avanza a salti;
gli strumenti EXCON si applicano in qualsiasi momento, anche mentre il tempo
scorre.

- **Esercitazione vera, nel browser.** Stesso scenario e stesso motore dello
  standalone (`store::scenarios`, modello operativo attivo), turno 1 aperto
  all’avvio. La sessione salvata del TTX in questo browser non viene letta né
  sovrascritta (`store::freeze`).
- **Console EXCON.** È il componente `App` dell’EXCON sulla pagina Controllo
  (`controllo.rs`): innesco, linee, lanci, spegnimenti, evacuazioni, meteo,
  missioni, comandi JSON… La pagina installa la propria esercitazione con
  `standalone::install`, così le richieste della console vanno a quella.
- **Tempo.** Barra in alto: ▶/⏸ usa il clock del turno
  (`Exercise::configure_clock`) con il rapporto scelto (1×…900×); «+N min»
  avanza a passi di un minuto (`AdvanceTime`), così la vista 3D segue il salto.
  I salti sono disattivati mentre il clock corre.
- Scenario dal menu in alto (ricarica la pagina con `&scn=<id>`); «Ricomincia»
  riparte dallo stesso scenario.
- Attributi per l’automazione: `data-simbench`, `data-simbench-clock`,
  `data-simbench-play|pause`, `data-simbench-step=<min>`; la console ha i suoi.

## Avvio

```bash
trunk serve --skip-version-check --config crates/frontend/Trunk.toml --features standalone
# → http://127.0.0.1:8080/?simbench=1   (scenario: &scn=<id>)
```

Trunk osserva solo `crates/frontend`: dopo modifiche a `sim` o `exercise`
toccare un file del frontend per ricompilare.
