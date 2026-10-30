# Specifica di alto livello: stato attuale

Documento di riferimento per definire le specifiche del TTX. Riassume le
funzionalità presenti su questo branch; non introduce capacità future. I
dettagli implementativi restano nel codice.

## Territori alternativi

Disponibili Valverde (4 × 4 km), Selvariva (6 × 6 km) e l’Isola di Roccavento (8 × 8 km, dal gioco `propagator_abm`), con catalogo delle basi nello standalone, filtro nell’editor e caricamento della base dichiarata da ciascun pack. Il cambio scenario ricostruisce la scena 3D locale; il viewer remoto riceve l’identità del territorio e carica la mappa corrispondente. Il pack `selvariva_crisi_integrata` (sostituisce `selvariva_festival`) propone 3 ruoli, cinque turni da 30 minuti e dieci inject: festival e scolaresca, accessi, rinforzi, vento, assistenza, stampa e consegne; il drone è della direzione dei soccorsi e cinque inject rivelano la mappa (stime approssimate e rilievi esatti). Territorio, nomi e quantità sono sintetici. Vedi [traccia e validazione](scenario-selvariva.md). Roccavento porta i sei **borghi** del gioco con il comando EXCON «Evacua borgo» (`EvacuateDistrict`): partenze scaglionate (5 min di preparazione, 6 persone/min, prima le case vicine alla strada) verso l’area di raccolta del borgo, senza aprirla; pack `roccavento_valico`, il fuoco della demo del gioco. Vedi [scenario-roccavento.md](scenario-roccavento.md).

## Authoring 3D degli scenari

