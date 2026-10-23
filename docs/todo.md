# Temi da definire

Il progetto è in fase di definizione: questi punti non hanno priorità né
scadenze. Per le capacità già presenti, vedere la [specifica di alto livello](stato-attuale.md).

- Discutere quali ulteriori azioni operative il simulatore dovrebbe modellare.
- Aggiungere hint testuali ai partecipanti sulle azioni possibili nel loro ruolo
  e su come chiedere supporto.
- Modellare più viaggi per autobus nella stessa evacuazione, con riuso dei mezzi
  e tempi di completamento.

## Problemi emersi dal playthrough del 2026-10-23

- **Blocco della scheda (alta priorità).** Nella build standalone, inviare una
  proposta `CutFireLine` come Fire Commander (da/a con zone `F4`→`G4` o con luoghi
  con nome) manda il renderer al 100% di CPU con memoria in crescita (> 4 GB)
  finché la scheda non viene terminata. Un lancio d'acqua indipendente non lo fa.
  Le stesse chiamate in headless (`exercise::player_actions` e gli endpoint HTTP
  `/api/master`, `/api/state`, `/api/player/…`) terminano subito: test
  `named_place_fire_line_player_action_terminates` e
  `structured_fire_line_action_endpoints_terminate`. Il ciclo va quindi cercato
  nel solo percorso WASM/UI (renderer 3D o rendering Yew della proposta).
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
