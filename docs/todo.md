# Temi da definire

Il progetto è in fase di definizione: questi punti non hanno priorità né
scadenze. Per le capacità già presenti, vedere la [specifica di alto livello](stato-attuale.md).

- Discutere quali ulteriori azioni operative il simulatore dovrebbe modellare.
- ~~Aggiungere hint testuali ai partecipanti~~: la guida alla risposta accanto al
  testo (cosa, dove, con quali mezzi, entro quando, chi si informa) e competenze e
  risorse del ruolo (2026-10-29).
- Solo l’EXCON agisce (2026-10-29), aperti: piani, leggi e procedure per ruolo
  non esistono ancora nello scenario (la guida mostra competenze e risorse);
  provare `review_reply` con `TTX_LIVE_LLM=1` sul caso del DOS/Sindaco; il banco
  (`?bench=1`) e alcuni script QA descrivono ancora il vecchio flusso con azioni
  del player; l’EXCON non riceve ancora un avviso «riletta» quando l’AI finisce;
  domande aperte non entrano ancora nella finestra di revisione prima
  dell’avanzamento (solo nel chip del pannello del turno).
- Modellare più viaggi per autobus nella stessa evacuazione, con riuso dei mezzi
  e tempi di completamento.

## Spegnimento senza togliere vegetazione (decisioni aperte)

`ExtinguishArea` (area disegnata dall'EXCON) spegne il fuoco senza toccare la
vegetazione. Restano da decidere:

- **Velocità di spegnimento** (domanda di Sabrina): ora è istantaneo
  all'applicazione. Una velocità impostata dall'EXCON (es. ettari/ora, o un
  fronte che avanza nel poligono) richiede un compito progressivo in `sim`
  integrato sul tempo simulato.
- Gli altri percorsi che cambiano ancora la vegetazione (combustibile →
  non vegetato): `Extinguish` su zona/cerchio (`FireSim::extinguish_cells`,
  anche sulle celle in fiamme), `CutFireLine` istantanea e progressiva
  (`FireSim::fire_line`). Da decidere se `Extinguish` debba usare la nuova
  regola; la linea tagliafuoco per definizione toglie combustibile.
- Aggiungere lo strumento anche ai comandi allegati alle narrazioni e alle
  azioni degli inject (`staged_commands.rs`, `action_form.rs`), oggi esclusi.

## Azioni EXCON in coda (2026-10-29): da decidere

- «Esegui ora ordine approvato» (`POST /api/master/proposals/<id>/execute`) applica
  ancora subito una proposta approvata a turno aperto: tenerlo come eccezione o
  toglierlo dall'interfaccia (le proposte approvate si applicano già all'avanzamento).
- Le azioni a offset 0 degli inject restano immediate all'invio; negli scenari, gli
  inneschi andrebbero spostati in azioni EXCON programmate (o azioni anticipate)
  separate dall'inject di segnalazione.
- Una modifica a un'azione in coda di un turno precedente non è permessa (si annulla
  e si rimette): valutare se serve davvero modificarla.

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
- ~~"AI non disponibile" dopo un chiarimento~~: la bozza privata AI è stata
  sostituita dai suggerimenti automatici sulla narrazione (`suggest_actions`).
  Da provare con `TTX_LIVE_LLM=1` la qualità dei suggerimenti nei due ambiti
  (player / EXCON) e valutare se ripetere la richiesta quando cambiano le azioni
  già aggiunte.

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
- ~~Proposta con verdetto negativo bloccata anche dopo la modifica~~ (risolto: la
  fattibilità si ricalcola in automatico dopo ogni modifica, `/revalidate`). Il
  campo "Zona" modificato non resta: da riverificare. Il testo dei verdetti del
  simulatore ("no reachable route from staging to task", ecc.) è ancora in inglese
  tranne le note già coperte da `localize_note`.
- ~~Il form del player non si svuota dopo l'invio~~ (risolto: luogo, secondo punto,
  quantità e "in risposta a" vengono azzerati; il pulsante era già disabilitato
  durante l'invio). Da verificare: la zona predefinita `A1` (nel codice il campo
  parte vuoto, forse arriva da un clic sulla mappa).
- Fire Commander: nessun "Rispondi →" sull'inject in alcune viste (solo il menu a tendina).
- Rollback: la timeline EXCON del turno 1 mostra ancora l'approvazione annullata, il
  selettore Turno offre ancora il turno 2 dopo il ritorno al turno 1, gli inject e le
  vecchie proposte del player restano; nessuna conferma né feedback.
