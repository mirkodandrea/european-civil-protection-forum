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

## Situazione EXCON: impatto, anteprima di fine turno, suggerimenti

La scheda **Situazione** (solo EXCON, solo desktop) mostra a EXCON l'effetto dell'incendio e cosa accadrà entro la fine del turno, così può scrivere gli inject in anticipo. **Gli inject restano manuali**: nulla viene inviato o pianificato da solo.

- **Impatto ora** (`GET /api/master/impact`, `sim::impact`): righe per strutture, rifugi, edifici toccati dal fuoco e strade non integre (le abitazioni semplici sono solo contate per settore finché il fuoco non le raggiunge); totali e aggregati per settore con variazione rispetto all'inizio del turno (snapshot di rollback): edifici minacciati/danneggiati/distrutti, persone presenti, in evacuazione, arrivate, a rischio, strade tagliate, riempimento dei rifugi.
- **Anteprima deterministica** (`GET /api/master/situation/preview[?have=<key>]`, `exercise::situation`): una copia del mondo (`World` è `Clone`, comprese le sequenze casuali dell'ensemble) viene portata alla fine pianificata del turno **fuori dal mutex**, con le stesse regole di una risoluzione reale (`begin_resolution` + `step_resolution`): proposte approvate, azioni pianificate attive (rispettando disattivazioni e inject saltati), passi da 1 s. Con lo stesso seme e senza altre decisioni il risultato coincide con l'avanzamento reale (test `preview_equals_the_real_resolution`). Restituisce, per elemento, il primo istante di minaccia/danno/distruzione/taglio/svuotamento, l'impatto e la mappa del fuoco a fine turno. Si ricalcola in background (un solo lavoro, debounce 600 ms, risultato scartato se la chiave non coincide più) mentre la scheda è aperta; in pausa durante la risoluzione. Nel browser (standalone) il calcolo gira sul thread principale: circa 0,2 s su Valverde e 1 s su Selvariva senza fuoco esteso, alcuni secondi con un incendio ampio. Non è la previsione ai giocatori (`fire::forecast`).
- **Suggerimenti** (`GET /api/master/situation/suggestions`, `exercise::suggestions`): regole `building_reached` (struttura con persone raggiunta prima del termine dell'evacuazione, priorità scuole/ospedali), `houses_reached` (abitazioni per settore), `place_without_exit` (abitato senza uscita stradale), `evac_route_cut` (evacuati in viaggio su una strada che verrà tagliata), `shelter_front` / `shelter_over` (rifugio vicino al fronte o oltre capienza), `task_route_cut` (percorso dei mezzi dispiegati). Ogni bozza ha id stabile `regola:oggetto`, gravità, momento (ora / entro HH:MM), destinatari derivati dalle azioni consentite ai ruoli, titolo e testo it/en ed evidenza. EXCON la apre, la modifica, può chiedere all'AI di riscriverla con la voce di un mittente (`POST /api/master/situation/rewrite`, senza AI il testo resta invariato), poi la **invia ora** (`POST /api/master/inject`), la **pianifica** in timeline (`POST /api/master/situation/plan`: aggiunge un inject MSEL all'ora scelta, da inviare a mano) o la **scarta** (`POST /api/master/situation/mark`). Le coppie scartate/inviate/pianificate non vengono riproposte; il rollback dimentica quelle successive.
- La Timeline mostra solo un chip «N suggerimenti» che porta alla scheda.

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
   libero. Le viste interne a EXCON sono Timeline, Situazione, Risposte, Controllo e
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
   i riepiloghi dei turni annullati. Nella scheda **Risposte** (en “Response”) la
   mappa tattica sta a sinistra e l’elenco degli inject a destra. Un click su un
   inject apre al posto dell’elenco il dettaglio (indietro all’elenco, come nelle
   Conversazioni): thread dell’inject con le risposte, i chiarimenti e gli esiti,
   conversazioni collegate e due azioni: **Rispondi** e **Nuova conversazione**.
   Rispondi apre un modale quasi a schermo intero con mappa e modulo: una risposta
   è una **narrazione facoltativa** più **zero o più azioni** (almeno una delle due).
   Le azioni si aggiungono dal modulo strutturato oppure descrivendole all’assistente
   AI, che propone una bozza da rivedere (nessuna azione codificata: la bozza non è
   aggiungibile); il pulsante **Rispondi** invia tutto in una sola volta. Ogni
   inject può ricevere più risposte.
3. La risposta è verificata prima di produrre effetti. EXCON vede inject inviati e risposte nella tab Risposte (stato per
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

### Decisione EXCON per singola azione

`POST /api/master/proposals/<id>/decide` accetta, oltre a `approve`, `commands`
(modifica) e `note` (`reason` è accettato come sinonimo), l'elenco
`rejected_actions: [{index, note?}]` (indici nelle azioni revisionate: la modifica
EXCON, altrimenti i comandi della proposta; `note` ammette il sinonimo `reason`).

- **Approvazione parziale** (`approve: true` con azioni escluse): si eseguono solo le
  azioni approvate; la verifica di fattibilità vale solo per queste, quindi un'azione
  non fattibile si può escludere senza bloccare le altre. La proposta è `approved`
  (poi `applied`/`failed`) e le comunicazioni rivedute partono come per
  un'approvazione completa. `ApprovalRecord` conserva `proposed_commands`,
  `effective_commands` (solo le approvate) e `rejected_actions` (indice, comando,
  descrizione, motivo). Escludere tutte le azioni di una proposta che non porta
  altro (comunicazioni, inject, limiti, narrazione) equivale a un rifiuto.
- **Rifiuto** (`approve: false`): tutte le azioni sono rifiutate; i motivi per
  azione eventualmente indicati sono conservati e comunicati.
- **Presa visione** (`approve: true` su una proposta senza nulla da eseguire, ad
  esempio una domanda informativa): stato `acknowledged`, finale; nessuna
  esecuzione, la nota EXCON è la risposta al player. Diversa da un'iniziativa
  narrativa, che dopo l'approvazione attende un esito: una risposta presa visione
  non riceve esiti narrativi. Il rollback non la annulla (come un rifiuto).
- **Stato della revisione** (`review` su ogni proposta e nella decisione):
  `in_review`, `clarification_requested`, `approved`, `partially_approved`,
  `rejected`, `acknowledged`, `informational`, `superseded`, `withdrawn` (annullata
  da rollback prima di una decisione). È separato da `status`, che segue anche
  l'esecuzione: una proposta `applied` resta `partially_approved`, e dopo un
  rollback (`rolled_back`) la decisione e il suo `review` restano nel record.
  Ogni riga di `inject_responses` ha `decided_proposals` (`review`, nota, numero
  di azioni approvate/rifiutate): la tab Risposte mostra «approvata», «approvata
  in parte», «rifiutata», «presa visione» invece di «risposta ricevuta». Il player
  vede lo stesso esito nella lista e, per un'approvazione parziale, ogni azione
  con ✓/✗ e il motivo.
- Il messaggio di riscontro al player (e quindi la sua conversazione con l'AI)
  elenca `APPROVED: …` e `NOT APPROVED: … Reason: …` per ogni azione. L'evento
  `proposal_decided` del log riporta `review` e la decisione completa; il journal
  della sessione ripete il corpo della richiesta, e i campi nuovi hanno valori
  predefiniti (sessioni salvate prima restano leggibili).

Nella console (Risposte) ogni azione ha la casella «Approva questa azione» e, se
esclusa, un motivo facoltativo per il player; il pulsante diventa «Approva N di M
azioni». Una proposta senza azioni offre «Prendi atto».

### Risposte con narrazione e azioni

Una risposta è una sola proposta (`Proposal`) con testo narrativo opzionale e comandi
opzionali. `narrative = true` indica che c’è una narrazione; `carries_actions = true`
che porta anche comandi (una narrazione senza comandi è “pura” e non è mai eseguita).
La narrazione richiede un inject a cui rispondere; il testo originale è conservato e
l’AI serve solo alla traduzione (se fallisce, il testo è consegnato con
l’indicazione del problema). I comandi seguono le regole ordinarie: ruolo autorizzato,
verifica di fattibilità, approvazione di EXCON, esecuzione. La narrazione non esegue
comandi, non assegna risorse e non altera le grandezze fisiche.

EXCON riceve la risposta in Risposte e può approvarla, rifiutarla con una nota o
chiedere chiarimenti (la risposta del player resta un messaggio solo tradotto). Dopo
l’approvazione (anche dopo l’esecuzione dei comandi) EXCON può registrare un **esito**,
aggiornabile, fatto di una narrazione e/o di una o più **azioni EXCON**
(`POST /api/master/proposals/<id>/narrative-outcome {text?, language, persona?,
commands?}`; almeno una delle due). Le azioni sono interventi EXCON ordinari
(`Exercise::direct_command_logged`, stessa validazione e applicazione immediata dei
comandi di Controllo, voce `master_command` nel log con `origin: narrative_outcome`):
vengono verificate tutte su una copia del mondo e applicate solo se tutte valide. L’esito
conserva i `log_seq` degli interventi (`interventions`); il player vede la narrazione
con il personaggio e gli effetti (`effects`). L’interfaccia (Risposte) riusa le
finestre degli strumenti di Controllo con selezione sulla mappa
(`staged_commands.rs`, più un campo JSON per comandi come `CloseRoad`). Registro,
valutatore e debrief distinguono gli interventi EXCON dalle esecuzioni dei player;
l’approvazione non indica una missione completata. Il salvataggio della sessione
(journal) li ripristina; il rollback ripristina il mondo e annulla esiti e interventi
dei turni annullati.

### Personaggi: i player non vedono mai «EXCON»

Tutto ciò che EXCON scrive a un player (domande di chiarimento, note di
approvazione/rifiuto, esiti narrativi, inject, informazioni e previsioni) è detto da un
**personaggio** (`exercise::persona`): un ruolo dello scenario (anche non giocato), un
ruolo interpretato dall’EXCON (`excon_roles`), un mittente di inject o un testo libero
(`persona` nei corpi di `clarify`, `decide`, `narrative-outcome`; la «fonte» per
informazioni e previsioni; il mittente dell’inject personalizzato). Predefinito: il
mittente dell’inject a cui si risponde, altrimenti `default_persona` dello scenario
(campo del pack, modificabile nelle Impostazioni; vuoto = «Sala operativa» /
«Operations room»). Il personaggio è salvato su `Message.persona`, sulle decisioni e sugli
esiti; `GET /api/player/<role>` lo risolve (`persona`, `default_persona`) e sostituisce
ogni «EXCON» residuo nelle stringhe. Le chiavi tradotte con «EXCON» sono state
riscritte; cinque stringhe (`clock_note`, `sit_report`, `awaiting_start`,
`inject_info`, `map_legend`) non sono più nel catalogo delle altre lingue (ripiego
sull’inglese). La console EXCON continua a dire EXCON.

### Conversazioni collegate a un inject

Chi ha ricevuto un inject può aprire una conversazione con uno o più altri player
(`exercise::conversations`). È privata ai partecipanti verso gli altri player; EXCON
invece legge la trascrizione completa (`linked_conversations[].messages`: autore, ora,
testo originale e inglese, citazioni; `message_count`), mostrata espandibile in Risposte
sotto l’inject. Il testo compare anche nel log (`conversation_message`), nel debrief e
nelle evidenze del valutatore (`linked_conversations`). Ogni messaggio può citare un messaggio precedente della
conversazione o l’inject. Regola di visibilità dell’inject: chi entra in una
conversazione su un inject che non ha ricevuto lo legge citato in testa alla
conversazione (titolo, mittente, ora, testo); il testo è la copia del partecipante se ne
ha una, altrimenti la copia di chi ha creato la conversazione se la lingua coincide,
altrimenti la versione inglese di lavoro (nessuna traduzione aggiuntiva). Non
vede altri inject né altre conversazioni.

Dalla conversazione si può rispondere all’inject (stesso modale). Un partecipante che
non ha ricevuto l’inject può rispondere con `reply_to` = consegna a cui la conversazione
è collegata e `conversation` = id: il server verifica che sia partecipante e che la
conversazione sia collegata a quell’inject; la proposta registra `via_conversation`.
In Risposte di EXCON la risposta compare sotto l’inject con il ruolo che risponde e il
riferimento alla conversazione. Fuori dalla conversazione `reply_to` verso quell’inject
è rifiutato.

API: `POST /api/player/<role>/actions {narrative?, commands?, text?, reply_to?,
conversation?}` (unica rotta di risposta; ricorda che serve almeno narrazione o un
comando); `POST /api/player/<role>/narrative {text, reply_to}` resta per compatibilità
(solo narrazione); `POST /api/player/<role>/conversations {delivery, participants,
text}` e `POST /api/player/<role>/conversations/<id>/messages {text, reply_to?}`;
`GET /api/player/<role>` espone `linked_conversations` (con `quote`, messaggi nella
lingua del ruolo, `responses`), `GET /api/master` espone `linked_conversations`
(riferimenti) e, nelle righe di `inject_responses`, `conversations`, `role` e
`conversation` delle proposte. `POST /api/master/proposals/<id>/narrative-outcome
{text, language?}` registra l’esito. Limite noto: i partecipanti si fissano alla
creazione.

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

- Etichette e legenda della mappa 2D (`crates/frontend/src/map.rs`, `label_layout.rs`, `map_legend.rs`): nomi di edifici, luoghi, chiusure, risorse e codici di zona sono posizionati in pixel dello schermo dopo il disegno del mondo, con font di 14,5–18 px CSS (leggibile al 75% di zoom del browser; cresce lentamente con lo zoom della mappa) su fondo bianco con bordo. Il posizionamento (`label_layout::place`, puro e con test) segue la priorità: 0 edifici critici (scuola, ospedale, distributore, municipio) e colpiti da minaccia/danno/evacuazione, incendio segnalato; 1 altri edifici con nome, chiusure, risorse; 2 luoghi; 3 codici di zona. Ogni etichetta prova una corona di posizioni attorno al punto; se si sposta di oltre 10 px è unita al punto da una linea guida; se non c'è spazio le etichette a bassa priorità spariscono, i codici di zona (sempre visibili) passano a un angolo della zona. Nome di luogo ed edificio uguali e vicini valgono un’etichetta. I segnaposto (edifici con stato, ancore) non vengono coperti. La legenda (`map_legend.rs`, stringhe it/en proprie, altre lingue in inglese) non si affida al solo colore: fronte attivo = puntini, bruciato = tratteggio diagonale, acqua/ritardante = righe orizzontali, linee tratteggiate/continue per previsione, linea di contenimento, strada tagliata e chiusura polizia, simboli per stato degli edifici (▲ minacciato, ◆ danneggiato, ✕ distrutto, ➜ in evacuazione, ○ evacuato) disegnati sulla mappa con lo stesso glifo. `GET` della statica per ruolo include ora `kind` degli edifici.
- Mappa interattiva (EXCON e giocatori): zoom/pan (rotellina, trascinamento, pinch, tastiera), ispezione hover/tocco, livelli, inquadra incendio, schermo intero, barra di scala. Per i giocatori l’ispezione mostra solo le celle riferite (fog of war): altrove «non riferito». Pagina di prova: `/maptest.html` (dati sintetici, selettori funzioni e zone abilitate).

- Console EXCON unificata (“Controllo”): mappa interattiva più un elenco ricercabile di 29 strumenti, ciascuno con il proprio tooltip, che si aprono in modali dedicati (con mappa per la scelta dei punti). Gli strumenti sulla mappa applicano subito il comando (nessuna anteprima; “Correggi ultimo intervento” annulla l’ultimo). Il meteo si riduce a vento e umidità del combustibile. Informazioni ai ruoli, previsione e missioni restano raggiungibili dall’elenco. Le informazioni ai ruoli si rivelano per zona e per livello (incendio, edifici, strade, linee e lanci, squadre) con `POST /api/master/knowledge {role, source, zones, layers}`; il giocatore vede il resto velato (fog of war, `known_zones`). `GET /api/master` espone `role_knowledge`. Gli occupanti degli edifici restano sempre nascosti.

- MSEL unificato (redesign EXCON): `InjectTemplate.event` (default vuoto) raggruppa gli inject. `GET /api/master` e `/api/state` espongono `planned_injects` (cronologico: `id_inject`, `title`, `event`, `scenario_time`, `scenario_time_s`, `recipients_planned`, `sent_to`, `not_yet_sent_to`); `GET /api/master` espone anche `inject_responses` (per consegna e ruolo: `replies`, `pending_proposals`). `POST /api/master/msel/<id>/send` accetta `{"recipients":[ruoli]}` (default: destinatari pianificati); i ruoli che hanno già ricevuto l’inject sono saltati in silenzio.
- Azioni pianificate (`Scenario.timeline`, `exercise::actions`): world changes sulla timeline MSEL, indipendenti dagli inject. A ogni avanzamento del tempo EXCON decide Esegui/Salta/Rinvia per quelle che cadono nella finestra; la durata del turno è una proprietà dello scenario (`turn_minutes`) e la scheda Timeline dell'editor mostra inject e azioni su un'unica asse oraria con fasce di turno e trascinamento; vedi `docs/scenari.md`.
- Proposte: `POST /api/master/proposals/<id>/clarify {note}` porta una proposta del player in `needs_clarification` e consegna la domanda nello stesso thread; `POST /api/master/proposals/<id>/revalidate {commands?}` ricalcola descrizioni e verdetti senza decidere; la risposta del player a un chiarimento (`POST /api/player/<role>/reply` con `reply_to` = messaggio di domanda) e un messaggio `reply` nello stesso thread: solo traduzione in inglese, nessuna interpretazione AI, nessuna nuova proposta e nessuna sostituzione; la proposta torna `pending` con i comandi originali (per cambiare azione il player ne crea una nuova; `/actions` e `/draft` con `reply_to` = domanda aperta sono rifiutati con errore esplicito). `superseded`/`supersedes` restano nei tipi ma non vengono piu prodotti. Il flag `edited` della decisione è vero solo se i comandi cambiano davvero.
- Thread di chiarimento: la conversazione testuale EXCON-player su una dichiarazione e una vista derivata (`exercise::threads`), non una copia dei dati. Mette in fila la proposta (e l'eventuale catena storica `supersedes`/`superseded_by`), i messaggi `clarification` di EXCON e le risposte del player; l'id del thread e quello della proposta d'origine. Stato: `awaiting_player` (ultima voce di EXCON), `awaiting_excon`, `closed` (chiuso da EXCON con `POST /api/master/clarifications/<id>/close`, oppure proposta decisa). Chiudere ritira una domanda senza risposta (la proposta torna `pending`); un nuovo `clarify` riapre. EXCON fa domande di follow-up con lo stesso `clarify` sulla proposta corrente (`current_proposal`); il player risponde in testo, piu volte, con `POST /api/player/<role>/reply` con `reply_to` = ultimo messaggio di EXCON (si puo rispondere a un chiarimento anche senza inject). Esposto come `clarification_threads` in `/api/master` (con testo inglese), `/api/player/<role>` (senza `text_en`) e nel debrief; ogni riga di `inject_responses` ha `clarification_threads` e i messaggi del suo `thread` portano `clarification_thread`, cosi la scheda Risposte mostra le stesse voci con stato e pulsante "Chiudi chiarimento". Lato player la chat compare nella timeline tattica (`crates/frontend/src/clarify.rs`). I thread non modificano il mondo: solo `World::apply` da proposte approvate.
- Rendering dei dati: `render.rs` mostra comandi, meteo, esecuzioni, eventi e dati generici come elenchi leggibili (il JSON grezzo resta in un blocco richiudibile “dati grezzi” solo dove serve al debug); `event_label`/`role_label` danno nomi leggibili a eventi e ruoli.
- LLM: `chat()` ritenta fino a 2 volte su errori di rete, corpo illeggibile o troncato, 429/5xx e risposta 2xx senza contenuto (mai sui 4xx).
- Notifiche della console EXCON (`crates/frontend/src/notifications.rs`): la campanella è lo storico cronologico (più recente in alto, «da leggere» evidenziato, pallino arancione se ci sono voci che richiedono azione); i toast compaiono in basso a destra, fuori dai controlli del turno. Gli eventi sono mappati in modo puro da `log_spec` (titolo specifico: «Turno 1 · avanzamento a 13:05», «Inject inviato a …», «Azione pianificata eseguita: …»). Priorità *Informativo* (toast che scade dopo ~6 s, oppure solo campanella per avanzamenti, inizio risoluzione, inject inviati, risorse) e *Richiede azione* (proposta in attesa da un ruolo, azione pianificata o consegna non riuscita, bozza da rivedere: toast arancione che resta finché la proposta non viene decisa; non si segna letto aprendo la vista). Le voci simili ravvicinate (avanzamenti dello stesso turno, risorse, missione) si fondono finché non lette; al massimo 3 toast, gli informativi cedono per primi. Non notificano: comandi propri dell'EXCON, parsing delle proposte, cambi di stato interni, chat tra altri ruoli. I player ricevono un solo avviso per cambio di fase del turno, esiti delle proprie proposte, chiarimenti (azione), messaggi, riepiloghi e missioni.

- Validazione luogo/tipo di azione (playtest 2026-10-27, problema #3): un luogo di tipo diverso da quello atteso non fa più perdere il turno. Nel modulo del giocatore (`tactical.rs`) cambiare tipo di azione svuota il luogo se ne cambia la natura (zona, edificio, strada, punto); per «Evacuare una zona» e «Allertare una zona» il campo ha l’elenco dei codici griglia e dei luoghi, e scrivere il nome di un edificio passa da solo a «Evacuazione di edificio» con un avviso. Lato modello, `sim::World::reconcile_command` (chiamato da `Exercise::set_commands`, quindi per form e interpretazione AI) adatta i casi non ambigui: zona = nome di edificio, EvacuateBuilding; zona = luogo con nome («Pineta»), zona in cui cade; allerta su un edificio, zona dell’edificio; EvacuateBuilding con un codice zona, EvacuateZone. L’adattamento compare come nota del verdetto. Se nulla corrisponde, l’errore dice che cosa è stato riconosciuto e propone i nomi più vicini (`unknown zone "X": a zone is a grid code like A1 (did you mean «…»?)`; `"X" is a building, not a zone: use Evacuate building…`). Il catalogo per l’LLM (`sim::command::catalogue`) dice che `zone` è sempre un codice griglia e che una struttura singola usa `EvacuateBuilding`. Le stringhe nuove sono in `i18n.rs` (it/en) e `i18n_place.rs` (altre lingue).
