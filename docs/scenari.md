# Scenari

Uno scenario è un **pack** (piccolo, modificabile) sopra un **territorio** (grande, in sola lettura).

- **Territorio**: `data/scenarios/<id>.json`. Griglia, combustibili, edifici, strade, settori. Non si modifica dalla UI.
- **Pack**: `data/scenarios/<id>.pack.json`. Dice quale territorio usa (`base`) e cosa cambia: nome, ora di inizio, meteo, inneschi, luoghi, tipi di risorsa, ruoli, rinforzi, inject (MSEL, ciascuno con le sue azioni), politica tattica, modello operativo, base di partenza delle risorse. Un campo assente in `edits` resta quello del territorio. `run` fissa seme, realizzazioni, soglia e modello operativo; le opzioni da riga di comando hanno la precedenza.

Nella UI EXCON, vista **Scenario**: elenco, personalizza/duplica, modifica (Generale, Ruoli, Risorse, Inneschi con mappa, Rinforzi, Luoghi, Inject, Avanzate), controlla, salva, avvia. «Avvia» costruisce una nuova esercitazione dal pack e scarta la sessione in corso. Salvare non tocca l'esercitazione in corso. Un pack salvato dall’editor contiene tutti i campi operativi; continua a richiedere il territorio indicato da `base`.

## Dove vivono i pack

| Modalità | Pack | Scelta |
|---|---|---|
| Server (`ttx`) | file `*.pack.json` in `--scenarios-dir` (default: cartella di `--scenario`) | `runs/state/active.txt` |
| Standalone (WASM) | `localStorage` (`ttx.standalone.packs`) più i pack incorporati | `ttx.standalone.active` |

La sessione salvata contiene il pack da cui è partita: al riavvio si ricostruisce lo stesso scenario anche se la libreria è cambiata.
Il browser incorpora **Valverde** (`valverde_factory`, 4 × 4 km) e **Selvariva** (`selvariva_basin`, 6 × 6 km), con i pack `valverde_night_wind` e `selvariva_festival`. La libreria carica sempre la base dichiarata dal pack e rifiuta basi non incluse. Il filtro «Territorio» distingue le mappe; «Personalizza» sul territorio crea uno scenario sulla mappa scelta. Il territorio di una copia resta fisso: cambiare solo l’ID della base non trasferisce luoghi, coordinate e riferimenti.

I salvataggi browser nuovi registrano anche l’identità dei dati del territorio: un replay su una mappa modificata viene scartato con avviso. I salvataggi precedenti senza impronta restano leggibili. Il 3D locale ricostruisce la scena quando cambia scenario; il viewer remoto scarica la nuova scena fuori dal ciclo di rendering quando cambia territorio.

Per aggiungere un territorio al server basta un nuovo `<id>.json` valido nella libreria; per includerlo anche nello standalone aggiungerlo a `crates/scenario/src/embedded.rs`, con eventuali pack, e ricostruire il browser. La feature `scenario/embedded` evita di incorporare i raster negli strumenti headless che non ne hanno bisogno.

La traccia di conduzione da un’ora, le assunzioni e i comandi di rigenerazione di Selvariva sono in [scenario-selvariva.md](scenario-selvariva.md).

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

