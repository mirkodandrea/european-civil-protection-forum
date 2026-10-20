---
name: public-game-release
description: Preparare una distribuzione TTX riproducibile e verificare web, desktop e uso offline.
---

# Distribuzione e prontezza TTX

La destinazione è una sessione di esercitazione. Verificare il percorso di build del
repo; non aggiungere hosting pubblico, QR, classifica o moduli di feedback per default.

- Registrare commit e stato modificato, versioni, scenario effettivo e configurazione.
- Preparare un bundle con asset e dati necessari; testare percorsi relativi e symlink
  sulle piattaforme supportate. Non includere chiavi, cassette private o dati personali.
- Verificare avvio, connessione partecipanti/EXCON, ruoli, proposta/approvazione,
  avanzamento, previsione, rollback, salvataggio e debrief.
- Controllare che endpoint e schermate non espongano informazioni EXCON ai partecipanti.
- Distinguere offline del renderer da offline dei servizi LLM: dichiarare cosa richiede
  rete e quale fallback esiste. Una cache del browser non basta come prova offline.
- Usare viewport reali della sala e macchina di destinazione; telefono solo se supportato.
- Misurare tempi e memoria nella build release prima di modificare profili o shader.
- Verificare i job CI pertinenti; la sola compilazione locale non convalida altri OS.

Preparare risultati concreti e reviewabili prima di chiedere approvazione per una
pubblicazione non autorizzata. Non creare tag, push, inviare messaggi o pubblicare
servizi se l'utente non l'ha richiesto. Se la richiesta già autorizza la pubblicazione,
non introdurre un checkpoint aggiuntivo derivato dal gioco.

## Provenienza e adattamento

Adattato il 2026-10-20 dalla skill `public-game-release` del progetto `propagator_abm`
(`/Users/mirko/dev/experiments/propagator_abm/.claude/skills/public-game-release/SKILL.md`).
SHA-256 sorgente: `53e440b3f72d5aa33c993f11a09e0b5b3f1aae8d9fc494f60bee537b64f7e78d`. Il documento sorgente non dichiara un autore
individuale: si conserva l'attribuzione al progetto e alla sua storia git; non si
attribuisce la paternità a questo adattamento. Riferimento alla licenza EUPL-1.2
dell'integrazione esistente, da conservare per eventuale codice trasferito.

Le regole TTX sopra sono un adattamento, non una copia normativa del gioco:
prevalgono istruzioni dell'utente e del repository. Non si trasferiscono punteggi,
ricarica implicita, restrizioni artificiali alle evacuazioni o checkpoint umani
non richiesti. Verificare API e comandi nel repo prima di usarli.
