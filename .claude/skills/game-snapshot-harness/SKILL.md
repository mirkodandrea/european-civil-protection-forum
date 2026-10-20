---
name: game-snapshot-harness
description: Produrre stati e immagini riproducibili per verificare grafica e UX del TTX.
---

# Harness di immagini TTX

Usare il percorso di snapshot già presente nel repo; prima leggere le relative opzioni
in `crates/app`, `crates/viz`, server e script. Non copiare variabili `KIOSK_*` del
progetto sorgente come se fossero implementate qui.

Ogni specifica deve identificare scenario, seme, istante simulato, sequenza di comandi,
ruolo visibile, camera, selezioni e layer attivi. Arrivare allo stato tramite il core e
comandi reali, fermare il tempo allo scatto. Reset per tempi precedenti o scenari nuovi.
Non mutare direttamente edifici o squadre per ottenere una scena impossibile.

- Preferire acquisizioni headless, invisibili e senza focus.
- Attendere l'assestamento visuale mantenendo ferma la simulazione.
- Guardare sempre le immagini prodotte; confrontare stessa camera e risoluzione.
- Verificare zenitale e obliqua, scena senza UI e quadro operativo con simboli.
- Coprire viaggio, lavoro, rifornimento, ritirata, evacuazione, chiusura locale,
  previsione pubblicata e rollback; non coprire il soggetto con un pannello.
- Salvare le specifiche insieme alle immagini e alle metriche prestazionali.

L'autorizzazione dell'utente a implementare e verificare comprende la verifica
reversibile necessaria: non ereditare dal gioco un nuovo checkpoint umano per ogni
screenshot. Se l'ambiente non consente il rendering, riportare precisamente il limite
senza dichiarare che la grafica sia stata osservata.

## Provenienza e adattamento

Adattato il 2026-10-20 dalla skill `game-snapshot-harness` del progetto `propagator_abm`
(`/Users/mirko/dev/experiments/propagator_abm/.claude/skills/game-snapshot-harness/SKILL.md`).
SHA-256 sorgente: `bcc7e79cdcfdc1c91a94ed79e95ec654a0a4af6353f677a4812a45678fddb1c2`. Il documento sorgente non dichiara un autore
individuale: si conserva l'attribuzione al progetto e alla sua storia git; non si
attribuisce la paternità a questo adattamento. Riferimento alla licenza EUPL-1.2
dell'integrazione esistente, da conservare per eventuale codice trasferito.

Le regole TTX sopra sono un adattamento, non una copia normativa del gioco:
prevalgono istruzioni dell'utente e del repository. Non si trasferiscono punteggi,
ricarica implicita, restrizioni artificiali alle evacuazioni o checkpoint umani
non richiesti. Verificare API e comandi nel repo prima di usarli.


## Harness concreto trasferito

`scripts/snapshot.py --out NUOVA_CARTELLA --spec 'name=fire,t=30,at=fire,ui=0,marks=0'`
accetta stati separati da `;`, prepara ciascuno tramite fixture TTX e riavvia il
processo. `fixture=...` applica comandi reali e approvazioni, `at=place:NOME` o
`task:ID` usa gli oggetti autorevoli. Gli angoli del batch sono gradi;
`TTX_CAM` resta in radianti. Dimensioni e scala sono esplicite. Non usare chiavi
di fasi, Mario o squadre del gioco: non esistono nel TTX e vengono rifiutate.

`--bench SECONDI` usa il `bench.rs` sorgente per CPU, render e timestamp GPU;
il clock visuale viene fermato dopo l'assestamento. Salvare JSON e numerosità
dei campioni. FPS della finestra invisibile e tempi GPU possono dipendere da
compositor e clock; non presentarli come prestazioni garantite del proiettore.
Il completamento del PNG è atteso nella callback; il runner fallisce anche se
un PNG viene salvato con errori degli shader. Manifest, specifiche, log e
`report.html` restano insieme; `--compare` affianca una cartella precedente.
