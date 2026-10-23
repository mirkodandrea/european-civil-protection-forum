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

1. EXCON controlla tempo e turni dalla timeline e apre il modale inject dalla
   colonna di un player oppure con “Inject a tutti”. Il form riusa i template
   dello scenario e permette di modificare titolo, mittente e testo. Le tab
   principali corrispondono agli attori; Timeline, Mappa, Strumenti e Cronologia
   sono viste interne a EXCON. La Cronologia raccoglie tutti gli eventi operativi
   salvati, con ricerca, dettagli ed esportazione JSON per la valutazione.
2. I partecipanti comunicano in chat solo con gli altri ruoli. Nelle proprie
   conversazioni vedono anche gli inject ricevuti dal proprio ruolo, senza
   condividerli automaticamente con l’interlocutore. Non è richiesta conferma
   della lingua predefinita, che resta modificabile dal menu. La scheda iniziale
   “Funzione” è in sola lettura: descrive ruolo, capacità e
   asset, distingue risorse disponibili, prenotate e impiegate e raccoglie i
   riepiloghi AI dei turni conclusi. I riepiloghi usano solo le evidenze visibili
   al ruolo (azioni proprie e messaggi ricevuti/inviati), senza contesto privato
   di altri ruoli o EXCON; se l’AI manca, la scheda lo indica. Il rollback elimina
   i riepiloghi dei turni annullati. Nella scheda “Injects” leggono gli inject
   e propongono azioni
   tramite moduli e mappa, oppure preparano una decisione con l’AI. L’AI può
   tradurre il testo, riconoscere azioni proponibili e chiedere chiarimenti.
3. Il partecipante rivede e conferma la bozza AI oppure invia una proposta
   strutturata senza AI. EXCON vede inject e azioni in una timeline per turno,
   suddivisa per ruolo, ne controlla la fattibilità e decide se approvarle.
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

- Mappa interattiva (EXCON e giocatori): zoom/pan (rotellina, trascinamento, pinch, tastiera), ispezione hover/tocco, livelli, inquadra incendio, schermo intero, barra di scala. Per i giocatori l’ispezione mostra solo le celle riferite (fog of war): altrove «non riferito». Pagina di prova: `/maptest.html` (dati sintetici, selettori funzioni e zone abilitate).

- Console EXCON unificata: la scheda Mappa è un’unica area di lavoro (mappa interattiva + pannello a schede Intervento / Informazioni ai ruoli / Previsione / Missioni). Le informazioni ai ruoli si rivelano per zona e per livello (incendio, edifici, strade, linee e lanci, squadre) con `POST /api/master/knowledge {role, source, zones, layers}`; il giocatore vede il resto velato (fog of war, `known_zones`). `GET /api/master` espone `role_knowledge`. Gli occupanti degli edifici restano sempre nascosti.
