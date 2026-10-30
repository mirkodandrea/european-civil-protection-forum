# Scenari

Uno scenario è un **pack** (piccolo, modificabile) sopra un **territorio** (grande, in sola lettura).

- **Territorio**: `data/scenarios/<id>.json`. Griglia, combustibili, edifici, strade, settori. Non si modifica dalla UI.
- **Pack**: `data/scenarios/<id>.pack.json`. Dice quale territorio usa (`base`) e cosa cambia: nome, ora di inizio, meteo, inneschi, luoghi, tipi di risorsa, ruoli, rinforzi, inject (MSEL, ciascuno con le sue azioni), politica tattica, modello operativo, base di partenza delle risorse. Un campo assente in `edits` resta quello del territorio. `run` fissa seme, realizzazioni, soglia e modello operativo; le opzioni da riga di comando hanno la precedenza.

Nella UI EXCON, vista **Scenario**: elenco, personalizza/duplica, modifica (Generale, Ruoli e risorse con rinforzi, Timeline con inject e azioni — inneschi inclusi —, Avanzate; i luoghi sono definiti nel territorio), controlla, salva, avvia. «Avvia» costruisce una nuova esercitazione dal pack e scarta la sessione in corso. Salvare non tocca l'esercitazione in corso. Un pack salvato dall’editor contiene tutti i campi operativi; continua a richiedere il territorio indicato da `base`.

## Dove vivono i pack

| Modalità | Pack | Scelta |
|---|---|---|
| Server (`ttx`) | file `*.pack.json` in `--scenarios-dir` (default: cartella di `--scenario`) | `runs/state/active.txt` |
| Standalone (WASM) | `localStorage` (`ttx.standalone.packs`) più i pack incorporati | `ttx.standalone.active` |

La sessione salvata contiene il pack da cui è partita: al riavvio si ricostruisce lo stesso scenario anche se la libreria è cambiata.
Il browser incorpora **Valverde** (`valverde_factory`, 4 × 4 km), **Selvariva** (`selvariva_basin`, 6 × 6 km) e l'**Isola di Roccavento** (`roccavento_island`, 8 × 8 km), con i pack `valverde_night_wind`, `selvariva_crisi_integrata` e `roccavento_valico`. La libreria carica sempre la base dichiarata dal pack e rifiuta basi non incluse. Il filtro «Territorio» distingue le mappe; «Personalizza» sul territorio crea uno scenario sulla mappa scelta. Il territorio di una copia resta fisso: cambiare solo l’ID della base non trasferisce luoghi, coordinate e riferimenti.

I salvataggi browser nuovi registrano anche l’identità dei dati del territorio: un replay su una mappa modificata viene scartato con avviso. I salvataggi precedenti senza impronta restano leggibili. Il 3D locale ricostruisce la scena quando cambia scenario; il viewer remoto scarica la nuova scena fuori dal ciclo di rendering quando cambia territorio.

Per aggiungere un territorio al server basta un nuovo `<id>.json` valido nella libreria; per includerlo anche nello standalone aggiungerlo a `crates/scenario/src/embedded.rs`, con eventuali pack, e ricostruire il browser. La feature `scenario/embedded` evita di incorporare i raster negli strumenti headless che non ne hanno bisogno.

La traccia di conduzione in cinque turni, la visibilità della mappa, le assunzioni e i comandi di rigenerazione di Selvariva sono in [scenario-selvariva.md](scenario-selvariva.md). Roccavento, i suoi borghi e il comando «Evacua borgo» sono in [scenario-roccavento.md](scenario-roccavento.md).

## Edifici: tipo e aspetto (`kind`, `visual_kind`)

`kind` dice cosa fa la simulazione con l’edificio e ha tre valori. `house` (abitazione): contata nei totali del settore, elencata singolarmente solo se ha un nome o è colpita, occupanti evacuano in auto. `sensitive` (struttura sensibile: scuole, ospedali, RSA): gli occupanti non evacuano da soli e servono bus, sempre elencata. `facility` (altra struttura): sempre elencata, occupanti in auto. `visual_kind` («Aspetto» nell’editor: `school`, `town_hall`, `church`, `mill`…) cambia solo modello 3D ed etichette (categoria nelle tabelle di situazione, nomi evidenziati sulla mappa); capienza da rifugio (`shelter_capacity`) e occupanti sono campi a sé. I vecchi tipi (`school`, `hospital`, `fuel_station`, `sports_centre`, `town_hall`, `industrial`) si caricano ancora: diventano `sensitive` o `facility` e, se manca, l’aspetto prende il vecchio nome; al salvataggio esce il formato nuovo.

