# Specifica di alto livello: stato attuale

Documento di riferimento per definire le specifiche del TTX. Riassume le
funzionalità presenti su questo branch; non introduce capacità future. I
dettagli implementativi restano nel codice.

## Territori alternativi

Disponibili Valverde (4 × 4 km) e Selvariva (6 × 6 km), con catalogo delle basi nello standalone, filtro nell’editor e caricamento della base dichiarata da ciascun pack. Il cambio scenario ricostruisce la scena 3D locale; il viewer remoto riceve l’identità del territorio e carica la mappa corrispondente. Il pack `selvariva_festival` propone otto inject per una demo facilitata di un’ora: festival, scolaresca, due abitati, rotazione del vento e assistenza narrativa. Territorio, nomi e quantità sono sintetici. Vedi [traccia e validazione](scenario-selvariva.md).

## Authoring 3D degli scenari

Editor dedicato condiviso tra standalone web (`?editor=1`) e app Bevy
(`scenario-editor`): territorio vuoto, apertura e clonazione degli scenari,
pennelli di quota e combustibile, edifici, strade e luoghi,
proprietà, meteo, annulla/ripristina e import/export JSON completo.
La bozza è indipendente dall’esercitazione. I salvataggi sono territori JSON
nella libreria nativa oppure territori in localStorage nel browser, disponibili
anche all’editor pack e all’avvio dell’esercitazione. Vedi [uso e limiti](scenari.md#editor-scenario-3d).

## Scopo e ruoli

Il simulatore è pensato per esercitazioni da tavolo su emergenze. Lo scenario
attuale rappresenta un incendio boschivo nella valle sintetica di Valverde.

- **EXCON** prepara e invia inject, controlla l'esercitazione e il tempo,
  assegna risorse, approva o rifiuta le azioni dei partecipanti e può intervenire
  direttamente nel modello.
- **Partecipanti** agiscono entro il ruolo, le autorizzazioni, le informazioni
  e le risorse definiti dallo scenario. Possono comunicare in linguaggio libero
  nella propria lingua.
- **Valutatori** dispongono di una vista in sola lettura delle evidenze e del
  debrief.

## Flusso dell'esercitazione

### Turni e avanzamento del tempo

Il turno è una **finestra [inizio, fine pianificata) sulla timeline dello scenario**: l'inizio è il tempo simulato a cui il turno si è aperto, la durata è `turn_minutes` (campo minuti nella striscia del turno). Un turno ha tre fasi: *Preparazione* (ordini aperti, EXCON approva proposte e invia inject), *Risoluzione* (la simulazione scorre a passi di 1 s; le azioni degli inject scattano al loro istante) e *Chiusura* (log, riepiloghi AI, snapshot, apertura del turno successivo).

La scheda Timeline è pensata per il PC e la **lista degli inject** (con le loro azioni come sotto-elementi) è l'elemento principale: da 1200 px di larghezza occupa tutta l'altezza a sinistra (righe compatte con ora, tipo, titolo, anteprima del testo, stato e invio), mentre una colonna a destra contiene il pannello del turno e il **dettaglio dell'elemento selezionato** (testo completo, obiettivo, destinatari, «Invia…»; di default il primo elemento da gestire). Sotto i 1200 px il pannello torna sopra la lista e il dettaglio si apre nella riga. Il **pannello del turno**, compatto e unico proprietario di stato e tempo (l'intestazione della pagina mostra solo «Turno N · fase»): una riga con turno, finestra, stato («⏸ Fermo · Day 1 13:30» oppure «▶ In corso 13:06 → 13:30 · 60× · rimangono N min simulati»), selettore di **velocità** (max, 10×, 60×, 120×, `POST /api/master/settings {speed}`, valido anche prima di avanzare e durante la risoluzione), **⏸ Ferma qui** durante la risoluzione e la durata del turno (il pulsante dice «Sposta la fine a HH:MM»: cambiare la durata sposta la fine pianificata); la **traccia** con asse orario (tacche ogni 10 minuti, ora di inizio e di fine), cursore «ora», tratto in corso, un segno per ogni avanzamento intermedio e i marcatori degli inject (cerchio, senza marcatori per le azioni): gli elementi allo stesso istante sono un solo badge con il conteggio, quelli pianificati prima del turno e ancora da gestire stanno in una zona «in ritardo» a sinistra; il clic porta alla riga nella lista; e una riga di controlli con il chip **«Da gestire ora: N inject»** (clic: evidenzia le righe), l'ultimo avanzamento in una riga (azioni eseguite con il loro esito, azioni giocatore applicate, numero di eventi di simulazione) e gli avanzamenti:

