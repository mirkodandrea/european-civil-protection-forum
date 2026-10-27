---
name: agent-playtest
description: Verificare i flussi TTX con personas operative e osservazioni riproducibili.
---

# Verifica dei flussi TTX

Separare la prova di usabilità dalla verifica degli algoritmi del laboratorio.
Il partecipante vede solo le informazioni del proprio ruolo; EXCON vede il controllo
esercitazione. Nessuna lettura di stato riservato per aiutare una persona simulata.

1. Definire ruolo, obiettivo e briefing: sindaco, DOS, coordinamento regionale o EXCON.
2. Provare scenario iniziale, proposta, approvazione/rifiuto, avanzamento, messaggi,
   previsione pubblicata, debrief e rollback. Non inventare procedure di comando.
3. Annotare per ogni azione ciò che era visibile, l'aspettativa, l'esito e i punti di blocco.
4. Distinguere un limite dello strumento da un difetto del prodotto.
5. Registrare scenario, seme, commit, viewport e screenshot con tempi simulati.
6. Produrre una tabella problema / gravità / evidenza / correzione; riprovare i difetti risolti.

Per una persona non usare console o stato interno per scegliere gli ordini. Un test
tecnico può usarli, ma va etichettato separatamente. Non avviare sessioni native che
rubano il focus quando bastano test headless o screenshot. Più personas non devono
condividere un browser concorrente. Delegare solo se autorizzato dalle istruzioni
applicabili; non imporre modelli o richiedere sotto-agenti.

Nessuna classifica, voto sul divertimento o raccolta di dati personali per default.
I risultati riguardano comprensione dei tempi, coordinamento, autorità e incertezza.

## Provenienza e adattamento

Adattato il 2026-10-20 dalla skill `agent-playtest` del progetto `propagator_abm`
(`/Users/mirko/dev/experiments/propagator_abm/.claude/skills/agent-playtest/SKILL.md`).
SHA-256 sorgente: `233aee8e271b480d7ba44b5cbad5bad113dc6d88c80d914951019a56bcadbfaf`. Il documento sorgente non dichiara un autore
individuale: si conserva l'attribuzione al progetto e alla sua storia git; non si
attribuisce la paternità a questo adattamento. Riferimento alla licenza EUPL-1.2
dell'integrazione esistente, da conservare per eventuale codice trasferito.

Le regole TTX sopra sono un adattamento, non una copia normativa del gioco:
prevalgono istruzioni dell'utente e del repository. Non si trasferiscono punteggi,
ricarica implicita, restrizioni artificiali alle evacuazioni o checkpoint umani
non richiesti. Verificare API e comandi nel repo prima di usarli.
