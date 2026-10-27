---
name: synthetic-territory
description: Creare e validare territori sintetici e scenari TTX riproducibili.
---

# Territorio sintetico per TTX

Usare `scenario`, `scripts/scenario_factory.py` e i formati esistenti. Il territorio
sintetico deve essere dichiarato tale e sostenere gli obiettivi dell'esercitazione.

- DEM, combustibile e maschere condividono reticolo e coordinate. Origine locale SW,
  +x est e +y nord; raster con riga 0 a nord. Usare le conversioni di `Scenario`.
- Il terreno visuale può essere più fine, ma deriva dal modello e non ne cambia la quota.
- Script riproducibili e idempotenti con seme, richiesta e parametri registrati.
  Non sovrascrivere dati reali o asset sorgente per provare un'ipotesi.
- Strade carrabili con pendenze e curve plausibili, incroci realmente connessi e uscite
  verificabili. Un attraversamento senza nodo comune non è un incrocio nel routing.
- Una strada disegnata non diventa tagliafuoco senza una scelta fisica esplicita.
- Edifici orientati e distanziati sulla dimensione visuale, collegati alla rete.
  Strutture vulnerabili, aree di attesa e rifugi hanno ruolo e capienza distinti.
- Destinazioni sicure e raggiungibili secondo il modello, mai scelte solo per estetica.
- Conservare provenienza/licenza dei dati e distinguere dati reali da trasformazioni.

Cercare scenari mediante innesco patch, meteo, durata e cambi annunciati, osservando
prima gli impatti senza ordini. Provare più semi e un piano fattibile con ritardo;
non ottimizzare danni o territorio soltanto per forzare un esito. Salvare mappe
comparabili e replay; rifare il laboratorio dopo ogni cambiamento del territorio.

## Provenienza e adattamento

Adattato il 2026-10-20 dalla skill `synthetic-territory` del progetto `propagator_abm`
(`/Users/mirko/dev/experiments/propagator_abm/.claude/skills/synthetic-territory/SKILL.md`).
SHA-256 sorgente: `7e4fe332a5c99e1bbbd69a9641c05939a890fce0e40324405d73c1f3a5241572`. Il documento sorgente non dichiara un autore
individuale: si conserva l'attribuzione al progetto e alla sua storia git; non si
attribuisce la paternità a questo adattamento. Riferimento alla licenza EUPL-1.2
dell'integrazione esistente, da conservare per eventuale codice trasferito.

Le regole TTX sopra sono un adattamento, non una copia normativa del gioco:
prevalgono istruzioni dell'utente e del repository. Non si trasferiscono punteggi,
ricarica implicita, restrizioni artificiali alle evacuazioni o checkpoint umani
non richiesti. Verificare API e comandi nel repo prima di usarli.