## Borghi (`districts`)

Un territorio può dichiarare i suoi borghi: `districts` (`id`, `name`, `buildings` per id, `gathering` facoltativa, nome di un edificio con capienza) e il ritmo `district_evacuation` (`per_min` persone al minuto, `prep_min` minuti di preparazione; predefiniti 6 e 5). Con i borghi è disponibile il comando `EvacuateDistrict` (vedi [scenario-roccavento.md](scenario-roccavento.md#evacua-borgo-evacuatedistrict)). Senza borghi il comando è rifiutato e nulla cambia. Un pack può sostituire `districts` e `district_evacuation` (campi `edits`); nell’editor la scheda «Borghi» li modifica (una casa appartiene a un solo borgo), con area di raccolta e ritmo delle partenze. La mappa parte inquadrata sulle case dei borghi e si usa così: clic su una casa la aggiunge o la toglie; trascinando si disegna un rettangolo che aggiunge le case comprese (Maiusc: le toglie; con «Solo case senza borgo», attivo di default, non ruba case agli altri borghi); «Aggiungi zona…» aggiunge le case di una zona della griglia; rotella o `+`/`−` ingrandiscono, `Sposta` (o Alt, o il tasto centrale) sposta la mappa, `0` o «Adatta» inquadra tutto; i tasti `1`–`9` scelgono il borgo e Ctrl+Z (o «↶ Annulla») annulla l’ultima modifica delle case. Sulla mappa ogni borgo ha il suo inviluppo convesso colorato, il nome, e il borgo scelto mostra l’area di raccolta; passando sopra una casa un suggerimento dà nome, persone, capienza e borgo, e una riga di stato dice cosa è cambiato (anche «spostata da … a …»). L’identificativo segue il nome finché non lo si cambia a mano.

### Controlli dell’editor

Mentre si modifica, lo scenario viene verificato in automatico (con una breve attesa dopo l’ultima modifica): le schede con problemi mostrano un contatore rosso, l’elenco in fondo permette di aprire la scheda interessata con «→ scheda», e nella scheda Borghi l’errore compare accanto al borgo. «Controlla» rilancia la verifica a mano e «Salva» mostra «Salvato ✓» quando non ci sono modifiche in sospeso. «Controlla» segnala borghi vuoti, duplicati, case sconosciute e aree di raccolta senza capienza.

## Da riga di comando

```bash
cargo run --release -p app -- --scenario data/scenarios/valverde_night_wind.pack.json
cargo run --release -p app -- --scenarios-dir miei_scenari --scenario mio_territorio
```

## API (solo EXCON)

`GET /api/master/scenarios`, `GET /api/master/scenarios/<id>` (pack completo + nomi del territorio), `POST …/validate`, `…/translate` (`{pack, language}` → etichette della mappa mancanti tradotte dall'AI, niente salvato), `…/save`, `…/delete`, `…/activate` (solo server). Tutti i problemi di un pack tornano insieme (400, uno per riga).

## Testi in più lingue (`translations`)

I contenuti dello scenario (nomi e descrizioni di ruoli e risorse, titoli e mittenti
degli inject) sono scritti in inglese. Ogni lettore li vede nella propria lingua
(scelta a inizio sessione) con questa precedenza:

1. `translations` dello scenario: `{"it": {"Buses": "Autobus", ...}}`, lingua → testo
   inglese di origine → traduzione. Vale per ogni punto in cui compare la stringa
   intera (corrispondenza esatta, non parziale). Un pack può aggiungerne in
   `edits.translations` (stessa forma), unite sopra quelle del territorio: così fanno
   `selvariva_crisi_integrata` e `valverde_night_wind`, scritti in inglese con il testo
   italiano originale in `translations.it` (anche note, obiettivi e testi degli inject,
   che la console EXCON italiana mostra senza LLM).
2. Glossario incorporato (`crates/scenario/src/glossary.rs`): ruoli e risorse standard
   in tutte le lingue dei giocatori; testi completi della Valverde in italiano (azioni
   degli inject comprese).

I nomi di luoghi, edifici e strade dei territori restano quelli della mappa (anche se
italiani, es. «Area festival delle Fonti»): sono chiavi con cui comandi e pack salvati
ritrovano i luoghi, quindi non si traducono.
3. Traduzione LLM a runtime (solo giocatori), una volta per testo e lingua, in cache;
   include i titoli degli inject scritti dall'EXCON.
4. Senza LLM e senza voce: resta il testo dello scenario.

La sostituzione avviene sul server (`display_map` in `crates/server/src/lib.rs`) nelle
viste di giocatore, `/api/static`, `/api/state` e `/api/master`; mai nell'editor scenario.
Un ruolo scritto nella lingua del lettore non viene tradotto.

### Etichette della mappa

I nomi disegnati sulle mappe (luoghi, edifici con nome, strade, borghi, settori:
`Scenario::map_texts`) hanno traduzioni **statiche** nella stessa `translations`, una
tabella per lingua, inglese compreso (i territori Selvariva e Roccavento sono scritti
in italiano). Regole:

- Solo i **giocatori** le vedono: l'EXCON legge i nomi originali, che sono quelli che
  i comandi usano. Le etichette non sono mai tradotte a runtime.
- Il gazetteer dell'LLM aggiunge come `aliases` i nomi nelle lingue dei giocatori
  (`Exercise::gazetteer`), così una risposta che li usa si risolve nel nome originale.
- Territori inclusi: le ricette della factory (`scripts/scenario_factory.py landscape |
  selvariva | roccavento`) chiamano `translations.fill` prima di scrivere: riportano le
  traduzioni del file precedente il cui testo è ancora nel territorio e fanno tradurre
  all'LLM le etichette mancanti (binario `translate-territory`, chiave in `.env`).
  `--no-translate` lavora offline e segnala solo le etichette scoperte. `gen-scenario`
  (`synthetic_valley.json`) riporta le traduzioni ma non chiama l'LLM. Per completare
  un territorio esistente: `python3 scripts/factory/translations.py [TERRITORIO …]
  [--lang xx …]`, che aggiunge solo le mancanti e aggiorna il manifest.
- Editor scenario, scheda **Traduzioni**: copertura per lingua dei ruoli, tabella
  modificabile nome originale → traduzione, «Genera le mancanti (AI)» per una lingua o
  per tutte le lingue dei ruoli (`POST /api/master/scenarios/translate`). Le voci
  generate restano modificabili e finiscono nel pack (`edits.translations`, che se
  presente sostituisce quella del territorio) al salvataggio.

## Terminologia per i giocatori

Nelle pagine dei giocatori «inject» diventa «messaggio» (it/en e le 10 lingue di
`i18n_help.rs`); «Traduzione letterale» diventa «Tradotto automaticamente» con un
suggerimento che lo spiega. L'EXCON mantiene «inject» (termine MSEL). JSON e dati
tecnici stanno solo dietro sezioni «Avanzate» o «dati tecnici (JSON)»; lo «Spotting»
è spiegato come «braci che accendono nuovi focolai». Limite noto: le chiavi
dell'interfaccia giocatore fuori da `i18n_lang`, `i18n_function` e `i18n_help` restano
in inglese per le lingue diverse da it/en.

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

### Azioni di rivelazione (`reveal`)

Un'azione può, invece di un `command`, rivelare **conoscenza sulla mappa** ai destinatari dell'inject (nebbia di guerra, `crates/exercise/src/knowledge.rs`). Non è stato del mondo: la esegue `Exercise`, mai `World::apply`. Esattamente uno tra `command` e `reveal`; `offset_s` deve essere ≥ 0 (un `reveal` anticipato è rifiutato al caricamento). Con `offset_s` 0 l'osservazione viene allegata ai messaggi consegnati dell'inject (uno per destinatario); con `offset_s` > 0 ogni destinatario riceve a quell'istante un messaggio di situazione. Come per i comandi, si decide al primo invio dell'inject e vale per i ruoli di quell'invio.

```json
{"id":"A-02","title":"Stima dell'incendio (112)","offset_s":0,
 "reveal":{"kind":"approximate","position_error_m":400,"area_error_frac":0.3}}
{"id":"A-03","title":"Mappa del fuoco","offset_s":0,
 "reveal":{"kind":"exact","zones":"burning_and_adjacent","layers":["fire","roads"],"source":"Sala operativa"}}
```

- **`exact`**: `zones` è un elenco (`["C3","D3"]`) oppure una regola calcolata all'esecuzione, `"burning"` (zone con celle in fiamme) o `"burning_and_adjacent"` (più le otto zone vicine); `layers` tra `fire, buildings, roads, lines, units` (predefinito `["fire"]`). Usa `zone_observation`. Una regola senza zone in fiamme fa fallire l'azione ("no zone is burning").
- **`approximate`**: stima non verificata dell'incendio *reale* (celle in fiamme o bruciate, come nel raster del fuoco) calcolata dal simulatore all'esecuzione: baricentro, area (ha) e direzione di propagazione, con errore deliberato e deterministico (funzione di seme, id inject, id azione e ruolo, quindi ogni ruolo legge un errore diverso ma riproducibile): posizione entro `position_error_m` (predefinito 300, max 5000), area entro ±`area_error_frac` (predefinito 0.25) e arrotondata a due cifre significative, direzione ±10° arrotondata a 5°. La direzione è quella dal baricentro delle celle bruciate (dove il fuoco è passato) a quello delle celle in fiamme (il fronte); se distano meno di una cella (fuoco appena innescato) è il vento verso cui soffia (direzione del vento + 180°). Il giocatore vede un cerchio tratteggiato (area dichiarata + errore di posizione), una freccia e l'etichetta «fonte · ora · stima non verificata»; l'ultima stima ricevuta prevale. Non rivela zone.
- `source` (facoltativo) è la fonte mostrata; predefinita il mittente dell'inject. Nel `KnowledgeEvent` si registrano `source_name` e (per `exact`) `layers`.
- Nei pacchetti forniti: `I-01` (112) → `A-02` approssimata, `I-03` (sala operativa → fire_commander) → `A-03` esatta (`burning_and_adjacent`, fire + roads). Nell'editor Scenario un'azione `reveal` si mostra e si modifica come JSON al posto degli strumenti di comando.

**Aggiorna mappe dei ruoli** (manuale, mai automatico): nel pannello del turno compare, se almeno un ruolo ha ricevuto una rivelazione per zone (anche dallo strumento «Informazioni ai ruoli»), il pulsante che per ciascun ruolo ripete le stesse zone e gli stessi livelli dell'ultima rivelazione come nuovo snapshot (`POST /api/master/knowledge/refresh {source?}`; fonte predefinita la persona della sala operativa). I ruoli e le zone interessati sono elencati accanto al pulsante (`role_knowledge[ruolo].refresh`).

## Drone di monitoraggio (`drones`, `MonitorArea`)

Una risorsa che, finché è in posizione, tiene aggiornata la mappa del ruolo che la possiede. Gli aggiornamenti continui si guadagnano impegnando la risorsa: non sono gratuiti. Nei territori forniti (`synthetic_valley`/Valverde, `valverde_factory`, `selvariva_basin`) il `fire_commander` ha 1 drone e l'azione `MonitorArea`; un pack che non ridefinisce risorse e ruoli lo eredita dalla base. Il pack `selvariva_crisi_integrata` ridefinisce i ruoli e assegna il drone (limite di vento 45 km/h) a `direzione_soccorso`.

Risorsa (in `resource_types`; un id `drones` senza `drone` usa i valori predefiniti):

```json
{"id":"drones","name":"Monitoring drones","description":"…","speed_mps":20,
 "drone":{"max_zones":4,"endurance_s":2400,"turnaround_s":900,"update_interval_s":300,"wind_limit_kmh":50}}
```

| Campo | Predefinito | Significato |
|---|---|---|
| `max_zones` | 4 | il rettangolo che contiene le zone ordinate (griglia 8×8) non supera tanti riquadri |
| `endurance_s` | 2400 | tempo **in posizione**; il transito dalla base non conta |
| `turnaround_s` | 900 | dall'atterraggio alla base a "di nuovo disponibile" |
| `update_interval_s` | 300 | intervallo tra due aggiornamenti della mappa, tempo simulato |
| `wind_limit_kmh` | assente (nessun limite) | sopra questo vento non decolla (ordine rifiutato) e, in volo, viene richiamato |
| `speed_mps` (risorsa) | 20 | velocità di transito in linea d'aria dalla base (`staging_base`, o `SetDeploymentTravel`) |

Comando: `{"type":"MonitorArea","zones":["F3-G4"]}`. Ogni voce di `zones` è un codice (`"F3"`) o un rettangolo di zone (`"F3-G4"`); sono accettati anche nomi di luogo (la zona in cui cadono). `role` (solo EXCON) indica il possessore; senza, il primo ruolo che ha un drone. Un giocatore usa sempre il proprio e passa dal normale flusso proposta → approvazione EXCON → esecuzione (`DeployEngine`-like: partenza dalla base, transito, effetto all'arrivo). L'EXCON può comandarlo direttamente (`POST /api/master/command`, strumento «Drone di monitoraggio» in Controllo, JSON per più zone); il drone resta comunque impegnato. Un drone solo: finché non è di nuovo disponibile ogni ordine è rifiutato.

Ciclo (`sim::drone`, nel `Dyn`, quindi annullato dal rollback): *in volo* (`en_route`) → *in posizione* (`working`, campionamenti a `arrivo + k·update_interval_s` per `k = 0…`, l'ultimo a fine autonomia) → *rientro e ricarica* (`refilling`) → `completed` e allocazione restituita da sola (nessun `ReturnResource` necessario). Un `ReturnResource` dell'EXCON lo richiama subito.

Conoscenza (`exercise::monitoring`): ogni campione diventa un `KnowledgeEvent` di fonte `Monitoring` (`message` 0, `source_name` «Drone», livelli fire, buildings, roads, lines, units sulle zone coperte), senza messaggio. Solo due messaggi di situazione: «Drone in posizione» all'arrivo e «Drone rientrato» alla fine della copertura (autonomia, vento, richiamo). Il campionamento avviene ai secondi simulati esatti dentro `World::advance`, quindi non dipende dal passo con cui si avanza; l'`Exercise` li raccoglie dopo ogni avanzamento. Un rollback elimina le osservazioni `Monitoring` successive all'istante ripristinato. Lo stato per il giocatore è in `map.drones`, `map.live_zones`, `map.zone_updates`; la mappa disegna il perimetro tratteggiato delle zone in diretta e il drone, e vela leggermente le altre zone note.

## Ricognizione a terra (`Reconnoitre`)

Comando: `{"type":"Reconnoitre","zone":"F4"}` (codice di zona o nome di luogo; `role` solo per l'EXCON, che indica il ruolo informato). Impegna 1 `ground_crews` del ruolo: la squadra parte dalla base del ruolo (`staging_base` o `SetDeploymentTravel`), segue strade e terreno se il modello operativo è attivo (nessun percorso = ordine rifiutato) e all'arrivo produce **un solo** campione (`MonitorSample` con `recon: true`): un `KnowledgeEvent` di fonte `Monitoring` con `source_name` «Squadra di ricognizione»/«Reconnaissance crew», gli stessi livelli del drone sulla zona, e il messaggio «Ricognizione completata». Nessun aggiornamento continuo: per una mappa in diretta serve il drone. La squadra resta impiegata (`completed`, allocazione aperta) finché l'EXCON non conferma il rientro con `ReturnResource`. Un'osservazione successiva all'istante ripristinato da un rollback viene dimenticata come quelle del drone. Per più zone servono più ordini (la pagina del giocatore li crea uno per zona).

Formato precedente: un pack può ancora avere `edits.timeline` (e un territorio `timeline`) con azioni autonome `{id, title, at_s, command, after?, note}`. Al caricamento vengono convertite e il campo non è più scritto: con `after` l'azione passa a quell'inject; senza `after` passa all'ultimo inject pianificato a `at_s` o prima di esso, altrimenti al primo inject; `offset_s = at_s − ora prevista dell'inject` (negativo se l'azione cadeva prima: diventa un'azione anticipata). L'errore è esplicito se non esiste alcun inject cui agganciarla.

Gli scenari non hanno inneschi iniziali (il concetto `initial_ignitions` non esiste più; i pack salvati che lo contengono si caricano e il campo viene ignorato). Negli scenari forniti l'innesco è un'azione `Ignite` di un inject: l'azione `A-01` (e `N-A1` nel pack notturno) dell'inject `I-01` (`N-01`), con `offset_s` 0: il fuoco parte quando EXCON invia l'inject che lo annuncia; prima mappa e vista 3D non mostrano fuoco. Chi guida l'esercitazione da script invia l'inject (`POST /api/master/msel/I-01/send`) prima di avanzare (vedi `scripts/demo_session.sh`).

API: `POST /api/master/resolve {minutes?, substep?, until?: "next_inject"|"turn_end"}` (`next_event` è un alias di `next_inject`); `POST /api/master/resolve/stop` (ferma l'avanzamento in corso, il turno resta aperto); `GET /api/master` espone `planned_injects` (con `actions`, `skipped`) e `inject_actions` (ogni esecuzione con il suo stato).


## Editor scenario 3D

L’editor 3D modifica una **bozza di scenario completo**, incluso il territorio.
È separato dalla simulazione: non avvia HTTP o AI e non applica interventi
all’esercitazione in corso. La vista Scenario descritta sopra resta l’editor dei
pack sopra un territorio; il nuovo ambiente serve anche ad authoring geometrico.

- **Web standalone:** dalla vista Scenario, «Apri editor scenario 3D», oppure
  aprire `?editor=1`. Richiede una build standalone
  (`./scripts/build_frontend.sh --features standalone`).
- **Dalla schermata di partenza** (standalone): la scheda «Territori» elenca i territori inclusi e quelli creati nel browser. «Modifica copia» apre un territorio incluso come copia (`<id>-modificato`, non sovrascrivibile l’originale); «Modifica» riapre un territorio creato qui; «Nuovo territorio vuoto» parte da una griglia piatta. Il link è `?editor=1&territory=<id>`.
- **App Bevy:** `cargo run --release -p app --bin scenario-editor`.
  Opzioni: `--scenarios-dir DIR` (default `data/scenarios`) e `--scenario ID`.
  La libreria comprende territori e pack, risolti sulla propria base.

**Strade e oggetti.** Con lo strumento Seleziona si sceglie una strada cliccando
la sua linea. Sulla strada selezionata: trascina un vertice per spostarlo,
Maiusc+clic su un tratto per inserirne uno nuovo (e trascinarlo), `X` sul
vertice per rimuoverlo (restano almeno due vertici), «Aggiungi vertici in coda»
per prolungarla. Edifici e luoghi selezionati si spostano trascinandoli; ogni
trascinamento è un solo passo di annullamento. Lo strumento «Aggiungi edificio»
offre un catalogo di strutture (abitazione, scuola, ospedale, chiesa, caserma
VVF, cascina, silo, torre idrica, piazza, area di raccolta, porto…) con tipo,
variante grafica, dimensioni e occupanti già impostati; Q/E ruotano di 15° prima
e dopo il posizionamento.

«Crea territorio vuoto» costruisce una griglia piatta senza edifici, strade,
ruoli o inject ereditati. Si scelgono identificativo, celle per lato e metri per
cella. «Apri scenario esistente» carica un territorio o il contenuto completo di
un pack; «Clona scenario corrente» assegna alla copia un nuovo ID e conserva
territorio e contenuti dell’esercitazione. Aprire un altro progetto con modifiche
non salvate richiede una scelta esplicita nell’interfaccia.

Strumenti:

- **Quota terreno (pennello push/pull):** tenendo premuto il tasto sinistro il
  terreno si alza nel cerchio, con Maiusc si abbassa (cerchio rosso); in
  alternativa «Livella a una quota» porta il cerchio a un valore fisso.
  L'intensità è in m/s. Un tratto di pennello è un solo passo di annulla.
- **Vegetazione (pennello):** stessa logica, tenendo premuto si dipinge il
  combustibile scelto.
- **Acqua:** il terreno sotto 0 m s'allaga da solo: nel 3D compare una
  superficie d'acqua a quota 0 e le celle diventano non combustibili (il
  combustibile non bruciabile dello scenario), quindi senza vegetazione né
  fuoco. Dipingere vegetazione sul fondo non ha effetto; gli edifici
  non si possono posizionare sott'acqua. Per fare un lago basta abbassare la
  quota sotto zero (o «Livella» a un valore negativo), per un'isola rialzare
  il fondo sopra zero al centro. Le celle riportate sopra lo zero restano
  non vegetate finché non si ridipingono.
- **Raggio del pennello:** rotella del mouse (10–1000 m); Ctrl/Cmd + rotella
  per lo zoom. Con un pennello attivo il tasto sinistro non sposta la vista
  (usare il tasto destro per ruotare, il centrale per spostare).
- **Edifici:** si sceglie prima tipo (abitazione, scuola, ospedale, …),
  variante grafica (chiesa, cascina, piazza, campo, parcheggio, … vedi
  `VISUAL_KINDS`), dimensioni, rotazione e occupanti; l'anteprima verde mostra
  l'impronta ruotata. Q/E ruotano di 15° l'edificio da posizionare o quello
  selezionato. `rotation_deg` (antiorario visto dall'alto) è solo grafico:
  esposizione e percorsi usano ancora la posizione.
- **Luoghi:** oltre al nome e alle alternative possono avere un'**area**
  circolare (`radius_m`) e l'elenco di **edifici** e **strade** associati
  (`buildings`, `roads`); «Associa ciò che cade nell'area» li riempie da solo.
  Le associazioni sono dati di authoring e rendering: il gazetteer della
  simulazione continua a risolvere il centro del luogo. Eliminando un edificio
  o una strada il riferimento sparisce dai luoghi.
- **Strade** come polilinee e **luoghi** con un clic. Gli inneschi non si
  disegnano qui: sono azioni `Ignite` degli inject (vedi sopra). Gli oggetti si scelgono dalla lista o sulla mappa; le proprietà
  permettono di cambiare coordinate e attributi, oppure eliminarli.

«Scenario e meteo» modifica nome, descrizione, orario iniziale, durata dei turni e meteo. Importazione/esportazione JSON (download nel browser, copia negli appunti nel desktop) trasferisce l’intero scenario e permette di modificare anche ruoli, risorse, inject e politiche avanzate. Il formato è quello dei territori `.json`, non quello dei pack `.pack.json`. Nel browser è possibile usare poi l’editor pack ordinario sul nuovo territorio per i moduli di ruoli, risorse e timeline.

Annulla/ripristina conserva fino a 32 revisioni della bozza. Una modifica non
valida lascia intatti documento e cronologia. I pennelli di quota aggiornano
anche l’eventuale terreno visuale ad alta risoluzione. Ogni modifica confermata
ricostruisce la scena in sola lettura, preservando la camera a parità di griglia;
l’authoring di territori grandi può quindi richiedere un’attesa. Durante un tratto
di pennello la bozza si aggiorna a ogni passo ma la scena 3D viene ricostruita solo
ogni qualche decimo di secondo (a un multiplo del costo dell’ultima ricostruzione)
e a fine tratto; nel frattempo i tocchi sono mostrati come cerchi.

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

## Tool WebMCP per agenti (ChatGPT Atlas, Chrome)

`crates/frontend/src/webmcp.rs` espone su `document.modelContext` (WebMCP), con compatibilità per la precedente API `navigator.modelContext`, i tool per modificare uno scenario, nell'app server e nello standalone (stesse chiamate dell'editor, `api::request`). **I tool esistono solo mentre uno scenario è aperto nell'editor** (registrati all'apertura, `unregisterTool` alla chiusura o uscendo dalla vista) e lavorano sulla bozza che l'utente ha davanti, anche una copia non ancora salvata (per crearne uno nuovo: «Duplica» o «Personalizza», poi l'agente lo modifica). Mentre l'editor è aperto sono chiamabili anche come `window.ttxWebMcp.call(nome, args)`.

La registrazione preferisce `document.modelContext.registerTool` e riprova per circa un minuto se il contesto viene iniettato dopo il caricamento. Se i tentativi si esauriscono, la console segnala il problema. Il fallback `window.ttxWebMcp` resta disponibile, ma non sostituisce la registrazione WebMCP del browser.

| Tool | Cosa fa |
|---|---|
| `describe_scenario_format` | guida al formato e al flusso di lavoro |
| `get_open_scenario` | bozza completa, stato di salvataggio, problemi, nomi del territorio (`reference`) |
| `edit_open_scenario {operations,dry_run?}` | operazioni JSON-pointer `add/replace/remove` sulla bozza (`/edits/injects/-` aggiunge); tutto o niente, la bozza resta non salvata; l'id si cambia solo prima del primo salvataggio |
| `validate_open_scenario` | tutti i problemi della bozza |
| `save_open_scenario` | salva nella libreria (mai sugli inclusi); non avvia nulla |
| `list_scenarios`, `get_scenario {id}` | sola lettura, per prendere esempi |

I tool non avviano né toccano l'esercitazione in corso.