Una sola lista cronologica di inject (●) divisi in fasce di turno («Turno 2 · 13:30–14:00», con il conteggio per fascia; le fasce vuote restano visibili, le serie di più di due turni vuoti si comprimono in una riga «Turni 3–9»). Ogni riga mostra l'orario del quadrante (`Day N HH:MM`) e i minuti dall'inizio; un orario in corsivo con «~» è implicito (inject senza `at_s`). «Rendi espliciti i tempi» scrive `at_s` in tutti gli inject che non lo hanno; «Ordina per orario» riordina gli inject (a parità di orario vale l'ordine della lista). Le azioni non sono righe: ogni riga di inject mostra «◆ N azioni (M prima)» e le azioni si modificano nel dettaglio dell'inject, sezione **Azioni** (vedi sotto).

Trascinamento di una riga: sull'intestazione di una fascia = inizio della fascia; sulla metà superiore di una riga = orario della riga precedente se è nella stessa fascia, altrimenti inizio della fascia; sulla metà inferiore = stesso orario della riga bersaglio, dopo di essa. Durante il trascinamento la riga bersaglio mostra la linea di rilascio e l'orario risultante. In alternativa: campo HH:MM, minuti dall'inizio, ±5 min, «Inizio turno» o Alt+↑/↓ sulla riga selezionata. Il turno di un inject è sempre ricavato dall'orario. Un'azione anticipata che cadrebbe prima dell'inizio dell'esercitazione mostra un avviso. «Aggiungi inject» inserisce con orario esplicito all'inizio della fascia selezionata (clic sull'intestazione) o in coda alla timeline.

## Azioni degli inject (`actions`)

Le azioni non sono eventi separati: appartengono a un inject. Ogni inject del pack può avere `actions`, una lista di voci con `id` (unico in tutto lo scenario), `title`, `command` (un comando EXCON di `sim::Command`, ad esempio `{"type":"SetWeather","wind_dir_deg":270}`), `offset_s` (secondi, anche negativo; predefinito 0) e `note`.

- **`offset_s` ≥ 0**: l'azione parte perché l'inject viene inviato. Con 0 passa subito per `direct_command` (come un intervento EXCON) al momento dell'invio; con un valore positivo viene registrata come *approvata* con istante `invio + offset_s` e applicata al passo di simulazione giusto durante i prossimi avanzamenti.
- **`offset_s` < 0** (*azione anticipata*): precede l'inject. L'istante è `ora prevista dell'inject + offset_s` (negativo = prima). Parte da sola durante l'avanzamento del tempo (la simulazione si ferma esattamente a quell'istante), a meno che EXCON l'abbia disattivata (interruttore «esegui» nella riga della timeline o nella finestra di revisione prima dell'avanzamento) oppure abbia scelto «Non inviare» per l'inject prima di quell'istante. Se l'inject viene inviato in anticipo, le sue azioni anticipate ancora in attesa partono all'invio.
- Nella finestra di invio dell'inject (e nel corpo di `POST /api/master/msel/<id>/send {recipients?, actions?}`) l'EXCON sceglie quali azioni eseguire: per impostazione predefinita tutte (le anticipate disattivate restano spente). Le azioni si decidono con il **primo** invio: inviare lo stesso inject ad altri ruoli non le riesegue. Un inject personalizzato porta le proprie azioni nel corpo di `POST /api/master/inject {…, actions:[…]}` (solo `offset_s` ≥ 0: non ha un'ora prevista).
- Stato mostrato come sotto-elemento dell'inject nella timeline: pianificata, approvata alle HH:MM, eseguita, fallita, non eseguita, annullata (inject saltato). Il rollback a un turno dimentica le esecuzioni, gli interruttori e i «Non inviare» decisi da quel momento in poi.
- **Non inviare**: sull'inject raggiunto («da gestire») i pulsanti **Invia** e **Non inviare**; «Ripristina» annulla la scelta. Un inject saltato non blocca «Avanza al prossimo inject» (`POST /api/master/msel/<id>/skip` e `/restore`).

Formato precedente: un pack può ancora avere `edits.timeline` (e un territorio `timeline`) con azioni autonome `{id, title, at_s, command, after?, note}`. Al caricamento vengono convertite e il campo non è più scritto: con `after` l'azione passa a quell'inject; senza `after` passa all'ultimo inject pianificato a `at_s` o prima di esso, altrimenti al primo inject; `offset_s = at_s − ora prevista dell'inject` (negativo se l'azione cadeva prima: diventa un'azione anticipata). L'errore è esplicito se non esiste alcun inject cui agganciarla.

Gli scenari forniti (`valverde_factory` e il pack `valverde_night_wind`) non hanno `initial_ignitions`: l'innesco è l'azione `A-01` (e `N-A1` nel pack notturno) dell'inject `I-01` (`N-01`), con `offset_s` 0: il fuoco parte quando EXCON invia l'inject che lo annuncia; prima mappa e vista 3D non mostrano fuoco. Chi guida l'esercitazione da script invia l'inject (`POST /api/master/msel/I-01/send`) prima di avanzare (vedi `scripts/demo_session.sh`).

API: `POST /api/master/resolve {minutes?, substep?, until?: "next_inject"|"turn_end"}` (`next_event` è un alias di `next_inject`); `POST /api/master/resolve/stop` (ferma l'avanzamento in corso, il turno resta aperto); `GET /api/master` espone `planned_injects` (con `actions`, `skipped`) e `inject_actions` (ogni esecuzione con il suo stato).


## Editor scenario 3D

L’editor 3D modifica una **bozza di scenario completo**, incluso il territorio.
È separato dalla simulazione: non avvia HTTP o AI e non applica interventi
all’esercitazione in corso. La vista Scenario descritta sopra resta l’editor dei
pack sopra un territorio; il nuovo ambiente serve anche ad authoring geometrico.

- **Web standalone:** dalla vista Scenario, «Apri editor scenario 3D», oppure
  aprire `?editor=1`. Richiede una build standalone
  (`./scripts/build_frontend.sh --features standalone`).
- **App Bevy:** `cargo run --release -p app --bin scenario-editor`.
  Opzioni: `--scenarios-dir DIR` (default `data/scenarios`) e `--scenario ID`.
  La libreria comprende territori e pack, risolti sulla propria base.

«Crea territorio vuoto» costruisce una griglia piatta senza edifici, strade,
ruoli o inject ereditati. Si scelgono identificativo, celle per lato e metri per
cella. «Apri scenario esistente» carica un territorio o il contenuto completo di
un pack; «Clona scenario corrente» assegna alla copia un nuovo ID e conserva
territorio e contenuti dell’esercitazione. Aprire un altro progetto con modifiche
non salvate richiede una scelta esplicita nell’interfaccia.

Gli strumenti lavorano con un clic sulla superficie 3D: quota assoluta e
vegetazione con pennello circolare, edifici, luoghi, inneschi (raggio minimo
40 m), strade con due estremi. Gli oggetti si scelgono dalla lista o sulla mappa;
le proprietà permettono di cambiare coordinate e attributi, oppure eliminarli.
Le strade sono selezionabili dalla lista. «Scenario e meteo» modifica nome,
descrizione, orario iniziale, durata dei turni e meteo. Importazione/esportazione
JSON (download nel browser, copia negli appunti nel desktop) trasferisce l’intero scenario e permette di modificare anche ruoli,
risorse, inject e politiche avanzate. Il formato è quello dei territori `.json`,
non quello dei pack `.pack.json`. Nel browser è possibile usare poi l’editor
pack ordinario sul nuovo territorio per i moduli di ruoli, risorse e timeline.

Annulla/ripristina conserva fino a 32 revisioni della bozza. Una modifica non
valida lascia intatti documento e cronologia. I pennelli di quota aggiornano
anche l’eventuale terreno visuale ad alta risoluzione. Ogni modifica confermata
ricostruisce la scena in sola lettura, preservando la camera a parità di griglia;
l’authoring di territori grandi può quindi richiedere un’attesa.

Salvataggio nativo: `<scenarios-dir>/<id>.json`, con sostituzione atomica.
Un ID già usato da un pack richiede un nuovo identificativo. Salvataggio web:
`localStorage`, chiave `ttx.standalone.territories`; i territori compaiono nella
libreria ordinaria e possono essere avviati o usati da nuovi pack. Gli ID inclusi
nell’app non sono sovrascrivibili nel browser: clonare prima. Gli errori di quota
del browser sono mostrati e non segnano la bozza come salvata. Per trasferire un
territorio tra browser e nativo usare il JSON completo. La sessione browser
verifica anche l’impronta dei territori creati dall’editor prima del ripristino.

Verifica grafica invisibile, senza avviare l’esercitazione:

```sh
cargo build --release -p app --bin scenario-editor
TTX_SNAPSHOT="$PWD/runs/editor.png" TTX_SCALE=1 target/release/scenario-editor
```
