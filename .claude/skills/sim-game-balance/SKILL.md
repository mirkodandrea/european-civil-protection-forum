---
name: sim-game-balance
description: Calibrare simulazione e agenti TTX mediante esperimenti headless accoppiati.
---

# Laboratorio e calibrazione TTX

Prima di cambiare tempi, risorse, autonomia o territorio, registrare una baseline
riproducibile con `cargo run --release --offline -p sim --bin operational_lab --
SUITE_JSON NEW_OUTPUT_DIRECTORY`. Vedi `docs/operational-lab.md`.

- Una sola fisica: il runner usa `World`, comandi e risorse reali; niente modello clone.
- Confrontare gli stessi scenari, semi e inject con nessun ordine, attacco precoce,
  difesa strutture, evacuazione precoce/tardiva e piano combinato.
- Esporre tarature/interruttori nel JSON di scenario; cambiare un parametro alla volta
  quando si vuole attribuire l'effetto. Salvare lo scenario effettivo nel replay bundle.
- Usare almeno 3–5 semi per risultati esplorativi; mostrare ogni seme oltre alla media.
  Una differenza piccola richiede più repliche e una soglia di rumore dichiarata.
- Misurare persone, danni, ettari, tempi viaggio/lavoro/ricarica/attesa, acqua, ritirate,
  ordini rifiutati e motivi di inattività. Il runner non trasforma questi valori in voti.
- Un comando rifiutato è evidenza: distinguere piano non fattibile da algoritmo scarso.
- Un miglioramento della politica non giustifica automaticamente meno risorse. Le
  capacità sono ipotesi formative esplicite, non leve per rendere il TTX "difficile".
- Un meccanismo spento deve mantenere gli esiti precedenti; verificare snapshot/restore
  e invarianza al passo oltre alle metriche aggregate.

Rieseguire dopo variazioni del territorio. Dichiarare differenze, eccezioni, costi e
limiti del modello; nessuna attribuzione causale agli ordini senza traccia. Non imporre
nuova conferma per modifiche già autorizzate; documentare la configurazione attiva.

## Provenienza e adattamento

Adattato il 2026-10-20 dalla skill `sim-game-balance` del progetto `propagator_abm`
(`/Users/mirko/dev/experiments/propagator_abm/.claude/skills/sim-game-balance/SKILL.md`).
SHA-256 sorgente: `d0851b0408628a4be5f831a52c1b7b3edb7f3dd111c3500ec0bf3a51d6c3f4c8`. Il documento sorgente non dichiara un autore
individuale: si conserva l'attribuzione al progetto e alla sua storia git; non si
attribuisce la paternità a questo adattamento. Riferimento alla licenza EUPL-1.2
dell'integrazione esistente, da conservare per eventuale codice trasferito.

Le regole TTX sopra sono un adattamento, non una copia normativa del gioco:
prevalgono istruzioni dell'utente e del repository. Non si trasferiscono punteggi,
ricarica implicita, restrizioni artificiali alle evacuazioni o checkpoint umani
non richiesti. Verificare API e comandi nel repo prima di usarli.
