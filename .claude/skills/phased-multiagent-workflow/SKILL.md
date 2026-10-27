---
name: phased-multiagent-workflow
description: Gestire lavoro TTX per fasi verificabili e coordinare eventuali contributi autorizzati.
---

# Lavoro verificabile e coordinamento

Prima leggere specifiche, stato git e istruzioni locali. Documenti descrivono il
comportamento atteso; il codice mostra quello effettivo. Segnalare e correggere le
divergenze senza trasformare documentazione vecchia in vincolo operativo implicito.

1. Tenere traccia di tutte le richieste e dei criteri di accettazione.
2. Implementare per gruppi verificabili; aggiornare documentazione nello stesso lavoro.
3. Provare comportamenti con test e laboratorio, grafica con immagini osservate.
4. Riportare risultati, comandi riproducibili, limiti e attività realmente rimaste.

Non fermarsi a checkpoint umani non richiesti: l'autorizzazione dell'utente a
implementare comprende le fasi necessarie. Chiedere una decisione solo se serve
veramente o se richiesta esplicitamente. Nessun commit/push/deploy implicito.

Con contributi concorrenti: leggere `git status` e diff, concordare proprietà dei file,
non cancellare modifiche altrui e verificare il risultato integrato. Non mettere in
stage file misti indiscriminatamente. Nessun isolamento o modello hardcoded.
Delegare o creare sotto-agenti solo se l'utente o istruzioni applicabili lo autorizzano;
questa skill non è un'autorizzazione automatica a parallelizzare qualunque lavoro.

In italiano scrivere aggiornamenti brevi con evidenza e limiti concreti. Prima di
compattare conservare stato utile nei documenti pertinenti senza dati sensibili.

## Provenienza e adattamento

Adattato il 2026-10-20 dalla skill `phased-multiagent-workflow` del progetto `propagator_abm`
(`/Users/mirko/dev/experiments/propagator_abm/.claude/skills/phased-multiagent-workflow/SKILL.md`).
SHA-256 sorgente: `a04215ec234004677531cf9f938426fd173c1df5c3fe8f6ee9723cc8f908e468`. Il documento sorgente non dichiara un autore
individuale: si conserva l'attribuzione al progetto e alla sua storia git; non si
attribuisce la paternità a questo adattamento. Riferimento alla licenza EUPL-1.2
dell'integrazione esistente, da conservare per eventuale codice trasferito.

Le regole TTX sopra sono un adattamento, non una copia normativa del gioco:
prevalgono istruzioni dell'utente e del repository. Non si trasferiscono punteggi,
ricarica implicita, restrizioni artificiali alle evacuazioni o checkpoint umani
non richiesti. Verificare API e comandi nel repo prima di usarli.