- **▶▶ Avanza al prossimo inject (HH:MM)** (pulsante primario `#advance-next`): avanza all'istante pianificato del prossimo inject ancora da inviare e non saltato, **mai oltre la fine pianificata del turno**: se la fine del turno viene prima, l'etichetta dice «Avanza e chiudi turno N (HH:MM)», il turno si chiude lì e il clic successivo prosegue nel turno nuovo. Senza inject successivi avanza alla fine del turno (e lo chiude). Il server calcola il bersaglio (`until: "next_inject"`, `Exercise::advance_target`). La scansione non si ferma sulle azioni anticipate: ci passa attraverso, fermando la simulazione esattamente al loro istante. Dopo la scansione l'inject raggiunto è **da gestire** («due»): evidenziato nella lista con **Invia** e **Non inviare** (EXCON decide se inviarlo; «Non inviare» lo marca come saltato, non blocca più l'avanzamento e si può **Ripristinare**).
- **Avanza di [N] min ▶**: avanzamento intermedio libero; il turno resta aperto.

**Revisione prima di ogni avanzamento.** Se ci sono inject non ancora inviati il cui istante è raggiunto o attraversato (compresi quelli in ritardo di turni precedenti) oppure azioni anticipate che l'avanzamento esegue da sole, si apre una sola finestra con titolo che dice dove va il tempo e cosa succede al turno («Avanza a Day 1 13:05 · il turno resta aperto», «Chiudi turno 1 alle 13:30 · si apre il turno 2»). Ogni inject ha «Invia ora» (destinatari predefiniti), «Destinatari…» e «Non inviare»; ogni azione anticipata ha l'interruttore «esegui». Con inject non inviati il pulsante diventa «Avanza comunque · …». Se non c'è nulla da gestire si avanza direttamente. Non c'è più una decisione Esegui/Salta/Rinvia per le azioni: le azioni seguono il loro inject.

**Ferma qui.** `POST /api/master/resolve/stop` termina la risoluzione in corso al secondo simulato corrente (`Exercise::stop_resolution`): diventa un avanzamento intermedio, il turno resta aperto (anche se si stava chiudendo) e le azioni approvate più avanti restano approvate; la richiesta `resolve` in attesa restituisce il rapporto fino a quel secondo.

Sotto, la **lista** di inject e azioni, che occupa la maggior parte dell'altezza e scorre nel proprio riquadro. Le righe sono raggruppate in **fasce per turno** («Turno 2 · 13:30–14:00 · in corso»; i turni passati sono «concluso» e chiusi se non hanno elementi da gestire; i futuri sono attenuati), calcolate con la durata del turno aperto (approssimate per i turni passati se la durata è cambiata). Righe compatte: orario di scenario (l'offset in tempo reale non c'è più), tipo, titolo, stato («raggiunto: da inviare» per l'istante raggiunto nel turno, «in ritardo» per quelli di turni precedenti), riassunto dei destinatari e «Invia…»; testo, obiettivo e pill dei destinatari si aprono con la freccia. Al cambio di turno la lista scorre al turno in corso (o al primo elemento da gestire).