- Cronologia: manca il messaggio di chat tra player (ids mancanti); le etichette
  degli eventi sono ora leggibili (`event_label`), restano quelle non mappate; nessun filtro per autore/tipo; l'evento "map information acquired"
  pesa ~3 MB nel DOM se espanso.
- Mappa del player: dopo "Pubblica perimetro" manca "Rapporto EXCON" con la fonte.
- ~~Inject privato ripetuto nei thread di chat player-player~~ (risolto):
  Conversazioni contiene solo messaggi tra player. Gli inject e le informazioni
  EXCON restano nelle viste dedicate e nelle notifiche del ruolo.
- "Operazione completata" stantio in Conversazioni e nel banner di Controllo;
  nessuna conferma visibile dopo la pubblicazione sulla Mappa.
- Fire Commander: dopo un lancio d'acqua gli "Air tankers" risultano 0 disponibili
  nonostante "replenished"; una stringa grezza `air_tankers · completed · …` rompe il
  layout della colonna Ground crews.
- Messaggio "Turn resolved" in inglese e generico per tutti i ruoli.
- Il cursore velocità non parte dal massimo (CLAUDE.md: default `speed` 0 = massimo).
- Il modello OpenRouter nello standalone è `openai/gpt-oss-120b` per scelta (per ora);
  CLAUDE.md indica `deepseek/deepseek-v4.1-flash` per la versione server.
- ~~Strumenti: pulsanti sopra la mappa e anteprima in basso, pagina lunga~~ (superato
  dalla tab Controllo con elenco ricercabile e modali). Da verificare in Controllo:
  "Correggi ultimo intervento" mostrato senza interventi; clock duplicato rispetto
  alla Timeline.
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

## Redesign console EXCON (2026-10-23): lasciato indietro

Fatto: timeline cronologica con pill e invio per destinatari, tab Risposte,
Controllo (elenco ricercabile + modali), rendering leggibile dei dati,
chiarimento reale, ricontrollo automatico, `superseded`, retry LLM. Da fare:

- Controllo: nessuno strumento per la temperatura (`SetWeather` non ha il campo);
  non tutti i percorsi di “Applica” dei 29 strumenti sono stati provati fino in
  fondo; gli strumenti sulla mappa non hanno più l'anteprima. Il difetto originale
  della linea tagliafuoco non è stato riprodotto: la causa più probabile era
  "Applica" disabilitato finché il clock non era fermo e senza motivo scritto.
- Editor degli inject (impostazioni): manca il campo `event` per raggrupparli.
- Risposte: non provato nel browser un caso reale di proposta `superseded`
  (coperto solo da test unitari); il rifiuto dal modale non è stato provato fino
  in fondo.
- Risposte per inject (playtest 28/10): le stringhe nuove (`i18n_risposte.rs`)
  esistono solo in italiano e inglese; le altre lingue ripiegano sull’inglese. Il
  P0 «azione narrata non registrata» non si riproduce sul percorso principale
  (Rispondi dal dettaglio inject); il percorso ambiguo («+ Nuova azione» con
  inject scelto automaticamente) è stato tolto.
- Standalone: dopo «Rimuovi chiave» e ricaricamento, la sessione ripresa può
  ancora indicare «AI configurata da .env» (l’AI risulta disponibile pur senza
  chiave salvata né chiave di build); `scripts/qa/standalone-settings.cjs`
  verifica solo che la chiave salvata sia sparita. Da chiarire.
- Ricontrollo: solo automatico (nessun pulsante "Ricontrolla"); ogni ricontrollo
  scrive un evento nel log; il test sul caso "raggio del tubo oltre 60 m" richiede
  il modello operativo (il test usa un'evacuazione con pochi autobus).
- Lingua: con “Tutti i player” più pannelli con lingue diverse condividono la
  lingua globale (l'UI è una sola istanza di `set_ui_lang`).
- Nota EXCON per proposta: non sopravvive a un reload della pagina.
- LLM: nel build wasm il retry è immediato (nessun timer); il troncamento del
  corpo (`ERR_ABORTED`) non è stato riprodotto in wasm, il retry è testato solo
  in nativo.
- `localize_note` copre solo alcune note del simulatore (squadre, edifici,
  persone, autobus, "to X", "arrive in"); le altre restano in inglese.
