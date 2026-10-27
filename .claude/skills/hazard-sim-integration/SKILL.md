---
name: hazard-sim-integration
description: Integrare fisica, agenti e previsioni nel TTX con una sola autorità e replay.
---

# Integrazione modello–TTX

`sim::World` è l'autorità usata da app, server, test e laboratorio. Il rendering e gli
agenti di ruolo non replicano fisica o regole. Il motore incendio resta `fire` /
`propagator-core`; scenari e configurazioni appartengono a `scenario`.

- Tempi, quantità e flussi si integrano in tempo simulato, mai per frame.
- Stessi scenario effettivo, seme, comandi e tempi producono gli stessi esiti.
- Testare invarianza al passo, conservazione persone/risorse e snapshot/restore.
- Anteprime di percorsi o comandi sono pure: nessun consumo o effetto sul mondo.
- Acqua e linee agiscono sulle condizioni fisiche; non premiare vicinanza a una casa.
- Separare esposizione degli edifici, pericolo delle persone e accessibilità stradale.
- Configurare tarature e interruttori nello scenario. Un interruttore spento è inerte,
  compreso T+0; verificare contro il comportamento precedente su più semi.
- Evitare A* per frame/sotto-passo: rivalutare con cadenza esplicita o cambio rete.
- Chiusura di polizia e pericolo per segmento non sono lo stesso stato.

La previsione nasce da un checkpoint corrente con ipotesi esplicite; non legge il
futuro realizzato. Conserva interventi già eseguiti, non simula nuovi ordini nascosti.
Pubblicazione e visibilità passano da EXCON, con generazione/rollback e scadenza del
prodotto. L'alone dell'ensemble corrente non è una previsione a +1/+3 ore.

Controllare provenienza del vento, unità, nodata/NaN e risoluzione. Un innesco patch
riduce estinzioni casuali da singola cella senza garantire attecchimento.
Usare `operational_lab` e conservare manifest, tracce ed eccezioni seme per seme.

## Provenienza e adattamento

Adattato il 2026-10-20 dalla skill `hazard-sim-integration` del progetto `propagator_abm`
(`/Users/mirko/dev/experiments/propagator_abm/.claude/skills/hazard-sim-integration/SKILL.md`).
SHA-256 sorgente: `13d8ed0176133297521b8f2888bd8c57df891364115b6f143002ab46a2c92b7a`. Il documento sorgente non dichiara un autore
individuale: si conserva l'attribuzione al progetto e alla sua storia git; non si
attribuisce la paternità a questo adattamento. Riferimento alla licenza EUPL-1.2
dell'integrazione esistente, da conservare per eventuale codice trasferito.

Le regole TTX sopra sono un adattamento, non una copia normativa del gioco:
prevalgono istruzioni dell'utente e del repository. Non si trasferiscono punteggi,
ricarica implicita, restrizioni artificiali alle evacuazioni o checkpoint umani
non richiesti. Verificare API e comandi nel repo prima di usarli.