Editor dedicato condiviso tra standalone web (`?editor=1`) e app Bevy
(`scenario-editor`): territorio vuoto, apertura e clonazione degli scenari,
pennelli di quota e combustibile, edifici, strade e luoghi,
proprietà, meteo, annulla/ripristina e import/export JSON completo.
La bozza è indipendente dall’esercitazione. I salvataggi sono territori JSON
nella libreria nativa oppure territori in localStorage nel browser, disponibili
anche all’editor pack e all’avvio dell’esercitazione. Vedi [uso e limiti](scenari.md#editor-scenario-3d).
L’editor verifica lo scenario mentre si modifica (contatori rossi sulle schede, errori accanto ai campi) e la scheda «Borghi» ha una mappa con zoom, selezione a rettangolo, annulla e suggerimenti: vedi [Borghi](scenari.md#borghi-districts).

## Solo l’EXCON agisce: azioni dalle risposte (2026-10-29)

I player **non propongono azioni**: ricevono inject e messaggi e rispondono **a
parole**. Nessun modulo d’azione, nessuna proposta da approvare, nessuno stato di
approvazione nella pagina del player. Solo l’EXCON cambia il mondo, e solo con un
`Command` applicato da `sim::World::apply`.

- **Pagina del player**: mappa della situazione; inject e messaggi con risposta
  libera (`POST /api/player/<role>/actions` con `kind: "communication"`, stato
  «Inviata»); accanto al testo la **guida alla risposta** (cosa, dove — zona come
  F4, lato di zona F4:N, luogo o coordinate —, con quali mezzi, entro quando, chi
  si informa, con un esempio) e il materiale di decisione del ruolo (competenze e
  risorse disponibili ora; descrizione e missioni nella scheda Funzione). Un clic
  sulla mappa scrive nella risposta la zona o l’edificio. La stessa guida, in una
  riga, sta sopra la risposta a una domanda della sala operativa.
- **Lettura AI per l’EXCON** (`llm::tasks::review_reply`, `exercise::reply_review`,
  `server::review`): ogni risposta di un player a un inject è letta per intero
  (inject, risposta, domande inviate, risposte) fuori dal mutex. L’AI propone
  azioni **solo dal catalogo** (`sim::Command`), ciascuna con la **fonte** (ruolo e
  parole citate, verificate sul testo dei player: senza citazione l’azione è
  scartata), i **parametri** e le **note** (ETA, fattibilità, incongruenze). Ogni
  suggerimento ha la verifica di fattibilità del simulatore.
- **Incompleti**: un suggerimento a cui mancano parametri obbligatori del suo
  comando (`sim::missing_params`, es. `CutFireLine` senza `from`/`to` né `along`)
  resta in lista come **incompleto** con i campi mancanti evidenziati: mai
  scartato, mai indovinato. I completi si eseguono subito, senza aspettare i
  chiarimenti.
- **Domande**: l’AI scrive **una** domanda per player che raccoglie tutte le
  lacune, nella lingua del player e con la voce del personaggio EXCON (mittente
  dell’inject o `default_persona`), richiamando la guida alla risposta; se il
  modello non la scrive, una domanda standard (it/en) copre comunque ogni lacuna.
  **Ogni domanda è una bozza**: l’EXCON la modifica e la invia con un clic
  (`POST /api/master/review/send/<id>`); nulla parte da solo, nemmeno le domande
  banali. Può anche scrivere una domanda propria (`ask`) o scartarla (`discard`).
  L’AI può proporre una domanda a un **altro ruolo** (incongruenza tra ruoli: il
  DOS usa il bulldozer del Comune → chiedere al Sindaco?), sempre come bozza.
- **Risposta e rilettura**: la risposta del player alla domanda (stesso box
  «Rispondi», `reply_to` = la domanda) è collegata alla domanda; l’AI rilegge tutto
  il thread e la console mostra la **differenza** (completate / modificate / nuove).
  Le decisioni già prese restano: un’azione eseguita non cambia mai, una ignorata
  resta ignorata se la rilettura dice lo stesso, i valori modificati dall’EXCON
  restano e l’AI completa solo ciò che manca. Le letture vecchie sono scartate
  (revisione).
- **Console EXCON** (Risposte, in alto, «Azioni dalle risposte»): un gruppo per
  risposta (ruolo, inject, risposta in una riga, stato della lettura, differenza),
  una riga compatta per suggerimento con lo stato leggibile a colpo d’occhio —
  **Completa**, **Incompleta** (con «Mancano: …»), **Incompleta · domanda da
  rivedere**, **Incompleta · attende risposta**, **Eseguita**, **Non eseguita**,
  **Ignorata** — e i pulsanti Esegui / Modifica / Ignora; il dettaglio apre i
  parametri modificabili, le note AI e il risultato. Le domande hanno gli stati
  **Domanda da rivedere**, **Inviata · attende risposta**, **Risposta ricevuta**.
  «Chiudi revisione» chiude il ciclo quando l’EXCON vuole (le bozze non inviate
  sono scartate); domande aperte e azioni da decidere restano visibili anche nel
  pannello del turno (chip «N domande aperte · M azioni da decidere») e nel badge
  della scheda Risposte.
- **Esecuzione**: `POST /api/master/review/execute/<id>` (eventuali `params`
  modificati) costruisce il `Command` e lo verifica per il ruolo che lo esegue
  (risorse e autorità di quel ruolo); come ogni azione EXCON sul mondo va **in
  coda** (stato «In coda · al prossimo avanzamento», pulsante «Metti in coda») e si
  applica con quel ruolo al prossimo avanzamento del tempo, rifacendo la verifica
  (`when: "now"` la applica subito, `at_clock` la programma). Annullata dalla coda
  delle azioni EXCON torna «Completa». Un’azione non fattibile non entra in coda e
  resta da modificare. Il log registra `action_executed` con
  `review_item`, ruolo, citazione. Il rollback dimentica le revisioni dei turni
  annullati.
- **Zone come luoghi**: un lato di zona (`F4:N`) vale anche come luogo puntuale
  (un quinto verso il centro della zona dal punto medio del lato).
- Le vecchie proposte con approvazione restano nel modello e nell’API (proposte
  dell’EXCON da prompt, fixture, banco), ma la pagina del player non le produce
  più. Test: `crates/exercise/tests/reply_review.rs` (scenario di accettazione),
  `crates/server/tests/reply_review.rs`, `llm::tasks` (fedeltà della lettura),
  `frontend::tactical` (nessun controllo d’azione nella pagina del player).

## Situazione EXCON: impatto, anteprima di fine turno, allerte

La scheda **Situazione** (solo EXCON, solo desktop) mostra a EXCON l'effetto dell'incendio e cosa accadrà entro la fine del turno, così può scrivere gli inject in anticipo. **Gli inject restano manuali**: nulla viene inviato o pianificato da solo.

- **Impatto ora** (`GET /api/master/impact`, `sim::impact`): righe per strutture, rifugi, edifici toccati dal fuoco e strade non integre (le abitazioni semplici sono solo contate per settore finché il fuoco non le raggiunge); totali e aggregati per settore con variazione rispetto all'inizio del turno (snapshot di rollback): edifici minacciati/danneggiati/distrutti, persone presenti, in evacuazione, arrivate, a rischio, strade tagliate, riempimento dei rifugi.
- **Anteprima deterministica** (`GET /api/master/situation/preview[?have=<key>]`, `exercise::situation`): una copia del mondo (`World` è `Clone`, comprese le sequenze casuali dell'ensemble) viene portata alla fine pianificata del turno **fuori dal mutex**, con le stesse regole di una risoluzione reale (`begin_resolution` + `step_resolution`): proposte approvate, azioni pianificate attive (rispettando disattivazioni e inject saltati), passi da 1 s. Con lo stesso seme e senza altre decisioni il risultato coincide con l'avanzamento reale (test `preview_equals_the_real_resolution`). Restituisce, per elemento, il primo istante di minaccia/danno/distruzione/taglio/svuotamento, l'impatto e la mappa del fuoco a fine turno. Si ricalcola in background (un solo lavoro, debounce 600 ms, risultato scartato se la chiave non coincide più) mentre la scheda è aperta; in pausa durante la risoluzione. Nel browser (standalone) il calcolo gira sul thread principale: circa 0,2 s su Valverde e 1 s su Selvariva senza fuoco esteso, alcuni secondi con un incendio ampio. Non è la previsione ai giocatori (`fire::forecast`).
- **Allerte per l'EXCON** (situational awareness; `GET /api/master/situation/suggestions`, `exercise::suggestions`): dicono a chi gestisce l'esercitazione cosa succede se nessuno interviene (es. «Scuola: fuoco previsto verso le 14:30, ancora 120 persone all'interno, evacuazione non avviata»), non sono messaggi per i giocatori. Regole `building_reached` (struttura con persone raggiunta prima del termine dell'evacuazione, priorità scuole/ospedali), `houses_reached` (abitazioni per settore), `place_without_exit` (abitato senza uscita stradale), `evac_route_cut` (evacuati in viaggio su una strada che verrà tagliata), `shelter_front` / `shelter_over` (rifugio vicino al fronte o oltre capienza), `task_route_cut` (percorso dei mezzi dispiegati). Ogni allerta ha id stabile `regola:oggetto`, gravità, testo `alert` it/en, momento dell'impatto (`reach_s`) e ora entro cui reagire (`at_s`), ruoli che possono intervenire (dalle azioni consentite) e una bozza di inject (titolo, testo, evidenza). EXCON può **ignorarla** oppure **creare un inject** ad hoc partendo dalla bozza: la modifica, può chiedere all'AI di riscriverla con la voce di un mittente (`POST /api/master/situation/rewrite`, senza AI il testo resta invariato), poi la **invia ora** (`POST /api/master/inject`), la **pianifica** in timeline (`POST /api/master/situation/plan`: aggiunge un inject MSEL all'ora scelta, da inviare a mano) o la **scarta** (`POST /api/master/situation/mark`). Le coppie scartate/inviate/pianificate non vengono riproposte; il rollback dimentica quelle successive.
- **Aiuto AI sulle allerte** (`exercise::alert_brief`; senza AI tutto resta com'è, con il motivo):
  - **Crea inject** chiede subito all'AI una bozza (`POST /api/master/situation/draft`, task `draft_inject`): un mittente verosimile (112, dirigente scolastico, squadra via radio…) riferisce solo ciò che vede ora, senza orari futuri né indicazioni ai giocatori. Se EXCON ha già iniziato a scrivere, il suo testo resta. Senza AI resta la bozza della regola. Non invia nulla.
  - **Cosa stanno facendo i giocatori?** (`POST /api/master/situation/brief`, task `situation_brief`, solo a richiesta) incrocia le allerte con risposte, messaggi, conversazioni e dispiegamenti dei ruoli che possono intervenire: una nota per allerta (es. «Nessuno ha menzionato la scuola») e un riepilogo, con l'ora del quadro. Le note per allerte inesistenti vengono scartate.
- La Timeline mostra solo un chip «N allerte» che porta alla scheda.

## Scopo e ruoli

Il simulatore è pensato per esercitazioni da tavolo su emergenze. Lo scenario
attuale rappresenta un incendio boschivo nella valle sintetica di Valverde.

- **EXCON** prepara e invia inject, controlla l'esercitazione e il tempo,
  assegna risorse, approva o rifiuta le azioni dei partecipanti e può intervenire
  direttamente nel modello.
- **Partecipanti** decidono entro il ruolo, le autorizzazioni, le informazioni
  e le risorse definiti dallo scenario, ma non agiscono sul mondo: rispondono in
  linguaggio libero nella propria lingua e l’EXCON attua le azioni (vedi «Solo
  l’EXCON agisce»).
- **Valutatori** dispongono di una vista in sola lettura delle evidenze e del
  debrief.

## Flusso dell'esercitazione

### Turni e avanzamento del tempo

Il turno è una **finestra [inizio, fine pianificata) sulla timeline dello scenario**: l'inizio è il tempo simulato a cui il turno si è aperto, la durata è `turn_minutes` (campo minuti nella striscia del turno). Un turno ha tre fasi: *Preparazione* (ordini aperti, EXCON approva proposte e invia inject), *Risoluzione* (la simulazione scorre a passi di 1 s; le azioni degli inject scattano al loro istante) e *Chiusura* (log, riepiloghi AI, snapshot, apertura del turno successivo).

La scheda Timeline è pensata per il PC e la **lista degli inject** (con le loro azioni come sotto-elementi) è l'elemento principale: da 1200 px di larghezza occupa tutta l'altezza a sinistra (righe compatte con ora, tipo, titolo, anteprima del testo, stato e invio), mentre una colonna a destra contiene il pannello del turno e il **dettaglio dell'elemento selezionato** (testo completo, obiettivo, destinatari, «Invia…»; di default il primo elemento da gestire). Sotto i 1200 px il pannello torna sopra la lista e il dettaglio si apre nella riga. Il **pannello del turno**, compatto e unico proprietario di stato e tempo (l'intestazione della pagina mostra solo «Turno N · fase»): una riga con turno, finestra, stato («⏸ Fermo · Day 1 13:30» oppure «▶ In corso 13:06 → 13:30 · 60× · rimangono N min simulati»), selettore di **velocità** (max, 10×, 60×, 120×, `POST /api/master/settings {speed}`, valido anche prima di avanzare e durante la risoluzione), **⏸ Ferma qui** durante la risoluzione e la durata del turno (il pulsante dice «Sposta la fine a HH:MM»: cambiare la durata sposta la fine pianificata); la **traccia** con asse orario (tacche ogni 10 minuti, ora di inizio e di fine), cursore «ora», tratto in corso, un segno per ogni avanzamento intermedio e i marcatori degli inject (cerchio, senza marcatori per le azioni): gli elementi allo stesso istante sono un solo badge con il conteggio, quelli pianificati prima del turno e ancora da gestire stanno in una zona «in ritardo» a sinistra; il clic porta alla riga nella lista; e una riga di controlli con il chip **«Da gestire ora: N inject»** (clic: evidenzia le righe), l'ultimo avanzamento in una riga (azioni eseguite con il loro esito, azioni giocatore applicate, numero di eventi di simulazione) e gli avanzamenti:

- **▶▶ Avanza al prossimo inject (HH:MM)** (pulsante primario `#advance-next`): avanza all'istante pianificato del prossimo inject ancora da inviare e non saltato, **mai oltre la fine pianificata del turno**: se la fine del turno viene prima, l'etichetta dice «Avanza e chiudi turno N (HH:MM)», il turno si chiude lì e il clic successivo prosegue nel turno nuovo. Senza inject successivi avanza alla fine del turno (e lo chiude). Il server calcola il bersaglio (`until: "next_inject"`, `Exercise::advance_target`). La scansione non si ferma sulle azioni anticipate: ci passa attraverso, fermando la simulazione esattamente al loro istante. Dopo la scansione l'inject raggiunto è **da gestire** («due»): evidenziato nella lista con **Invia** e **Non inviare** (EXCON decide se inviarlo; «Non inviare» lo marca come saltato, non blocca più l'avanzamento e si può **Ripristinare**).
- **Avanza di [N] min ▶**: avanzamento intermedio libero; il turno resta aperto.

**Revisione prima di ogni avanzamento.** Se ci sono inject non ancora inviati il cui istante è raggiunto o attraversato (compresi quelli in ritardo di turni precedenti) oppure azioni anticipate che l'avanzamento esegue da sole, si apre una sola finestra con titolo che dice dove va il tempo e cosa succede al turno («Avanza a Day 1 13:05 · il turno resta aperto», «Chiudi turno 1 alle 13:30 · si apre il turno 2»). Ogni inject ha «Invia ora» (destinatari predefiniti), «Destinatari…» e «Non inviare»; ogni azione anticipata ha l'interruttore «esegui». Con inject non inviati il pulsante diventa «Avanza comunque · …». Se non c'è nulla da gestire si avanza direttamente. Non c'è più una decisione Esegui/Salta/Rinvia per le azioni: le azioni seguono il loro inject.

**Azioni EXCON in coda (playtest 29/10).** Mentre il turno è aperto agli ordini i player leggono gli inject e rispondono: un'azione EXCON sul mondo presa in quel momento (innesco, linea, lancio, spegnimento, evacuazione, avviso, drone, stato edificio, meteo, chiusura strade…) **non cambia la situazione sotto le loro mani** ma va in coda e si applica quando il tempo si muove: all'inizio del prossimo avanzamento, prima delle proposte approvate, oppure a un'**ora simulata scelta**, indipendente dagli inject (es. l'innesco alle 13:00 e l'inject di segnalazione alle 13:20). Unico punto d'ingresso: `Exercise::apply_or_queue_excon(cmd, ExconWhen::{Play|At(s)|Now}, origin)` (`crates/exercise/src/queue.rs`), usato da `POST /api/master/command`, dall'applicazione dell'anteprima rivista (`/api/master/preview/apply`, sempre `Now`: è l'«applica adesso» rivisto e correggibile del controllo dell'esercitazione, con clock fermo) e dalle azioni degli esiti narrativi. Restano immediati: tutto prima dell'avvio (preparazione), `AdvanceTime` (controllo del tempo), `ReturnResource`, `AdjustResource` e `SetDeploymentTravel` (contabilità/configurazione delle risorse che decide cosa si può approvare durante la pausa), e la scelta esplicita «Subito, a turno aperto (eccezione)». Le azioni degli inject a offset 0 restano contestuali all'invio (sono ciò che il messaggio racconta); gli script e i test che chiamano `direct_command` restano immediati. Un'azione in coda è un `ActionRun` con `queued`: gira all'istante esatto durante la risoluzione a passi di 1 s e con il clock interno al turno (invariante rispetto al passo), compare nell'anteprima Situazione come nella risoluzione reale e segue il rollback: quelle decise nei turni annullati spariscono, quelle decise prima ed eseguite o annullate dopo l'istante ripristinato tornano in attesa. Il comando è validato quando entra in coda e di nuovo quando gira. In Controllo ogni strumento sulla mappa e il meteo hanno «Quando» (al prossimo avanzamento, all'ora simulata, subito) e il pulsante dice «Metti in coda»; la coda (cosa, quando, «Cambia ora», «Annulla», storico applicate/annullate) è in cima agli strumenti di Controllo e, se non vuota, sotto il pannello del turno nella Timeline. Si modificano solo le azioni messe in coda nel turno aperto (le altre si annullano e si rimettono, così il rollback non deve disfare modifiche). API: `POST /api/master/command` accetta anche `{commands|command, when: "play"|"now", at_clock: "HH:MM", at_s}` (un comando o una lista nudi = `play`; i risultati in coda hanno `queued`, `run_at_clock`); `POST /api/master/excon-actions/<id>/cancel {reason}`, `POST /api/master/excon-actions/<id>/edit {command?, at_clock?|at_s?|when: "play"}`; `GET /api/master` espone `excon_queue` (le azioni in coda non sono in `inject_actions`).

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
   testo, lasciando la bozza modificabile al mittente. Ogni nuova
   esercitazione (avvio, riavvio, nuovo scenario; server e standalone) chiede
   a ogni partecipante la lingua, anche se il ruolo è ora di un’altra persona:
   quella del ruolo nello scenario è proposta (`Exercise::with_language_choice`),
   poi il player attende l’avvio. Nello standalone la richiesta compare nella
   scheda di ciascun ruolo; le sessioni salvate prima di questa modifica
   (`SavedSession::language_choice` assente) si riprendono con le lingue dello
   scenario. Sul server la lingua della console EXCON è chiesta di nuovo quando
   l’esercitazione torna al briefing (riavvio). L’EXCON
   vede chi manca accanto ad “Avvia esercitazione”, che resta disabilitato
   finché tutti hanno scelto; “Avvia comunque” (`POST /api/master/start
   {force:true}`) parte senza attendere. La scheda iniziale
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
   conversazioni nate dall’inject (sotto le risposte) e in fondo, senza pulsanti né modali, la scelta di come rispondere («Risposta diretta
   al messaggio» oppure «Inizia una conversazione»). La risposta diretta apre il modulo
   qui sotto; la conversazione prosegue in linea nella chat (scelta dei player e primo
   messaggio). Chi ha solo letto l’inject via conversazione ha invece il pulsante **Rispondi**, che apre direttamente il modulo. È l’unico percorso: niente «+ Nuova azione», niente
   «Proponi azione» dalle chat, niente selettore «Rispondi a…» (vedi «Risposte per
   inject» sotto). La risposta diretta apre un modale quasi a schermo intero con mappa e
   modulo: destinatario (il mittente fittizio dell’inject; solo dalla risposta in una conversazione, i suoi partecipanti),
   tipo (**comunicazione** o **decisione/azione**), il testo «Descrivi la tua
   risposta o decisione», e per una decisione il **fondamento** obbligatorio e al
   più **una** azione di simulazione associata (dal modulo o dai **suggerimenti
   AI**: quando il player smette di scrivere, pausa di 2,5 s, almeno 20 caratteri,
   l’AI propone azioni del suo ruolo; associarne una sostituisce la precedente).
   Ogni inject può ricevere più risposte.
3. La risposta è verificata prima di produrre effetti. EXCON vede inject inviati e risposte nella tab Risposte (stato per
   destinatario, badge delle approvazioni), ne controlla la fattibilità e decide:
   approva, rifiuta oppure chiede un chiarimento (la proposta resta aperta con i suoi comandi; la
   risposta del player e un semplice messaggio a EXCON, tradotto ma mai
   interpretato dall'AI: la proposta torna `pending` e l'evento
   `clarification_answered` e registrato). Modificando i
   parametri la fattibilità è ricalcolata in automatico (`/revalidate`).
   La tab Risposte mostra solo gli inject effettivamente inviati e, per ciascuno,
   l'intero thread in ordine (inject, risposte con tipo, destinatario e stato,
   chiarimenti EXCON) e sotto le conversazioni tra player nate dall’inject. Un badge
   «💬 N conversazioni» sulla riga apre il visore delle conversazioni (selettore
   verticale a sinistra, una conversazione alla volta a destra, con gli stessi widget
   della conversazione del player); nel dialogo della risposta **Ispeziona la
   conversazione** apre direttamente quella da cui la risposta è nata.
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

Nella console (Risposte) la scheda di una proposta mostra la richiesta del player,
l'interpretazione e, sotto «Cosa succede se approvi», una riga leggibile per azione
(luoghi come zona e luogo vicino, edifici per nome) con l'esito della verifica.
Ogni riga ha la casella «Approva questa azione» e, se esclusa, un motivo facoltativo
per il player. «Modifica» apre, già compilato con l'azione: per un'azione di un
player il modulo e la mappa del player (menu del ruolo, più zone, rettangoli,
luoghi, bordi `F4:N`, strade ed edifici da elenco; più zone diventano più ordini);
per i comandi EXCON lo strumento del Controllo che li costruisce (mappa o meteo).
I campi che lo strumento non imposta (es. destinazione, larghezza) restano; i
comandi senza editor (es. `AdjustResource`) aprono i parametri.
Messaggi e inject proposti compaiono come «Messaggi/Inject che partiranno»
(mittente → destinatario, titolo e testo modificabili; per gli inject destinatari
«Tutti i player» o ruoli scelti) e i limiti di risorse come tabella per risorsa,
generale e per fase (`proposal_drafts.rs`); il JSON resta in «Dettagli tecnici». Il pulsante diventa
«Approva N di M azioni». Una proposta senza azioni offre «Prendi atto». Nota,
persona e pulsanti di decisione stanno in fondo; il JSON dei comandi, le azioni non
interpretabili e la cronologia sono in «Dettagli tecnici», chiusi per default.

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
(`Exercise::apply_or_queue_excon`, come i comandi di Controllo: a turno aperto vanno in
coda e si applicano al prossimo avanzamento; voce `excon_action_queued` nel log con
`origin.origin: narrative_outcome`, poi `master_command` con `queued_origin`):
vengono verificate tutte su una copia del mondo e applicate solo se tutte valide. L’esito
conserva i `log_seq` degli interventi (`interventions`); il player vede la narrazione
con il personaggio e gli effetti (`effects`). L’interfaccia (Risposte) riusa le
finestre degli strumenti di Controllo con selezione sulla mappa
(`staged_commands.rs`, più un campo JSON per comandi come `CloseRoad`). Registro,
valutatore e debrief distinguono gli interventi EXCON dalle esecuzioni dei player;
l’approvazione non indica una missione completata. Il salvataggio della sessione
(journal) li ripristina; il rollback ripristina il mondo e annulla esiti e interventi
dei turni annullati.

### Risposte per inject (playtest 28/10)

Ogni invio con **Rispondi** è una proposta salvata nella storia dell’inject, anche
senza effetti meccanici (`response_recorded` nel log), visibile al player e a EXCON.
Corpo di `POST /api/player/<role>/actions`, oltre a `narrative`, `commands`,
`reply_to`, `conversation`, `basis`, `attachment`:

- `kind`: `communication` (solo informazione: stato `informational` subito, senza
  fondamento, senza approvazione EXCON, senza azioni; non blocca il flusso) o
  `decision` (fondamento obbligatorio — piano/procedura, legge/norma, giudizio
  operativo — anche se l’azione è solo narrata; revisione EXCON). `kind` è
  obbligatorio per ogni chiamante (422 se manca) e non c’è fondamento predefinito;
  anche `Exercise::player_actions` richiede il fondamento esplicito.
- `to_persona` / `to_players`: destinatario. Predefinito: il mittente fittizio
  dell’inject (`addressed_to.persona`). Con `to_players` la risposta è anche
  pubblicata, tradotta per ciascuno, in una conversazione collegata all’inject con
  quei player (quella da cui si scrive, una esistente con gli stessi partecipanti o
  una nuova) e il messaggio porta `proposal`.

Stati mostrati (player ed EXCON, `i18n_risposte::response_state`): **Inviata**
(comunicazione), **In valutazione** (`pending`), **Approvata**, **Eseguita**
(`applied`), **Esito registrato** (EXCON ha registrato l’esito narrativo, ad es. la
pubblicazione dell’avviso: evento distinto dall’approvazione), più chiarimento
richiesto, approvata in parte, rifiutata, presa visione, non eseguita. Non c’è più
«Approvata · esito in attesa».

Una risposta data da una conversazione compare **una sola volta** in
`inject_responses`: sotto la riga dell’inject di chi risponde se ne ha una copia,
altrimenti sotto la riga dell’inject citato dalla conversazione (prima compariva due
volte, attribuita anche a chi aveva aperto la conversazione). Ogni riga mostra le
conversazioni sull’inject a cui partecipa il destinatario.

Un messaggio in una conversazione («vado in ricognizione») è solo testo: non è mai
interpretato né diventa una missione; per registrare una decisione si usa Rispondi.
Test: `crates/exercise/tests/risposte.rs`, `crates/server/tests/risposte.rs`.

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
(**Inizia una conversazione** nel dettaglio dell’inject: scelta dei player e primo
messaggio); poi prosegue come thread autonomo con l’inject citato in testa
(riferimento fissato all’apertura) (`exercise::conversations`). È privata ai partecipanti verso gli altri player; EXCON
invece legge la trascrizione completa (`linked_conversations[].messages`: autore, ora,
testo originale e inglese, citazioni; `message_count`), nel visore delle conversazioni
di Risposte (badge sulla riga dell’inject o **Ispeziona la conversazione** sulla risposta). Il testo compare anche nel log (`conversation_message`), nel debrief e
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

### Base della risposta e allegato PDF

- Il modale di risposta del player (`tactical.rs`) è in blocchi: **base della risposta**
  (Piani / Leggi / Giudizi operativi, `basis` = `plan|law|operational_judgment`,
  predefinito giudizio operativo), eventuale **PDF**, **narrazione** guidata dal formato
  chi/cosa/come/dove/quando (testo di aiuto e placeholder localizzati in `i18n_reply.rs`,
  tutte le 12 lingue), **azioni**. La base diventa `Proposal.evidence.basis.kind`
  (`BasisKind::Law` è nuovo) e compare in `inject_responses[].replies[]`, in
  `pending_proposals[]` e nei messaggi `reply` del `thread` come `basis` e `document`;
  il contesto AI del player riceve la riga `[basis] plans|laws|operational judgement`
  (+ nome del PDF).
- `attachment {name, content_type: "application/pdf", data: base64}` è ammesso solo con
  base piani/leggi. Il server controlla tipo dichiarato, firma `%PDF-` e dimensione
  (max 10 MB, `exercise::files::MAX_PDF_BYTES`; max 20 file per ruolo, 100 totali) prima di
  salvare qualunque cosa. I byte restano in memoria nell'`Exercise` (`exercise::files`), non
  entrano in log, `/api/master` né nell'export della cronologia (solo `{id,name,size}`).
  Un rollback segna le proposte come annullate ma conserva messaggi e documenti (storico).
- `GET /api/attachments/<id>` (JSON con `data` base64, identico in server e standalone):
  solo EXCON e il ruolo autore; valutatori 403, altri ruoli e id ignoti "document not found".
- EXCON: il modale Risposte (`risposte.rs`) è orizzontale, conversazione a sinistra e
  risposta da gestire a destra; il chip "PDF" apre `pdf_view.rs`, che disegna il file con
  pdf.js (`pdfjs-dist@4.10.38`, copiato in `crates/frontend/assets/vendor/pdfjs` e incluso
  nel WASM con `include_str!`: nessun CDN, funziona offline e nel `ttx.html` a file unico).
  Resta anche il link a un blob per aprire/scaricare con il visualizzatore del browser.
### Azioni suggerite dall’AI sulle narrazioni

Il vecchio assistente «Descrivi le azioni con l’assistente AI» (bozza privata
`POST /api/player/<role>/draft` + `/draft/submit`, task LLM `prepare_decision`,
`exercise::drafts`) è stato rimosso. Al suo posto un solo task LLM,
`suggest_actions` (`llm::tasks::SuggestScope`), con due ambiti; il modello
risponde `{actions, quotes, reasons, summary}` in JSON (citazioni: vedi
«Fedeltà delle formulazioni AI»), l’esercitazione lo valida
(`Exercise::check_suggestions`: solo `Command` validi del ruolo autore, niente
doppioni né azioni già nella risposta/proposta, verdetto di fattibilità dopo le
proposte approvate) e **nulla cambia**: i suggerimenti sono bozze che un umano
aggiunge. Il contesto si raccoglie sotto il mutex, il modello gira fuori; se
l’AI manca la risposta è `status: ai_unavailable` e l’interfaccia lo dice.

- **Player, automatico**: `POST /api/player/<role>/suggest-actions {narrative,
  reply_to?, conversation?, staged?}` (solo il player stesso; fase ordini). Il
  contesto è solo ciò che il ruolo sa (`Exercise::player_suggestion_context`):
  mappa del ruolo (`player_map`), risorse, schieramenti e proposte proprie del
  turno, thread visibile dell’inject (`thread_evidence`), conversazioni collegate
  a cui partecipa, chat con gli altri ruoli e la sua conversazione continua con
  l’AI. Il modale di risposta mostra «Azioni suggerite dall’AI» sotto la
  narrazione (`tactical.rs`, `data-work-suggestions`), con il motivo e l’eventuale
  esito negativo della verifica; «Aggiungi»/«Aggiungi tutte» le mette tra le
  azioni della risposta, che segue la normale approvazione EXCON.
- **EXCON, su richiesta**: nella revisione (Risposte, widget `Proposal`) una
  risposta con narrazione ha il pulsante **Suggerisci azioni**
  (`POST /api/master/proposals/<id>/suggest-actions`). Qui l’AI legge lo stato
  completo (mondo, schieramenti, design, contabilità risorse, mappa del player) e
  tutte le conversazioni (thread dell’inject per tutti i destinatari, conversazioni
  collegate all’inject o con il ruolo, chat del ruolo, conversazione AI del
  ruolo). Mentre la proposta è aperta le bozze si aggiungono alle azioni da
  approvare (ricontrollate da `/revalidate`, poi approvazione anche parziale);
  dopo l’approvazione vanno agli interventi dell’esito narrativo. Aggiungere
  azioni a una narrazione pura la trasforma in risposta con azioni
  (`carries_actions`, `excon_actions = true`, evento `narrative_actions_added`);
  togliendole tutte torna pura.

- Persistenza: il body completo delle POST è ora scritto nel journal (`server/src/body.rs`;
  prima `Data::peek` lo troncava a 512 byte e i corpi lunghi non si rigiocavano). Limite
  corpo 15 MiB. Nel salvataggio del browser standalone (localStorage, ~5 MB) i PDF oltre
  ~384 KB sono sostituiti da un PDF segnaposto: dopo un ricaricamento restano nome e
  riferimento ma il contenuto non è più disponibile.


API: `POST /api/player/<role>/actions {kind, narrative?, commands?, text?, reply_to?,
conversation?, basis?, attachment?, to_persona?, to_players?}` (unica rotta di
risposta: `kind` obbligatorio, `basis` obbligatorio per `decision`, serve almeno
narrazione o un comando; la vecchia `POST /api/player/<role>/narrative` è stata
rimossa); `POST /api/player/<role>/conversations {delivery, participants,
text}` e `POST /api/player/<role>/conversations/<id>/messages {text, reply_to?}`;
`GET /api/player/<role>` espone `linked_conversations` (con `quote`, messaggi nella
lingua del ruolo, `responses`), `GET /api/master` espone `linked_conversations`
(riferimenti) e, nelle righe di `inject_responses`, `conversations`, `role` e
`conversation` delle proposte. `POST /api/master/proposals/<id>/narrative-outcome
{text, language?}` registra l’esito. Limite noto: i partecipanti si fissano alla
creazione.

## Fedeltà delle formulazioni AI (playtest 28/10)

Un invito comunale a evitare abbruciamenti era diventato «divieto di
abbruciamento (Issue warning to zone F4)». Regole e controlli (`llm::fidelity`):

- I prompt di `parse_reply`, `suggest_actions`, traduzioni, riepilogo di turno e
  aggiornamento di situazione includono `FIDELITY_RULES`: quantità, condizioni,
  luoghi e forza giuridica restano quelli del player (un invito resta un invito;
  divieto/ordinanza/obbligo solo se il player li dichiara), nessun contenuto,
  luogo o potere inventato; un’intenzione («manderò una squadra») è
  un’intenzione; una narrazione non è eseguita dal simulatore.
- Ogni azione proposta dall’AI porta una **citazione letterale** (`quotes`) del
  testo del player: se la citazione non compare nel testo l’azione è scartata
  (contata in `dropped`); i suggerimenti mostrano la citazione davanti al motivo
  («parole del player» → motivo). Un’azione senza citazione è ancora accettata.
- `parse_reply` classifica la `modality` (decisione, intenzione, invito,
  richiesta, resoconto, domanda) ed etichetta il riepilogo per l’EXCON
  («Intention (not executed): …», «Invitation/appeal (not a ban or order): …»).
  Resoconti e domande non producono azioni; in chat diretta solo le decisioni.
- Un riepilogo, motivo o messaggio con lessico vincolante (ban, divieto,
  vietato, ordinanza, obbligo, Verbot, interdiction…, non negato) assente dal
  testo del player è sostituito dalle parole del player; una traduzione inglese
  che lo introduce è segnalata con «CHECK WORDING».
- La scheda della proposta EXCON mostra anche il **testo originale del player**
  accanto alla traduzione.
- Gli effetti di un esito narrativo nella vista player sono descritti nella
  lingua del player (`effect_commands`), non con la descrizione inglese del server.

Test offline in `crates/llm/src/{fidelity,tasks}.rs`; prova dal vivo
`TTX_LIVE_LLM=1 cargo test -p llm --test live`.

## Traduzioni dell’interfaccia player

Le stringhe player sono chiavi di `i18n.rs` (it/en) con le altre lingue nei
cataloghi `i18n_lang`, `i18n_reply`, … e in `i18n_more.json` (generato da
`scripts/i18n/player_catalogue.py`, con `--check` elenca le mancanze). I testi
scritti in linea come coppia italiano/inglese passano da `i18n::pair`. I test di
`i18n_more` falliscono se una chiave o una coppia manca in una lingua, o se una
schermata solo player usa il catalogo EXCON (`ex`, `exfmt!`) o un ramo
italiano/inglese. `ex()` in un pannello player non italiano mostra l’inglese
(o la traduzione di `i18n_more`), mai l’italiano.

## Banco di prova (sviluppo)

Pagina solo di sviluppo (feature cargo `bench`, `?bench=1` nello standalone,
`bench.html` sul server) che affianca le schermate EXCON e player di sei flussi
brevi (inject e narrativa, suggerimenti AI al player e all’EXCON, chiarimento,
conversazione tra player, decisione per azione) su un finto server in memoria,
senza AI né simulatore. La schermata di lavoro del player (`tactical.rs`,
`inject_panel.rs`, `clarify.rs`) è disegnata da `player_view::PlayerView`, che
`App` costruisce dal proprio stato e il banco dal finto server. Dettagli e
avvio: [banco-di-prova.md](banco-di-prova.md).

## Banco simulatore (sviluppo)

`?simbench=1` nello standalone: vista 3D a sinistra, console Controllo
dell’EXCON a destra, tempo avviabile, in pausa o a salti; gli strumenti EXCON si
applicano subito, anche mentre il tempo scorre. Nessun salvataggio. Dettagli:
[banco-simulatore.md](banco-simulatore.md).

## Capacità modellate

- Propagazione del fuoco influenzata da terreno, combustibile e meteo, calcolata
  con più realizzazioni; esposizione e danni agli edifici. Un edificio è
  **danneggiato/distrutto** solo se il fuoco confermato (celle raggiunte da
  almeno `cutoff` delle realizzazioni e in fiamme ora) brucia a contatto con il
  suo lotto, entro `sim::contact_radius_m` (50 m, o una cella e mezza su griglie
  più grosse): 300 s di esposizione per il danno, 900 s per la distruzione. Fuoco
  più lontano (entro 300 m), alone d'incertezza dell'ensemble e carichi
  radianti/da faville del modello operativo lo rendono solo **minacciato**. Mappa
  Situazione, impatto, digest per l'AI ed eventi leggono tutti questo stesso
  stato (`sim::World::step_buildings`; test `crates/sim/tests/building_damage.rs`).
- Avvisi ed evacuazioni di edifici o zone, rifugi e destinazioni, traffico
  d'evacuazione e risorse di trasporto.
- Interventi antincendio, tra cui linee tagliafuoco, lanci d'acqua e autobotti.
- Chiusure stradali, viaggi delle risorse, richieste di rinforzi e gestione
  delle disponibilità.
- Avanzamento del tempo, aggiornamenti della situazione, rollback e registrazione
  degli eventi.

**Spegnimento di un'area disegnata** (EXCON, strumento Controllo «Spegni area
disegnata», comando `ExtinguishArea {polygon: [LOC, …]}`, almeno 3 vertici):
l'EXCON clicca i vertici sulla mappa (numerati, area chiusa e ombreggiata,
«Togli ultimo vertice»); all'applicazione il fuoco dentro il poligono si spegne
**subito**, in tutte le 32 realizzazioni, **senza cambiare la vegetazione**. In
PROPAGATOR una cella si propaga nell'istante in cui si accende, mettendo in coda
gli eventi di accensione dei vicini: lo spegnimento
(`propagator_core::Propagator::extinguish`) cancella gli eventi in coda *verso*
le celle dell'area e quelli di propagazione per contatto *da* esse (ogni evento
del fronte ricorda la direzione di provenienza), e le accensioni programmate
nell'area. Le celle già bruciate restano bruciate (combustibile consumato): le
celle in fiamme del fuoco «reale» appaiono bruciate e senza fiamme; per ogni
cella si registra l'istante di spegnimento, e le realizzazioni che l'avevano
raggiunta prima non contano più per la probabilità, così anche l'alone scompare.
Il combustibile non bruciato resta: se un fronte ancora vivo fuori dall'area la
raggiunge **dopo** lo spegnimento, può riaccenderla (contata da capo). Le faville
già in volo (spotting, spento per default) atterrano comunque. Lo stato vive nel
checkpoint del core e in `FireSnapshot` (rollback e «Correggi ultimo
intervento»). Fixture visiva: `scripts/visual-fixtures/extinguish-area.json`. «Spegni zona» e
«Spegni area circolare» (`Extinguish`) seguono la stessa regola (dal playtest del
30/10; prima rendevano non combustibile tutta l'area): una zona (`F4`) spegne
tutte le celle in fiamme dell'intera zona, un luogo con nome il suo `radius_m`
(un settore i suoi limiti, altrimenti 250 m), un cerchio il suo raggio; la
vegetazione non cambia (`World::extinguish_footprint`, `FireSim::extinguish_rect`
/ `extinguish` → `extinguish_area`).

Le azioni disponibili e i loro effetti dipendono dallo scenario e dalla modalità
del modello. Il modello operativo aggiunge routing stradale, code, viaggi,
interventi progressivi, risorse fisiche limitate e altri effetti operativi; il
modello storico mantiene regole più semplici. La vista 3D e le mappe presentano
lo stato e non prendono decisioni.

**Interventi visibili nella vista 3D** (`crates/viz/src/effects.rs`, solo
presentazione, animata in tempo reale e non fedele al secondo simulato):
ogni lancio d'acqua recente (≤ 15 min simulati) viene riprodotto come passaggio
del Canadair (quello del dispiegamento, o un velivolo sostitutivo controvento)
con cortina d'acqua che cade sul cerchio del lancio e si apre in nebbia; le
autobotti al lavoro lanciano un getto ad arco sulle celle in fiamme più intense
entro ~95 m (vapore dove colpisce fiamma), oppure bagnano il combustibile verso
il fuoco più vicino; le squadre che tagliano la linea sollevano polvere; le celle
bagnate hanno una velatura blu. Il drone ha un modello Blender dedicato
(quadricottero, `scripts/build_unit_models.py`): in posizione orbita sulle zone
monitorate, con rotori e luci animate, contorno delle zone e cono di scansione
(simboli). Le evacuazioni senza traffico modellato mostrano auto (abitazioni) e
bus (scuole, ospedali) che partono distanziati dagli edifici in evacuazione e
percorrono la rete stradale verso il rifugio indicato o, in mancanza, verso
l'uscita più lontana dal fuoco evitando le strade vicine all'incendio; con il
traffico del modello operativo si disegnano solo i veicoli simulati. Il
protocollo `view` porta a questo scopo `Deployment::action/zones`,
`Drop::t_s`, `BuildingDraw::evac_people/evac_to` e `ViewState::traffic_model`.
Fixture: `scripts/visual-fixtures/suppression.json` (autobotte a 8 min, lancio a
26 min, drone; modello operativo) e `evacuation.json` (modello storico).

## Confini del modello

- Le persone sono conteggiate per edificio e non simulate individualmente.
- Tempi, capacità, soglie e politiche sono ipotesi dello scenario per
  l'esercitazione.
- La simulazione è riproducibile a parità di configurazione e seme. Le risposte
  generate dall'AI possono variare; se l'AI non è disponibile, EXCON può
  interpretare manualmente le proposte.

- I codici delle zone restano visibili a ogni zoom senza coprire la mappa: in
  vista d'insieme le lettere delle colonne corrono lungo il bordo superiore e i
  numeri delle righe lungo il bordo sinistro della parte inquadrata (come gli
  assi di un grafico); quando una zona supera 150 px sullo schermo ogni zona
  mostra il proprio codice nell'angolo visibile. Testo di 10 px semitrasparente
  con alone sottile, senza riquadro; linee della griglia tenui.

- Drone di monitoraggio: risorsa `drones` (1, `fire_commander`, tutti i territori) e comando `MonitorArea {zones}`. Dopo approvazione EXCON parte dalla base, vola in linea d'aria e, in posizione, ogni `update_interval_s` (300 s simulati) aggiunge alla mappa del possessore un'osservazione fresca delle zone coperte (fonte «Drone», senza messaggi; due soli messaggi, arrivo e rientro). Autonomia 2400 s, ricarica 900 s, massimo 4 zone, limite di vento facoltativo; poi il drone torna disponibile da solo. Mappa del giocatore: zone in diretta tratteggiate, drone e banner «drone in diretta»; vista 3D: quadricottero che orbita sulle zone con cono di scansione. Parametri e dettagli in `docs/scenari.md`. Non coperto: copertura circolare (punto + raggio), più droni in coda.
- Etichette e legenda della mappa 2D (`crates/frontend/src/map.rs`, `label_layout.rs`, `map_legend.rs`): nomi di edifici, luoghi, chiusure, risorse e codici di zona sono posizionati in pixel dello schermo dopo il disegno del mondo, con font compatto di 12–14,5 px CSS (cresce lentamente con lo zoom della mappa) su fondo bianco traslucido; solo i nomi critici hanno un bordo sottile. I codici di zona sono descritti sopra (assi o angolo della zona, senza riquadro). In vista d'insieme i nomi lunghi sono abbreviati («Presidio sanitari…», completi da zoom 3) e i luoghi descrittivi di più di due parole compaiono solo da zoom 1,4. Le etichette continuano a evitare il fronte attivo. Il posizionamento (`label_layout::place`, puro e con test) segue la priorità: 0 edifici critici (scuola, ospedale, distributore, municipio) e colpiti da minaccia/danno/evacuazione, incendio segnalato; 1 altri edifici con nome, chiusure, risorse; 2 luoghi; 3 codici di zona. Ogni etichetta prova una corona di posizioni attorno al punto; se si sposta di oltre 10 px è unita al punto da una linea guida; se non c'è spazio le etichette a bassa priorità spariscono, i codici di zona non entrano nel posizionamento ma ne sono ostacoli. Nome di luogo ed edificio uguali e vicini valgono un’etichetta. I segnaposto (edifici con stato, ancore) non vengono coperti. La legenda (`map_legend.rs`, stringhe it/en proprie, altre lingue in inglese) non si affida al solo colore: fronte attivo = puntini, bruciato = tratteggio diagonale, acqua/ritardante = righe orizzontali, linee tratteggiate/continue per previsione, linea di contenimento, strada tagliata e chiusura polizia, simboli per stato degli edifici (▲ minacciato, ◆ danneggiato, ✕ distrutto, ➜ in evacuazione, ○ evacuato) disegnati sulla mappa con lo stesso glifo. `GET` della statica per ruolo include ora `kind` degli edifici.
- Ogni mappa 2D (EXCON Situazione e Controllo, quadro tattico e finestra d'azione del giocatore, previsione) è una finestra rettangolare larga il 100% del suo contenitore, con altezza propria (lo spazio libero della scheda nel quadro tattico, l'altezza dello schermo meno l'intestazione in Situazione, Controllo e finestra d'azione). Il canvas segue le dimensioni della finestra (nitido a ogni densità di pixel); il territorio mantiene le proporzioni, a zoom 1 è centrato e intero, e ingrandendo lo spazio in più mostra più terreno; pan e zoom valgono su tutto il rettangolo (`map.rs`, `Frame`). In Situazione la mappa occupa la colonna larga accanto a «Impatto per luogo»; i suggerimenti di inject vanno sotto. Sotto 1050 px la vista tattica scorre come una pagina.
- Azioni del giocatore («Aggiungi un’azione», modulo `crates/frontend/src/action_pick.rs` sopra la logica pura di `place_pick.rs`; lo stesso modulo serve la revisione EXCON): il menu contiene solo le azioni abilitate per il ruolo nello scenario (sezione Risorse/ruoli dell'editor; il server rifiuta comunque ogni azione non abilitata, `sim::World::validate`). Un'azione non in elenco si descrive nella narrazione: l'EXCON ne valuta l'esito senza effetti automatici. Lo strumento di selezione dipende dall'azione: *punto* (autobotte, lancio d'acqua), *due punti* in ordine o un bordo di zona `F4:N` (linea tagliafuoco), *più zone* (avviso, evacuazione di zona, ricognizione, drone) cliccando le celle (di nuovo per toglierle), disegnando un rettangolo con due angoli o aggiungendo un luogo con nome che diventa le celle che copre (raggio `radius_m`, edifici del luogo, cella del centro); *edificio* o *strada* singoli. Avviso, evacuazione e ricognizione su più zone diventano un ordine per zona; il drone riceve tutte le zone in un solo ordine.
- Ricognizione a terra (`Reconnoitre {zone}`): una squadra (`ground_crews`) parte dalla base, raggiunge la zona (percorso stradale/a piedi con il modello operativo) e all'arrivo riferisce una sola volta cosa vede: la mappa del ruolo per quella zona si aggiorna a quell'ora (fonte «Squadra di ricognizione», messaggio «Ricognizione completata»). La squadra resta impiegata finché l'EXCON non ne conferma il rientro. L'EXCON può ordinarla direttamente indicando `role` (effetto immediato). Abilitata per il `fire_commander` dei territori forniti e per `direzione_soccorso` del pack Selvariva.
- Mappa interattiva (EXCON e giocatori): zoom/pan (rotellina, trascinamento, pinch, tastiera), ispezione hover/tocco, livelli, inquadra incendio, schermo intero, barra di scala. Per i giocatori l’ispezione mostra solo le celle riferite (fog of war): altrove «non riferito». Pagina di prova: `/maptest.html` (dati sintetici, selettori funzioni e zone abilitate).
- Controllo dei livelli della mappa 2D (`crates/frontend/src/map_layers.rs`): il pulsante ☰ nella colonna di strumenti apre un piccolo riquadro a caselle raggruppate. *Sfondo*: sfondo vegetazione/combustibile (spento = fondo chiaro neutro, così strade, edifici, luoghi, fuoco, zone e squadre risaltano), griglia, codici di zona (assi sui bordi e codici nelle celle), nebbia. *Territorio*: strade, edifici, etichette. *Incendio*: fronte e area bruciata, alone di incertezza dell'ensemble, segnalazioni e stime, previsione. *Operazioni*: squadre e droni, linee tagliafuoco/lanci/acqua, chiusure stradali e zone allertate. «Mostra tutto» ripristina; il pulsante è evidenziato se qualche livello è spento. La scelta è solo di presentazione, resta nel local storage del browser per contesto di mappa (`ttx.map_layers.<id>`: le due mappe del giocatore, quadro tattico e Rispondi, la condividono; le mappe EXCON hanno ciascuna la propria) e senza storage vale per la pagina. Con i livelli spenti il clic restituisce comunque la zona (edifici e strade nascosti non vengono selezionati). Testi it/en nel catalogo, fr/de/es nel modulo, altre lingue in inglese.

- Console EXCON unificata (“Controllo”): mappa interattiva più un elenco ricercabile di 30 strumenti, ciascuno con il proprio tooltip, che si aprono in modali dedicati (con mappa per la scelta dei punti). Gli strumenti sulla mappa mettono il comando in coda per il prossimo avanzamento o per un'ora scelta (vedi «Azioni EXCON in coda»; nessuna anteprima; “Correggi ultimo intervento” annulla l’ultimo applicato). Il meteo si riduce a vento e umidità del combustibile. Informazioni ai ruoli, previsione e missioni restano raggiungibili dall’elenco. Le informazioni ai ruoli si rivelano per zona e per livello (incendio, edifici, strade, linee e lanci, squadre) con `POST /api/master/knowledge {role, source, zones, layers}`; il giocatore vede il resto velato (fog of war, `known_zones`). `GET /api/master` espone `role_knowledge`. Gli occupanti degli edifici restano sempre nascosti. Gli inject del MSEL possono rivelare mappa con azioni `reveal` (stima approssimata dell'incendio con errore deterministico, oppure zone/livelli esatti, anche per regola «in fiamme + adiacenti»; vedi `docs/scenari.md`); il banner della mappa del giocatore mostra ora e fonte dell'ultimo dato e la sua età in minuti simulati. A fine turno l'EXCON può aggiornare a mano le mappe dei ruoli (`POST /api/master/knowledge/refresh`). Aperto: un inject inviato in più riprese esegue le rivelazioni solo per i ruoli del primo invio.

