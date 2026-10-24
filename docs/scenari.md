# Scenari

Uno scenario è un **pack** (piccolo, modificabile) sopra un **territorio** (grande, in sola lettura).

- **Territorio**: `data/scenarios/<id>.json`. Griglia, combustibili, edifici, strade, settori. Non si modifica dalla UI.
- **Pack**: `data/scenarios/<id>.pack.json`. Dice quale territorio usa (`base`) e cosa cambia: nome, ora di inizio, meteo, inneschi, luoghi, tipi di risorsa, ruoli, rinforzi, inject (MSEL), politica tattica, modello operativo, base di partenza delle risorse. Un campo assente in `edits` resta quello del territorio. `run` fissa seme, realizzazioni, soglia e modello operativo; le opzioni da riga di comando hanno la precedenza.

Nella UI EXCON, vista **Scenario**: elenco, personalizza/duplica, modifica (Generale, Ruoli, Risorse, Inneschi con mappa, Rinforzi, Luoghi, Inject, Avanzate), controlla, salva, avvia. «Avvia» costruisce una nuova esercitazione dal pack e scarta la sessione in corso. Salvare non tocca l'esercitazione in corso. Un pack salvato dall'editor contiene tutti i campi, quindi è autonomo rispetto al territorio.

## Dove vivono i pack

| Modalità | Pack | Scelta |
|---|---|---|
| Server (`ttx`) | file `*.pack.json` in `--scenarios-dir` (default: cartella di `--scenario`) | `runs/state/active.txt` |
| Standalone (WASM) | `localStorage` (`ttx.standalone.packs`) più i pack incorporati | `ttx.standalone.active` |

La sessione salvata contiene il pack da cui è partita: al riavvio si ricostruisce lo stesso scenario anche se la libreria è cambiata.
Nel browser il territorio incluso è `valverde_factory`; per usarne altri servono altri build.

## Da riga di comando

```bash
cargo run --release -p app -- --scenario data/scenarios/valverde_night_wind.pack.json
cargo run --release -p app -- --scenarios-dir miei_scenari --scenario mio_territorio
```

## API (solo EXCON)

`GET /api/master/scenarios`, `GET /api/master/scenarios/<id>` (pack completo + nomi del territorio), `POST …/validate`, `…/save`, `…/delete`, `…/activate` (solo server). Tutti i problemi di un pack tornano insieme (400, uno per riga).

## Cosa non è più cablato

Base di partenza delle risorse (`staging_base`), velocità per risorsa (`speed_mps`), risorse e azioni che il modello operativo aggiunge a un ruolo (`operational_grants`), esempi del catalogo comandi per l'AI (derivati dallo scenario).
Gli scenari senza questi campi si comportano come prima.
