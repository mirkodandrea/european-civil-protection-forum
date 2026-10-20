# Skill adattate dal progetto propagator_abm

Il 2026-10-20 sono state lette e adattate tutte le dieci skill del progetto sorgente,
conservando per ciascuna percorso e SHA-256 del `SKILL.md` originale. Si trovano in
`.claude/skills/`, riconoscibili anche da agenti che leggono manualmente quel percorso.
Non vengono installate o eseguite automaticamente da altri runtime.

| Skill | Applicazione nel TTX |
|---|---|
| `agent-playtest` | Verifica comprensione di ruoli, flussi e informazioni disponibili |
| `bevy-diorama-rendering` | Rendering sobrio, simbologia, prestazioni e verifica immagini |
| `cp-serious-game-design` | Obiettivi esercitazione, autorità, risorse e debrief |
| `game-snapshot-harness` | Stati riproducibili e immagini confrontabili |
| `hazard-sim-integration` | Autorità unica, tempo simulato, scenari e forecast separati |
| `phased-multiagent-workflow` | Tracciabilità e rispetto di contributi concorrenti autorizzati |
| `public-game-release` | Prontezza esercitazione e bundle riproducibile, accessi e offline |
| `risk-communication-ux` | Stato operativo, informazione osservata/previsione e leggibilità |
| `sim-game-balance` | Laboratorio accoppiato per calibrare ipotesi e agenti |
| `synthetic-territory` | Dati e rete coerenti, scenari e varianti riproducibili |

Le skill originali nascono per un gioco divulgativo. Qui non si trasferiscono stelle,
classifiche, penalità artificiali, un solo borgo evacuabile, ricarica implicita sul
posto, tempi imposti dal gioco o autorizzazioni aggiuntive per ogni fase/screenshot.
L'autorizzazione dell'utente e le istruzioni del repository prevalgono. La skill di
coordinamento non abilita delegazione automatica in assenza di autorizzazione.

Ogni documento distingue l'adattamento dalla fonte. Il sorgente non dichiara un autore
individuale; l'attribuzione resta al progetto e alla sua storia git. Per il codice
eventualmente trasferito continuano a valere provenienza e condizioni EUPL-1.2 già
indicate nell'integrazione. Le skill non dichiarano che una procedura operativa sia
validata da autorità di protezione civile.

Il runner e il protocollo sperimentale sono descritti in
[operational-lab.md](operational-lab.md). API e opzioni grafiche devono essere verificate
nel codice TTX; le variabili `KIOSK_*` e i comandi del gioco non sono API di questo repo.
