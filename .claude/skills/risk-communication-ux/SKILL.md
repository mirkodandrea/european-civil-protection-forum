---
name: risk-communication-ux
description: Rendere leggibili stato, decisioni e incertezza nelle mappe e schermate operative TTX.
---

# Comunicazione del rischio nel TTX

Il partecipante deve capire cosa è osservato, cosa è previsto, dove serve una decisione
e quale autorità la prende. Evitare estetica ludica e decorazioni che nascondano lo stato.

- Gerarchia: persone/esposizione e missioni, accessi e strutture, poi contesto geografico.
- Palette stabile con simboli/testo oltre al colore. Pericolo, evacuazione, risorse e
  stato ordine devono avere significati coerenti tra mappa, badge e log.
- Mostrare proposta, approvazione, viaggio, lavoro, rifornimento, attesa, ritirata e
  completamento con tempi e motivi del core. Non chiamare "completata" una partenza.
- Anteprima percorso/ETA e motivo di indisponibilità senza effetti sul mondo.
- Etichette leggibili, collisioni risolte o ridotte per priorità, chiusure di tratto
  visibili; indicare quale strada e quale accesso sono ancora utilizzabili.
- Separare fuoco corrente, probabilità ensemble corrente e previsione futura.
  Ogni previsione mostra emissione, orizzonte, ipotesi, incertezza e fonte EXCON.
- Simboli e pannelli rispettano la visibilità del ruolo: non rivelare un futuro inject.
- Messaggi brevi con ora simulata, mittente e azione richiesta; niente chat decorativa.
- Scorciatoie rispettano il focus dei campi. Esc annulla anteprime; un touch cancellato
  non deve diventare un comando. Non imporre autopausa o ripresa del gioco al TTX.

Debrief con persone dentro/in transito/arrivate, effetti e cronologia, senza stelle o
classifica. Annotare limiti e incertezza dei confronti. Verificare immagini a più scale,
stati affollati e vista obliqua, distinguendo prove tecniche e usabilità con operatori.

## Provenienza e adattamento

Adattato il 2026-10-20 dalla skill `risk-communication-ux` del progetto `propagator_abm`
(`/Users/mirko/dev/experiments/propagator_abm/.claude/skills/risk-communication-ux/SKILL.md`).
SHA-256 sorgente: `320034892226bde8792146bd10b71cbe287e571a14eb6bca373a14623daa8a02`. Il documento sorgente non dichiara un autore
individuale: si conserva l'attribuzione al progetto e alla sua storia git; non si
attribuisce la paternità a questo adattamento. Riferimento alla licenza EUPL-1.2
dell'integrazione esistente, da conservare per eventuale codice trasferito.

Le regole TTX sopra sono un adattamento, non una copia normativa del gioco:
prevalgono istruzioni dell'utente e del repository. Non si trasferiscono punteggi,
ricarica implicita, restrizioni artificiali alle evacuazioni o checkpoint umani
non richiesti. Verificare API e comandi nel repo prima di usarli.
