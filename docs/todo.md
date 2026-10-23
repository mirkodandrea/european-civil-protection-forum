# Temi da definire

Il progetto è in fase di definizione: questi punti non hanno priorità né
scadenze. Per le capacità già presenti, vedere la [specifica di alto livello](stato-attuale.md).

- Discutere quali ulteriori azioni operative il simulatore dovrebbe modellare.
- Aggiungere hint testuali ai partecipanti sulle azioni possibili nel loro ruolo
  e su come chiedere supporto.
- Modellare più viaggi per autobus nella stessa evacuazione, con riuso dei mezzi
  e tempi di completamento.

## Problemi emersi dal playthrough del 2026-10-23

- ~~Blocco della scheda con `CutFireLine`~~ (risolto). Con lo scenario factory e il
  modello operativo, `Network::route` (percorso a piedi, raggio 1,5 km) eseguiva un
  Dijkstra per ogni ancora di partenza e ricostruiva ogni coppia di percorsi: ~67 s
  in nativo, mai terminato in WASM. Ora usa un solo Dijkstra multi-sorgente
  (`crates/sim/src/network.rs`); test `factory_operational_fire_line_validation_is_fast`.
  Nota: `trunk serve` non ricostruisce quando cambia solo `crates/sim`.
- ~~Lingua dell'interfaccia~~ (fatto per i player, it/en; altre lingue ricadono su en).
  Resta da localizzare: lato EXCON, schermata di accesso, testi di server/LLM
  (stato e dettaglio missioni, errori), nomi di ruoli e risorse, e le fasi
  (`orders`, `resolving`, …) mostrate in inglese grezzo anche in italiano.
- ~~Focus sul titolo del modale inject~~, ~~ordine dei campi del modale~~,
  ~~bozza AI in inglese~~, ~~concatenazione richiesta/chiarimento~~ e
  ~~motivo del rifiuto visibile al player~~ (risolti).
- "AI non disponibile" dopo un chiarimento: ora il motivo è salvato (`failure`) e
  mostrato; la causa probabile (azione costruita dal testo libero, non valida come
  `Command`) è da confermare con `TTX_LIVE_LLM=1`. Possibile seguito: un nuovo
  tentativo automatico che rimandi all'AI l'errore di validazione.

## Esito QA multi-agente (2026-10-23)

Verificato nel browser: avvio, inject, proposta, approvazione, avanzamento turno e
clock live, rollback, chat tra player, pubblicazione perimetro. Nota di metodo:
alcune tab degli agenti avevano un bundle vecchio (hash diverso da quello servito)
e hanno mostrato blocchi già risolti; controllare sempre
`curl -s http://127.0.0.1:8080/ | grep -o 'frontend-[0-9a-f]*_bg.wasm'`.

Da sistemare:
- Riepilogo proposta grezzo e in inglese ("Player submits X for F4."), senza
  risorsa, quantità né inject di origine; duplicato nella scheda EXCON (il fix attuale
  nasconde il riepilogo solo se uguale alla richiesta).
- Il campo "Zona" modificato nella scheda EXCON non resta (torna al valore originale
  e l'approvazione viene rifiutata come infattibile).
- Proposta con linea non raggiungibile ("no reachable route from staging to task"):
  "Approva azione" resta attivo; il messaggio è in inglese e può restare stantio dopo
  una modifica.
- Il form del player non si svuota dopo l'invio e non c'è protezione dal doppio invio
  (due proposte identiche); la zona predefinita `A1` induce errori.
- Fire Commander: nessun "Rispondi →" sull'inject in alcune viste (solo il menu a tendina).
- Rollback: la timeline EXCON del turno 1 mostra ancora l'approvazione annullata, il
  selettore Turno offre ancora il turno 2 dopo il ritorno al turno 1, gli inject e le
  vecchie proposte del player restano; nessuna conferma né feedback.
- Cronologia: manca il messaggio di chat tra player (ids mancanti); etichette in
  inglese grezzo; nessun filtro per autore/tipo; l'evento "map information acquired"
  pesa ~3 MB nel DOM se espanso.
- Mappa del player: dopo "Pubblica perimetro" manca "Rapporto EXCON" con la fonte.
- Inject privato ripetuto nei thread di chat player-player; "Map information"
  assente da Conversazioni.
- "Operazione completata" stantio in Conversazioni e nel banner di Strumenti;
  nessuna conferma visibile dopo la pubblicazione sulla Mappa.
- Fire Commander: dopo un lancio d'acqua gli "Air tankers" risultano 0 disponibili
  nonostante "replenished"; una stringa grezza `air_tankers · completed · …` rompe il
  layout della colonna Ground crews.
- Messaggio "Turn resolved" in inglese e generico per tutti i ruoli.
- Il cursore velocità non parte dal massimo (CLAUDE.md: default `speed` 0 = massimo).
- Il modello OpenRouter nei campi è `openai/gpt-oss-120b`, CLAUDE.md dice
  `deepseek/deepseek-v4.1-flash`.
- Strumenti: pulsanti sopra la mappa e anteprima molto più in basso, "Correggi
  ultimo intervento" mostrato senza interventi, clock duplicato rispetto alla Timeline,
  pagina molto lunga; "Proposte da revisionare" è un titolo vuoto.
- Regional Civil Protection: "Risorse e missioni" è un titolo vuoto, senza segnaposto.
- Layout EXCON: colonne fisse da ~265 px con spazio vuoto a destra; il pannello 3D
  occupa ~30% della larghezza su ogni tab. Responsive non verificato.
- Con "Simulazione" attiva e più tab aperte il carico CPU/memoria è alto (ogni tab
  esegue la propria simulazione e vista 3D).

## Playthrough da completare

Ancora da verificare (`trunk serve` va forzato al rebuild se cambia solo `crates/sim`):

- Missioni in corso e arrivo delle squadre dopo l'approvazione di una linea
  tagliafuoco, con risorse "impegnate" nella vista del player.
- Percorso con testo libero all'AI e modalità "Tutti i player"; persistenza dopo reload.
- Aspetto responsive a finestra stretta.