- MSEL unificato (redesign EXCON): `InjectTemplate.event` (default vuoto) raggruppa gli inject. `GET /api/master` e `/api/state` espongono `planned_injects` (cronologico: `id_inject`, `title`, `event`, `scenario_time`, `scenario_time_s`, `recipients_planned`, `sent_to`, `not_yet_sent_to`); `GET /api/master` espone anche `inject_responses` (per consegna e ruolo: `replies`, `pending_proposals`). `POST /api/master/msel/<id>/send` accetta `{"recipients":[ruoli]}` (default: destinatari pianificati); i ruoli che hanno già ricevuto l’inject sono saltati in silenzio.
- Azioni pianificate (`Scenario.timeline`, `exercise::actions`): world changes sulla timeline MSEL, indipendenti dagli inject. A ogni avanzamento del tempo EXCON decide Esegui/Salta/Rinvia per quelle che cadono nella finestra; la durata del turno è una proprietà dello scenario (`turn_minutes`) e la scheda Timeline dell'editor mostra inject e azioni su un'unica asse oraria con fasce di turno e trascinamento; vedi `docs/scenari.md`.
- Proposte: `POST /api/master/proposals/<id>/clarify {note}` porta una proposta del player in `needs_clarification` e consegna la domanda nello stesso thread; `POST /api/master/proposals/<id>/revalidate {commands?}` ricalcola descrizioni e verdetti senza decidere; la risposta del player a un chiarimento (`POST /api/player/<role>/reply` con `reply_to` = messaggio di domanda) e un messaggio `reply` nello stesso thread: solo traduzione in inglese, nessuna interpretazione AI, nessuna nuova proposta e nessuna sostituzione; la proposta torna `pending` con i comandi originali (per cambiare azione il player ne crea una nuova; `/actions` con `reply_to` = domanda aperta è rifiutato con errore esplicito). `superseded`/`supersedes` restano nei tipi ma non vengono piu prodotti. Il flag `edited` della decisione è vero solo se i comandi cambiano davvero.
- Thread di chiarimento: la conversazione testuale EXCON-player su una dichiarazione e una vista derivata (`exercise::threads`), non una copia dei dati. Mette in fila la proposta (e l'eventuale catena storica `supersedes`/`superseded_by`), i messaggi `clarification` di EXCON e le risposte del player; l'id del thread e quello della proposta d'origine. Stato: `awaiting_player` (ultima voce di EXCON), `awaiting_excon`, `closed` (chiuso da EXCON con `POST /api/master/clarifications/<id>/close`, oppure proposta decisa). Chiudere ritira una domanda senza risposta (la proposta torna `pending`); un nuovo `clarify` riapre. EXCON fa domande di follow-up con lo stesso `clarify` sulla proposta corrente (`current_proposal`); il player risponde in testo, piu volte, con `POST /api/player/<role>/reply` con `reply_to` = ultimo messaggio di EXCON (si puo rispondere a un chiarimento anche senza inject). Esposto come `clarification_threads` in `/api/master` (con testo inglese), `/api/player/<role>` (senza `text_en`) e nel debrief; ogni riga di `inject_responses` ha `clarification_threads` e i messaggi del suo `thread` portano `clarification_thread`, cosi la scheda Risposte mostra le stesse voci con stato e pulsante "Chiudi chiarimento". Lato player la chat compare nella timeline tattica (`crates/frontend/src/clarify.rs`). I thread non modificano il mondo: solo `World::apply` da proposte approvate.
- Rendering dei dati: `render.rs` mostra comandi, meteo, esecuzioni, eventi e dati generici come elenchi leggibili (il JSON grezzo resta in un blocco richiudibile “dati grezzi” solo dove serve al debug); `event_label`/`role_label` danno nomi leggibili a eventi e ruoli.
- LLM: `chat()` ritenta fino a 2 volte su errori di rete, corpo illeggibile o troncato, 429/5xx e risposta 2xx senza contenuto (mai sui 4xx).
- Notifiche della console EXCON (`crates/frontend/src/notifications.rs`): la campanella è lo storico cronologico (più recente in alto, «da leggere» evidenziato, pallino arancione se ci sono voci che richiedono azione); i toast compaiono in basso a destra, fuori dai controlli del turno. Gli eventi sono mappati in modo puro da `log_spec` (titolo specifico: «Turno 1 · avanzamento a 13:05», «Inject inviato a …», «Azione pianificata eseguita: …»). Priorità *Informativo* (toast che scade dopo ~6 s, oppure solo campanella per avanzamenti, inizio risoluzione, inject inviati, risorse) e *Richiede azione* (proposta in attesa da un ruolo, azione pianificata o consegna non riuscita, bozza da rivedere: toast arancione che resta finché la proposta non viene decisa; non si segna letto aprendo la vista). Le voci simili ravvicinate (avanzamenti dello stesso turno, risorse, missione) si fondono finché non lette; al massimo 3 toast, gli informativi cedono per primi. Non notificano: comandi propri dell'EXCON, parsing delle proposte, cambi di stato interni, chat tra altri ruoli. I player ricevono un solo avviso per cambio di fase del turno, esiti delle proprie proposte, chiarimenti (azione), messaggi, riepiloghi e missioni.

- Validazione luogo/tipo di azione (playtest 2026-10-27, problema #3): un luogo di tipo diverso da quello atteso non fa più perdere il turno. Nel modulo del giocatore (`tactical.rs`) cambiare tipo di azione svuota il luogo se ne cambia la natura (zona, edificio, strada, punto); per «Evacuare una zona» e «Allertare una zona» il campo ha l’elenco dei codici griglia e dei luoghi, e scrivere il nome di un edificio passa da solo a «Evacuazione di edificio» con un avviso. Lato modello, `sim::World::reconcile_command` (chiamato da `Exercise::set_commands`, quindi per form e interpretazione AI) adatta i casi non ambigui: zona = nome di edificio, EvacuateBuilding; zona = luogo con nome («Pineta»), zona in cui cade; allerta su un edificio, zona dell’edificio; EvacuateBuilding con un codice zona, EvacuateZone. L’adattamento compare come nota del verdetto. Se nulla corrisponde, l’errore dice che cosa è stato riconosciuto e propone i nomi più vicini (`unknown zone "X": a zone is a grid code like A1 (did you mean «…»?)`; `"X" is a building, not a zone: use Evacuate building…`). Il catalogo per l’LLM (`sim::command::catalogue`) dice che `zone` è sempre un codice griglia e che una struttura singola usa `EvacuateBuilding`. Le stringhe nuove sono in `i18n.rs` (it/en) e `i18n_place.rs` (altre lingue).
