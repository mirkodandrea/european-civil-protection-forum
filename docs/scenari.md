# Scenari

Uno scenario è un **pack** (piccolo, modificabile) sopra un **territorio** (grande, in sola lettura).

- **Territorio**: `data/scenarios/<id>.json`. Griglia, combustibili, edifici, strade, settori. Non si modifica dalla UI.
- **Pack**: `data/scenarios/<id>.pack.json`. Dice quale territorio usa (`base`) e cosa cambia: nome, ora di inizio, meteo, inneschi, luoghi, tipi di risorsa, ruoli, rinforzi, inject (MSEL), azioni pianificate (`timeline`), politica tattica, modello operativo, base di partenza delle risorse. Un campo assente in `edits` resta quello del territorio. `run` fissa seme, realizzazioni, soglia e modello operativo; le opzioni da riga di comando hanno la precedenza.

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

## Istante degli inject (`at_s`)

Un inject può avere `at_s` (secondi simulati dall'inizio dell'esercitazione). Senza `at_s` è pianificato all'inizio del suo `turn` suggerito, contato con la durata del turno **dello scenario** (`turn_minutes`, vedi sotto), non con quella corrente: se EXCON cambia la durata durante l'esercitazione gli inject pianificati non si spostano. Quando il tempo lo raggiunge, la console lo propone a EXCON, che lo invia a mano. Nell'editor l'orario di ogni inject è modificabile (HH:MM, minuti dall'inizio, ±5 min, trascinamento); quando lo si modifica diventa esplicito (`at_s`) e `turn` viene ricavato dall'orario.

## Durata del turno (`turn_minutes`)

`edits.turn_minutes` (1–1440, assente = 30) fissa la durata di un turno per lo scenario; `Scenario.turn_minutes` la porta nel modello e `Exercise` la usa come durata iniziale. EXCON può comunque cambiarla in esercitazione (`POST /api/master/settings`): cambia solo la finestra dei turni successivi. L'editor la mostra in Generale («Durata del turno»). Un pack esportato per intero la include; i pack esistenti senza il campo restano a 30 minuti.

## Scheda Timeline dell'editor

Una sola lista cronologica con inject (●) e azioni pianificate (◆) divisi in fasce di turno («Turno 2 · 13:30–14:00», con il conteggio per fascia; le fasce vuote restano visibili, le serie di più di due turni vuoti si comprimono in una riga «Turni 3–9»). Ogni riga mostra l'orario del quadrante (`Day N HH:MM`) e i minuti dall'inizio; un orario in corsivo con «~» è implicito (inject senza `at_s`). «Rendi espliciti i tempi» scrive `at_s` in tutti gli inject che non lo hanno; «Ordina per orario» riordina le due liste (a parità di orario gli inject precedono le azioni, poi vale l'ordine della lista).

Trascinamento di una riga: sull'intestazione di una fascia = inizio della fascia; sulla metà superiore di una riga = orario della riga precedente se è nella stessa fascia, altrimenti inizio della fascia; sulla metà inferiore = stesso orario della riga bersaglio, dopo di essa. Durante il trascinamento la riga bersaglio mostra la linea di rilascio e l'orario risultante. In alternativa: campo HH:MM, minuti dall'inizio, ±5 min, «Inizio turno» o Alt+↑/↓ sulla riga selezionata. Il turno di un inject è sempre ricavato dall'orario. Un'azione con `after` mostra «↳ segue I-01» e un avviso se cade prima di quell'inject (a pari orario va bene: l'inject precede). «Aggiungi» inserisce con orario esplicito all'inizio della fascia selezionata (clic sull'intestazione) o in coda alla timeline.

## Azioni pianificate (`timeline`)

Accanto agli inject, un pack può avere `edits.timeline`: cambiamenti del mondo non legati a un messaggio. Ogni voce ha `id`, `title`, `at_s` (secondi simulati dall'inizio dell'esercizio, non dall'ora del giorno: con `start_clock_s` 01:00, `at_s` 600 cade alle 01:10), `command` (un comando EXCON di `sim::Command`, ad esempio `{"type":"SetWeather","wind_dir_deg":270}`), `after` (inject a cui è collegata, facoltativo) e `note`.

A ogni avanzamento del tempo, il server cerca le azioni ancora in attesa il cui istante cade prima della fine del turno (anche quelle in ritardo). La console apre un modale con l'elenco e per ciascuna chiede **Esegui** (all'istante previsto, o subito se già passato), **Salta** o **Rinvia** (ricompare al turno dopo). Senza una decisione per ogni azione dovuta il turno non parte. Le azioni approvate passano per `direct_command` (come un intervento EXCON) al passo di simulazione giusto. Stati mostrati nella timeline: in attesa, approvata (decisa «Esegui», in attesa del suo istante durante la risoluzione), eseguita, fallita, saltata, rinviata. Un'azione il cui istante è già passato mostra «Esegui subito» e parte al primo passo. Il rollback a un turno dimentica le decisioni prese da quel momento in poi.

API: `GET /api/master/due-actions?minutes=N` (o `?to_s=T`); `POST /api/master/resolve {minutes?, substep?, until?, actions:{id:"run"|"skip"|"defer"}}` (le decisioni si prendono a ogni avanzamento che attraversa l'azione); `POST /api/master/resolve/stop` (ferma l'avanzamento in corso, il turno resta aperto); `GET /api/master` espone `timeline_actions` con lo stato di ogni voce.

Gli scenari forniti (`valverde_factory` e il pack `valverde_night_wind`) non hanno `initial_ignitions`: l'innesco è l'azione `A-01` (e `N-A1` nel pack notturno) a `at_s` 0, collegata all'inject che lo annuncia. Il fuoco parte quando EXCON approva «Esegui» al primo «Avanza turno»; prima mappa e vista 3D non mostrano fuoco. Chi risolve il turno da script deve passare la decisione, ad esempio `{"minutes":30,"actions":{"A-01":"run"}}` (vedi `scripts/demo_session.sh`).
