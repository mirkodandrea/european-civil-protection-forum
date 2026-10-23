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
- **Lingua dell'interfaccia.** La UI dovrebbe essere localizzata con la lingua
  selezionata dal player; oggi il selettore cambia solo i contenuti (inject,
  messaggi) mentre etichette, bottoni e stati restano in italiano.
- Il focus automatico sul titolo del modale inject non è affidabile.
- Nel modale di risposta "Secondo punto" compare sotto "Rispondi a inject":
  riordinare i campi.
- Bozza AI parziale ("Evacuate Pineta Primary School") mostrata in inglese in UI
  italiana; dopo un chiarimento l'AI può risultare "non disponibile".
- Richiesta e chiarimento vengono concatenati senza punteggiatura nella bozza
  inviata a EXCON.
- Il motivo di un rifiuto EXCON è visibile al player solo se EXCON scrive una nota.
