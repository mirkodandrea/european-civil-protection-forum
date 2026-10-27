---
name: bevy-diorama-rendering
description: Migliorare rendering Bevy e cartografia TTX mantenendo leggibilità e prestazioni.
---

# Rendering operativo Bevy

L'aspetto resta serio: terreno sobrio, simboli leggibili, stati operativi precisi.
La grafica legge `sim::World`; non decide pericolo, percorsi, tempi o consumo d'acqua.

- Ispezionare `crates/viz` e il viewer web prima di introdurre nuove primitive.
- Usare maschere/raster del modello per combustibile, bruciato e pericolo; una mesh
  nuova non deve alterare la simulazione. Terreno più fine può essere solo derivato.
- Strade visibili a più scale, mezzi distinguibili per forma e simbolo oltre al colore.
  Chiusure locali, stato civile e passaggio dei soccorsi devono restare distinguibili.
- Verificare winding, attributi delle mesh, z-fighting, interpolazione della quota,
  nebbia dei materiali custom e blending delle particelle. Non lasciare mesh vuote.
- Ricampionare le linee sul DEM; evitare etichette sotto la chioma o dentro gli overlay.
- Vegetazione coerente con il combustibile e diradata vicino alle strade. Un filare
  decorativo non è una barriera al fuoco. Bruciato e fiamme derivano dalle celle del core tramite il campo visuale continuo.
- Geometria per gli oggetti che sporgono, shader per superficie e dettaglio distante;
  niente scintillio o animazione che nasconda ordini ed evacuazioni.
- Pulire cache visuali, particelle e selezioni dopo reset/rollback.

Verifica: stessa scena e camera prima/dopo, vista zenitale e obliqua, UI presente e
assente; guardare le immagini. Misurare FPS/p95 sulla macchina TTX e dichiarare
l'ambiente. Non dichiarare validato l'aspetto dopo la sola compilazione.

## Provenienza e adattamento

Adattato il 2026-10-20 dalla skill `bevy-diorama-rendering` del progetto `propagator_abm`
(`/Users/mirko/dev/experiments/propagator_abm/.claude/skills/bevy-diorama-rendering/SKILL.md`).
SHA-256 sorgente: `353b4e59e4e9d55b4e6ab42406dbed25b91fef35daa6ebb053413aa2f9f63377`. Il documento sorgente non dichiara un autore
individuale: si conserva l'attribuzione al progetto e alla sua storia git; non si
attribuisce la paternità a questo adattamento. Riferimento alla licenza EUPL-1.2
dell'integrazione esistente, da conservare per eventuale codice trasferito.

Le regole TTX sopra sono un adattamento, non una copia normativa del gioco:
prevalgono istruzioni dell'utente e del repository. Non si trasferiscono punteggi,
ricarica implicita, restrizioni artificiali alle evacuazioni o checkpoint umani
non richiesti. Verificare API e comandi nel repo prima di usarli.

## Pipeline PBR trasferita il 2026-10-20

Leggere `docs/graphics.md` prima di cambiare materiali o bakes. Il renderer usa materiali distinti per terreno, foglie e
strutture; non basta modificare `StandardMaterial` o copiare un asset per portarne
la resa. Nei modelli `units.json`, preservare `surfaces` nell'alpha dei colori e
`rig` nei figli: vetro, gomma, vernice e tessuti hanno risposte diverse. La base
del materiale dei mezzi deve abilitare `clearcoat: 1.0` affinché Bevy compili
quella variante; lo shader lo disattiva sulle superfici che non lo usano.

La vegetazione usa un indice pianta Uint32 e uno stato RG32Float per chunk.
Aggiornare stato/tempo, non ricaricare le mesh; rispettare il bit dei resti e la
separazione fra alone probabilistico e incendio effettivo. Gli shader dei passaggi
di profondità e ombra devono applicare lo stesso stato del passaggio principale.
Il suolo è nel frame centrato Bevy: le maschere north-first richiedono UV e
normali coerenti con `frame::to_bevy`, anche con DEM visuale fine.

`scripts/qa/pbr.sh` produce catture invisibili e fallisce se il log contiene un
errore GPU/shader. Un PNG salvato non garantisce che lo shader sia compilato.
Ispezionare sempre tutte le immagini; gli errori WGSL non sono rilevati da
`cargo check`. Nessuna metrica FPS è implicita in una cattura riuscita.

Il campo `fire_field.rs` ricostruisce solo la presentazione: bilineare, domain warp
ripetibile e arrivo per pianta. Non usarlo per decisioni fisiche o contatori. La
modalità `V`/`TTX_FIRE_RENDER=cells` consente confronti con il raster originale;
mostrare sempre la risoluzione fisica nell'HUD. L'alone non fornisce copertura o
tempi all'incendio confermato. Proteggere le celle disboscate anche nello shader
del suolo, perché triangoli interpolati possono attraversarne il bordo.