Gli inject restano **manuali**: quando l'istante previsto (`at_s` dell'inject, altrimenti l'inizio del turno suggerito) è raggiunto, la console li *propone* a EXCON ma non li invia da sola. Le proposte approvate sono applicate all'inizio di ogni avanzamento. Il rollback riporta all'inizio del turno e cancella gli avanzamenti intermedi. Il vecchio orologio in tempo reale (`/api/master/clock`) resta nell'API ma non è più nella console.

API: `POST /api/master/resolve {minutes?, substep?, until?: "next_inject"|"turn_end"}` (`next_event` alias); `POST /api/master/msel/<id>/skip|restore`; `POST /api/master/msel/<id>/actions/<action>/enabled {enabled}`; `POST /api/master/resolve/stop`; `POST /api/master/settings {turn_minutes?, speed?}`; `GET /api/master` espone `agenda` (finestra, `turn_minutes`, `clock_origin_s`, elementi, `due`, `next_event_s` (prossimo inject in attesa), `advance` (bersaglio dell'avanzamento: `to_s`, `closes_turn`, `inject`), `substeps`); `GET /api/state` espone `turn_window`, `speed`, `last_turn` e `resolving.partial`; `planned_injects` ha `actions` e `skipped`, `inject_actions` elenca le esecuzioni. `planned_injects` non ha più `real_time_offset`.

1. EXCON controlla tempo e turni dalla timeline: un unico elenco cronologico di
   tutti gli inject pianificati (orario reale e di scenario, raggruppabili per
   evento), con i destinatari come pill (verdi se già serviti). Cliccando un
   inject si scelgono i ruoli con i checkbox; un ruolo già servito è saltato e
   gli altri si possono servire più tardi. “Inject personalizzato” invia un inject
   libero. Le viste interne a EXCON sono Timeline, Risposte, Controllo e
   Cronologia. La Cronologia raccoglie tutti gli eventi operativi
   salvati, con ricerca, dettagli ed esportazione JSON per la valutazione.
2. I partecipanti comunicano in chat solo con gli altri ruoli. Conversazioni
   mostra esclusivamente messaggi tra player, separati per turno. Ogni messaggio
   cita un messaggio precedente della stessa conversazione oppure un inject
   ricevuto dal mittente; l’ultimo inject è preselezionato. Il riferimento è
   verificato, registrato e incluso nei riepiloghi AI anche tra turni. Da una
   bubble si può anche proporre un’azione: il modale è lo stesso degli inject
   e la proposta condivide con EXCON il thread collegato visibile al player,
   congelato al momento dell’invio. Le altre conversazioni restano private.
   L’AI traduce letteralmente i messaggi senza interpretarli come azioni. Un
   filtro lessicale blocca insulti e volgarità prima di tradurre o consegnare il
   testo, lasciando la bozza modificabile al mittente. Non è richiesta conferma
   della lingua predefinita, che resta modificabile dal menu. La scheda iniziale
   “Funzione” è in sola lettura: descrive ruolo, capacità e
   asset, distingue risorse disponibili, prenotate e impiegate e raccoglie i
   riepiloghi AI dei turni conclusi. La tab Conversazioni conta i messaggi non
   letti; la campanella conserva gli aggiornamenti del ruolo e le pill temporanee
   segnalano gli eventi ricevuti mentre si consultano altre schede. Leggere una
   conversazione azzera soltanto il suo conteggio. I riepiloghi usano solo le evidenze visibili
   al ruolo (azioni proprie e messaggi ricevuti/inviati), senza contesto privato
   di altri ruoli o EXCON; se l’AI manca, la scheda lo indica. Il rollback elimina
   i riepiloghi dei turni annullati. Nella scheda “Injects” leggono gli inject
   e propongono azioni
   tramite un modale quasi a schermo intero con mappa e modulo affiancati.
   Il click seleziona zone, coordinate, strade o edifici secondo l’azione; le
   linee tagliafuoco hanno due estremi selezionabili separatamente. Ogni inject
   può ricevere più risposte. Il testo libero passa da una verifica AI privata:
   senza comandi codificati nel simulatore la richiesta è bloccata con un
   messaggio esplicito. Solo una bozza valida e confermata arriva a EXCON;
   l’AI può chiedere chiarimenti prima della conferma.
3. Il partecipante rivede e conferma la bozza AI oppure invia una proposta
   strutturata senza AI. EXCON vede inject inviati e risposte nella tab Risposte (stato per
   destinatario, badge delle approvazioni), ne controlla la fattibilità e decide:
   approva, rifiuta oppure chiede un chiarimento (la proposta resta aperta con i suoi comandi; la
   risposta del player e un semplice messaggio a EXCON, tradotto ma mai
   interpretato dall'AI: la proposta torna `pending` e l'evento
   `clarification_answered` e registrato). Modificando i
   parametri la fattibilità è ricalcolata in automatico (`/revalidate`).
   La tab Risposte mostra solo gli inject effettivamente inviati e, per ciascuno,
   l'intero thread in ordine (inject, risposte, chiarimenti EXCON; le chat tra
   player restano private). Rispondendo a un chiarimento il selettore
   «Rispondi a inject» non è offerto: resta solo per le azioni indipendenti.
4. Le azioni approvate vengono eseguite o applicate durante l'avanzamento del
   tempo simulato. Le mappe, gli aggiornamenti ai ruoli, il registro e il debrief
   mostrano gli effetti.

La conversazione o l'interpretazione AI non modificano direttamente il mondo.
La simulazione applica solo comandi autorizzati e approvati. EXCON può annullare
un ramo dell'esercitazione con il rollback. Può inoltre calcolare previsioni del
fuoco, revisionarle e pubblicarle ai ruoli scelti.

### Iniziative narrative dei partecipanti

Dal modulo di risposta a un inject ricevuto, il player può scegliere **Iniziativa
narrativa** e inviare un testo senza comandi simulati e senza verifica AI. Il testo
originale è conservato; l'eventuale AI serve solo alla traduzione. Se la traduzione
non riesce, il testo viene comunque consegnato con l'indicazione del problema.
Una bozza operativa non supportata può essere ripresa esplicitamente come iniziativa
narrativa, da rivedere e inviare: non viene convertita automaticamente.

EXCON riceve l'iniziativa in Risposte e può approvarla, rifiutarla con una nota o
chiedere chiarimenti. La risposta a un chiarimento è un messaggio solo tradotto e
mantiene la stessa iniziativa. Dopo l'approvazione, EXCON registra un **esito
narrativo**, che può aggiornare con registrazioni successive. L'approvazione resta
separata dall'esito: non indica una missione completata. Le iniziative narrative
non eseguono comandi, non assegnano risorse e non alterano le grandezze fisiche.
Le eventuali conseguenze fisiche richiedono gli interventi o le proposte operative
ordinari, con i rispettivi controlli.

Il player vede gli esiti EXCON nel thread dell'inject; registro, valutatore e debrief
li distinguono dalle esecuzioni fisiche. Il salvataggio della sessione li ripristina;
il rollback annulla gli esiti registrati nei turni annullati, conservando quelli
precedenti. Gli altri ruoli non ricevono le iniziative o gli esiti privati.

API: `POST /api/player/<role>/narrative {text, reply_to}` (riferimento a un inject
ricevuto); decisioni e chiarimenti usano le rotte ordinarie delle proposte;
`POST /api/master/proposals/<id>/narrative-outcome {text, language?}` registra l'esito di
un'iniziativa narrativa approvata. Nessuna azione operativa con comandi vuoti diventa
approvabile implicitamente.

## Capacità modellate

- Propagazione del fuoco influenzata da terreno, combustibile e meteo, calcolata
  con più realizzazioni; esposizione e danni agli edifici.
- Avvisi ed evacuazioni di edifici o zone, rifugi e destinazioni, traffico
  d'evacuazione e risorse di trasporto.
- Interventi antincendio, tra cui linee tagliafuoco, lanci d'acqua e autobotti.
- Chiusure stradali, viaggi delle risorse, richieste di rinforzi e gestione
  delle disponibilità.
- Avanzamento del tempo, aggiornamenti della situazione, rollback e registrazione
  degli eventi.

Le azioni disponibili e i loro effetti dipendono dallo scenario e dalla modalità
del modello. Il modello operativo aggiunge routing stradale, code, viaggi,
interventi progressivi, risorse fisiche limitate e altri effetti operativi; il
modello storico mantiene regole più semplici. La vista 3D e le mappe presentano
lo stato e non prendono decisioni.

## Confini del modello

- Le persone sono conteggiate per edificio e non simulate individualmente.
- Tempi, capacità, soglie e politiche sono ipotesi dello scenario per
  l'esercitazione.
- La simulazione è riproducibile a parità di configurazione e seme. Le risposte
  generate dall'AI possono variare; se l'AI non è disponibile, EXCON può
  interpretare manualmente le proposte.

- I codici delle zone restano visibili a ogni zoom, anche ai bordi della
  porzione inquadrata, con dimensione percepita costante e indipendente dai
  livelli facoltativi della mappa.

- Mappa interattiva (EXCON e giocatori): zoom/pan (rotellina, trascinamento, pinch, tastiera), ispezione hover/tocco, livelli, inquadra incendio, schermo intero, barra di scala. Per i giocatori l’ispezione mostra solo le celle riferite (fog of war): altrove «non riferito». Pagina di prova: `/maptest.html` (dati sintetici, selettori funzioni e zone abilitate).

- Console EXCON unificata (“Controllo”): mappa interattiva più un elenco ricercabile di 29 strumenti, ciascuno con il proprio tooltip, che si aprono in modali dedicati (con mappa per la scelta dei punti). Gli strumenti sulla mappa applicano subito il comando (nessuna anteprima; “Correggi ultimo intervento” annulla l’ultimo). Il meteo si riduce a vento e umidità del combustibile. Informazioni ai ruoli, previsione e missioni restano raggiungibili dall’elenco. Le informazioni ai ruoli si rivelano per zona e per livello (incendio, edifici, strade, linee e lanci, squadre) con `POST /api/master/knowledge {role, source, zones, layers}`; il giocatore vede il resto velato (fog of war, `known_zones`). `GET /api/master` espone `role_knowledge`. Gli occupanti degli edifici restano sempre nascosti.

- MSEL unificato (redesign EXCON): `InjectTemplate.event` (default vuoto) raggruppa gli inject. `GET /api/master` e `/api/state` espongono `planned_injects` (cronologico: `id_inject`, `title`, `event`, `scenario_time`, `scenario_time_s`, `recipients_planned`, `sent_to`, `not_yet_sent_to`); `GET /api/master` espone anche `inject_responses` (per consegna e ruolo: `replies`, `pending_proposals`). `POST /api/master/msel/<id>/send` accetta `{"recipients":[ruoli]}` (default: destinatari pianificati); i ruoli che hanno già ricevuto l’inject sono saltati in silenzio.
- Azioni pianificate (`Scenario.timeline`, `exercise::actions`): world changes sulla timeline MSEL, indipendenti dagli inject. A ogni avanzamento del tempo EXCON decide Esegui/Salta/Rinvia per quelle che cadono nella finestra; la durata del turno è una proprietà dello scenario (`turn_minutes`) e la scheda Timeline dell'editor mostra inject e azioni su un'unica asse oraria con fasce di turno e trascinamento; vedi `docs/scenari.md`.
- Proposte: `POST /api/master/proposals/<id>/clarify {note}` porta una proposta del player in `needs_clarification` e consegna la domanda nello stesso thread; `POST /api/master/proposals/<id>/revalidate {commands?}` ricalcola descrizioni e verdetti senza decidere; la risposta del player a un chiarimento (`POST /api/player/<role>/reply` con `reply_to` = messaggio di domanda) e un messaggio `reply` nello stesso thread: solo traduzione in inglese, nessuna interpretazione AI, nessuna nuova proposta e nessuna sostituzione; la proposta torna `pending` con i comandi originali (per cambiare azione il player ne crea una nuova; `/actions` e `/draft` con `reply_to` = domanda aperta sono rifiutati con errore esplicito). `superseded`/`supersedes` restano nei tipi ma non vengono piu prodotti. Il flag `edited` della decisione è vero solo se i comandi cambiano davvero.
- Thread di chiarimento: la conversazione testuale EXCON-player su una dichiarazione e una vista derivata (`exercise::threads`), non una copia dei dati. Mette in fila la proposta (e l'eventuale catena storica `supersedes`/`superseded_by`), i messaggi `clarification` di EXCON e le risposte del player; l'id del thread e quello della proposta d'origine. Stato: `awaiting_player` (ultima voce di EXCON), `awaiting_excon`, `closed` (chiuso da EXCON con `POST /api/master/clarifications/<id>/close`, oppure proposta decisa). Chiudere ritira una domanda senza risposta (la proposta torna `pending`); un nuovo `clarify` riapre. EXCON fa domande di follow-up con lo stesso `clarify` sulla proposta corrente (`current_proposal`); il player risponde in testo, piu volte, con `POST /api/player/<role>/reply` con `reply_to` = ultimo messaggio di EXCON (si puo rispondere a un chiarimento anche senza inject). Esposto come `clarification_threads` in `/api/master` (con testo inglese), `/api/player/<role>` (senza `text_en`) e nel debrief; ogni riga di `inject_responses` ha `clarification_threads` e i messaggi del suo `thread` portano `clarification_thread`, cosi la scheda Risposte mostra le stesse voci con stato e pulsante "Chiudi chiarimento". Lato player la chat compare nella timeline tattica (`crates/frontend/src/clarify.rs`). I thread non modificano il mondo: solo `World::apply` da proposte approvate.
- Rendering dei dati: `render.rs` mostra comandi, meteo, esecuzioni, eventi e dati generici come elenchi leggibili (il JSON grezzo resta in un blocco richiudibile “dati grezzi” solo dove serve al debug); `event_label`/`role_label` danno nomi leggibili a eventi e ruoli.
- LLM: `chat()` ritenta fino a 2 volte su errori di rete, corpo illeggibile o troncato, 429/5xx e risposta 2xx senza contenuto (mai sui 4xx).
- Notifiche della console EXCON (`crates/frontend/src/notifications.rs`): la campanella è lo storico cronologico (più recente in alto, «da leggere» evidenziato, pallino arancione se ci sono voci che richiedono azione); i toast compaiono in basso a destra, fuori dai controlli del turno. Gli eventi sono mappati in modo puro da `log_spec` (titolo specifico: «Turno 1 · avanzamento a 13:05», «Inject inviato a …», «Azione pianificata eseguita: …»). Priorità *Informativo* (toast che scade dopo ~6 s, oppure solo campanella per avanzamenti, inizio risoluzione, inject inviati, risorse) e *Richiede azione* (proposta in attesa da un ruolo, azione pianificata o consegna non riuscita, bozza da rivedere: toast arancione che resta finché la proposta non viene decisa; non si segna letto aprendo la vista). Le voci simili ravvicinate (avanzamenti dello stesso turno, risorse, missione) si fondono finché non lette; al massimo 3 toast, gli informativi cedono per primi. Non notificano: comandi propri dell'EXCON, parsing delle proposte, cambi di stato interni, chat tra altri ruoli. I player ricevono un solo avviso per cambio di fase del turno, esiti delle proprie proposte, chiarimenti (azione), messaggi, riepiloghi e missioni.
