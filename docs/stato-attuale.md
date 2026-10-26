# Specifica di alto livello: stato attuale

Documento di riferimento per definire le specifiche del TTX. Riassume le
funzionalità presenti su questo branch; non introduce capacità future. I
dettagli implementativi restano nel codice.

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
   approva, rifiuta oppure chiede un chiarimento (la proposta resta aperta; la
   risposta del player la sostituisce, stato `superseded`). Modificando i
   parametri la fattibilità è ricalcolata in automatico (`/revalidate`).
4. Le azioni approvate vengono eseguite o applicate durante l'avanzamento del
   tempo simulato. Le mappe, gli aggiornamenti ai ruoli, il registro e il debrief
   mostrano gli effetti.

La conversazione o l'interpretazione AI non modificano direttamente il mondo.
La simulazione applica solo comandi autorizzati e approvati. EXCON può annullare
un ramo dell'esercitazione con il rollback. Può inoltre calcolare previsioni del
fuoco, revisionarle e pubblicarle ai ruoli scelti.

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

- MSEL unificato (redesign EXCON): `InjectTemplate.event` (default vuoto) raggruppa gli inject. `GET /api/master` e `/api/state` espongono `planned_injects` (cronologico: `id_inject, title, event, real_time_offset` s reali = tempo scenario / ratio del clock, `scenario_time`, `scenario_time_s`, `recipients_planned`, `sent_to`, `not_yet_sent_to`); `GET /api/master` espone anche `inject_responses` (per consegna e ruolo: `replies`, `pending_proposals`). `POST /api/master/msel/<id>/send` accetta `{"recipients":[ruoli]}` (default: destinatari pianificati); i ruoli che hanno già ricevuto l’inject sono saltati in silenzio.
- Azioni pianificate (`Scenario.timeline`, `exercise::actions`): world changes sulla timeline MSEL, indipendenti dagli inject. All'avvio del turno EXCON decide Esegui/Salta/Rinvia per quelle che cadono nella finestra; vedi `docs/scenari.md`.
- Proposte: `POST /api/master/proposals/<id>/clarify {note}` porta una proposta del player in `needs_clarification` e consegna la domanda nello stesso thread; `POST /api/master/proposals/<id>/revalidate {commands?}` ricalcola descrizioni e verdetti senza decidere; la risposta del player a un chiarimento marca la vecchia proposta `superseded` (`supersedes` / `superseded_by`). Il flag `edited` della decisione è vero solo se i comandi cambiano davvero.
- Rendering dei dati: `render.rs` mostra comandi, meteo, esecuzioni, eventi e dati generici come elenchi leggibili (il JSON grezzo resta in un blocco richiudibile “dati grezzi” solo dove serve al debug); `event_label`/`role_label` danno nomi leggibili a eventi e ruoli.
- LLM: `chat()` ritenta fino a 2 volte su errori di rete, corpo illeggibile o troncato, 429/5xx e risposta 2xx senza contenuto (mai sui 4xx).
