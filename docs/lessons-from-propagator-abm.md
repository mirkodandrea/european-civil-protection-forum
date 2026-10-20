# Lezioni da propagator_abm per il TTX

Scansione del 2026-10-20. Questo documento descrive interventi proposti, non
funzionalità già trasferite. Il confronto riguarda documenti, codice e immagini
esistenti; non include un playtest interattivo o misure FPS nuove.

Gli interventi sono stati successivamente implementati su richiesta dell'utente:
stato attuale, configurazione, verifiche e limiti in
[operational-upgrade.md](operational-upgrade.md). Il testo seguente conserva il
confronto che ha motivato il trasferimento.

## Provenienza e situazione di partenza

Sorgente: `/Users/mirko/dev/experiments/propagator_abm`, HEAD
`6571839c6e23fdf41faaf0e25a442b20bc515c2a`. Il checkout contiene anche modifiche
locali a launcher/snapshot/benchmark: non tutto ciò che è presente è riproducibile
dal solo commit. Nessun file del progetto sorgente è stato modificato.

Il TTX incorpora già un porting della revisione `be08c4b`, descritto in
[operational-model.md](operational-model.md): routing, consumabili, lavoro
progressivo, traffico aggregato, esposizione e comunicazioni. Il nuovo confronto
deve concentrarsi sulle differenze successive, senza reimportare tutto.
Il confronto ricorsivo delle due directory `crates/propagator-core/src` non
mostra differenze: il valore aggiunto è negli strati sopra il kernel.
Entrambi i renderer usano Bevy 0.14 e bevy_egui 0.28, ma i modelli di scenario e
stato sono diversi: i moduli non sono sostituzioni dirette.

Roccavento è un gioco educativo, esplicitamente non un addestramento operativo.
Il TTX conserva approvazione EXCON, autorità dei ruoli, informazioni parziali,
tempi controllati e occupanti aggregati. La grafica legge lo stato; le decisioni
fisiche restano in `sim`, con stato ripristinabile per rollback.

## 1. Grafica: trasferire leggibilità e qualità del territorio

| Lezione e sorgente nel gioco | Situazione nel TTX | Applicazione proposta |
|---|---|---|
| Strade ricampionate, bordi scuri, giunti corretti (`game/src/roads.rs`) | `viz/src/objects.rs` ha già nastri ricampionati a 15 m, con larghezza uniforme | Migliorare contrasto e giunti; distinguere carrabile/sentiero e chiusura amministrativa/pericolo. Provare su pendii e tornanti. |
| Segni indipendenti da luce/nebbia e visibili sopra le chiome (`flat.rs`, `kiosk/marks.rs`) | Percorsi e stati usano soprattutto gizmo e colori | Simboli e testi brevi insieme al colore; legenda comune tra Bevy e mappe web. Rischio, evacuazione e risorsa devono essere distinguibili sul proiettore. |
| Terreno di rendering distinto dalla griglia del fuoco (`terrain_mesh.rs`) | Il renderer interpola il DEM del fuoco; l'importer non porta il terreno fine | Campo visuale opzionale più fine e materiali per suolo/roccia; mantenere esplicita la risoluzione fisica. Non inventare dettagli che sembrino nuove strade o barriere. |
| Varietà vegetali, LOD e carbonizzazione della stessa pianta (`vegetation.rs`, `shaders/retro.wgsl`) | Vegetazione già aggregata in chunk con aggiornamenti delle celle cambiate | Portare varietà e transizione vivo/carbone, con densità limitata. Mostrare chiaramente anche combustibile tagliato e tracce di intervento. |
| Modelli di mezzi e landmark (`units.rs`, `town_kit.rs`, `models.rs`) | Alcuni landmark già importati; i deployment sono cuboidi | Mezzi riconoscibili e sobri, ID e stato operativo. Separare posizione attuale e destinazione. Evitare le forti proporzioni giocattolo di `visual_scale.rs`. |
| Profili grafici (`quality.rs`, `quality_live.rs`) | Nessun modulo equivalente nei file del renderer esaminati | Profili per macchina EXCON/proiettore con ombre, LOD e densità. La qualità non deve cambiare fisica, selezione o informazioni. |

Le immagini esistenti `docs/screenshots/sim_turn2_live.png` e
`propagator_abm/web/img/tattica-squadre.jpg` mostrano il beneficio di una gerarchia
visiva più chiara; rappresentano scenari e versioni diversi, non un confronto
controllato delle prestazioni o della grafica attuale.

Direzione estetica: cartografia operativa, palette contenuta, testo leggibile e
camera stabile durante la discussione. Vista cinematica opzionale per il grande
schermo; niente profondità di campo che renda illeggibile la situazione.
Personaggi a fumetti, stelle e classifica non servono al TTX. La chiarezza della
gerarchia può invece migliorare anche console EXCON e pagine dei partecipanti.

## 2. Agenti operativi: autonomia locale entro l'ordine approvato

Il salto utile è distinguere la scelta umana della missione dall'esecuzione
tattica. EXCON approva obiettivo, risorse e area; una politica deterministica
gestisce viaggio, lavoro, acqua, attesa e sicurezza entro quei vincoli.

| Intervento | Evidenza nel gioco | Adattamento al TTX |
|---|---|---|
| Muoversi dove resta lavoro utile | `rocca/src/game.rs::engines_follow_the_fire` valuta combustibile minacciato, umidità, altre squadre e parcheggi sicuri | Oggi `sim/src/travel.rs` bagna entro 60 m dal posto fisso. Aggiungere area d'intervento autorizzata e ricollocamento su strada, senza cambiare missione da soli. |
| Evitare lavoro duplicato | `share_fuel` e soglia `soaked_pts` | Il TTX esclude già celle con almeno 30 punti d'umidità aggiunta: conservare questa protezione. Aggiungere assegnazione coordinata del lavoro; non copiare semplicemente la soglia 40 del gioco. |
| Equipaggio e mezzo con tempi distinti | `Team::crew_units`, `regroup_s`, `dispatch`; `abm/src/suppression.rs` | Possibile missione composta autobotte/equipaggio: trasferimento insieme, ultimo tratto a piedi, ricongiungimento prima del nuovo viaggio. Non richiede civili individuali. |
| Linea coordinata lungo il fronte | `crews_line_up`, `team_line` | Oggi la linea è prescritta da estremi e progredisce. Conservare il comando esplicito; offrire eventualmente una missione di settore, senza trasformare una linea approvata in un'altra. |
| Ripresa dopo ritirata | `teams_return_to_work`, interruttore `reengage` | Qui la ritirata termina come task fallito. Introdurre eventualmente sospensione e ripresa condizionata, distinguendole da fallimento e da nuova approvazione. Conservare il rifornimento fisico alla base. |
| Decisioni spiegabili | `behavior`: osservazioni limitate, grafi puri, validazione e trace | Prima estrarre politiche tipizzate con motivo, parametri e versione nel log. L'editor a nodi è una possibilità successiva, non una dipendenza del miglioramento. |

Non confondere queste unità con i partecipanti AI: i bot di ruolo sono un altro
livello, già descritto in [specs/09-ai-players.md](specs/09-ai-players.md).
Un eventuale bot usa soltanto le informazioni del proprio ruolo e produce
proposte soggette alla stessa approvazione umana. Il composer comportamentale
non è un sostituto dell'orchestrazione LLM.

## 3. Simulazione: tempi, accessibilità e previsione

**Chiusure locali delle strade — priorità alta.** In
`sim/src/lib.rs::step_roads`, una cella pericolosa imposta `cut_by_fire` per
l'intera strada; `Network::open` applica lo stato a tutti i suoi archi. Il gioco
espone `closed_stretches` e valuta il pericolo lungo i collegamenti. Portare lo
stato di pericolo a livello di segmento diretto, distinto dalla chiusura di
polizia: una tratta interrotta non deve eliminare tutti gli accessi lungo una
strada nominata. Il formato delle chiusure deve restare coerente per routing,
traffico, mappe, eventi e rollback. L'eventuale riapertura richiede una politica
esplicita; non adottare automaticamente gli 8 minuti del gioco.

**ETA e viaggio con la stessa regola.** `abm/src/drive.rs` considera classe,
pendenza, curve e traffico civile; il TTX usa classe e velocità massima per
gamba. Calcolare ETA e movimento con una sola funzione. Valutare l'effetto
delle code anche sui mezzi d'emergenza, oggi avanzati con un orologio di task
separato dai pacchetti civili. Le costanti del gioco sono ipotesi da calibrare.

**Previsione come prodotto separato.** `rocca/src/forecast.rs` e
`fire/src/ensemble.rs` proiettano un checkpoint con semi e meteo perturbati:
nessuna lettura del futuro realizzato, interventi già fatti mantenuti, nessun
nuovo lavoro delle squadre. Il TTX possiede già un ensemble autorevole: il suo
alone di probabilità descrive le realizzazioni raggiunte al tempo corrente,
non è una previsione a +1/+3 ore. Aggiungere un prodotto con ora di emissione,
orizzonti, ipotesi sugli interventi e meteo disponibile. Dal modello del gioco
si può prendere il metodo, ma l'accesso allo stato completo non va trasferito
automaticamente ai ruoli: pubblicazione e visibilità devono passare da EXCON.
Un job deve identificare generazione/rollback per scartare risultati obsoleti.

**Conservare ciò che è già migliore per il TTX.** Esposizione degli edifici
separata dal pericolo per chi viaggia, conteggio evacuati all'arrivo, capacità
dei rifugi, risorse fisiche e limiti approvati sono già presenti. Non sostituirli
con un solo borgo evacuabile alla volta, 6 persone/minuto o ricarica implicita
sul posto: sono scelte del gioco, non lezioni operative trasferibili.

## 4. La lezione più forte: laboratorio e telemetria

`docs/05-LABORATORIO.md` e `rocca/src/lab/` confrontano strategie e tarature
sugli stessi semi attraverso il motore reale. Il rapporto sorgente descrive
675 corse e benefici di saturazione, coordinamento e ripresa dopo ritirata;
questi risultati non sono stati riprodotti nella scansione e non stimano il
beneficio nel TTX. Alcune altre euristiche sono state provate e lasciate spente.

Il TTX ha già `scenario_sweep`: estenderne il metodo alle azioni e ai deployment.
Confrontare nessun intervento, attacco precoce, difesa di strutture, evacuazione
anticipata/tardiva e piani combinati su più scenari e semi accoppiati. I copioni
devono usare comandi e regole reali del TTX, comprese le risorse disponibili.

Registrare persone dentro/in transito/arrivate, danni, primo arrivo e prima
azione utile, litri, linea effettiva, tempo viaggio/lavoro/ricarica/attesa,
ritirate, motivi di inattività e sovrapposizione tra squadre. Sono evidenze per
taratura e debrief, non un voto automatico ai partecipanti. Il confronto
controfattuale descrive questo modello e le sue ipotesi.

## Sequenza proposta e criteri di accettazione

1. **Baseline e laboratorio TTX.** Salvare scenario, versione, semi, comandi e
   metriche. Stesse condizioni danno stessi risultati; meccanismi spenti
   conservano il comportamento precedente. Verificare invarianza rispetto al
   passo, conservazione delle persone e ripristino anche degli stati nuovi.
2. **Grafica operativa.** Strade, mezzi, simboli, etichette e HUD coerenti tra
   web/Bevy; poi terreno e vegetazione. Stati riproducibili prima/dopo per
   viaggio, lavoro, ricarica, ritirata, evacuazione e rollback. Ispezionare
   screenshot e movimento; misurare sulla macchina destinata al TTX.
3. **Routing per segmento ed ETA.** Provare interruzione locale, percorso
   alternativo, strada senza uscita, curve e pendenze, destinazione non
   raggiungibile e interazione tra traffico civile e soccorsi.
4. **Autonomia locale delle squadre.** Portare una politica alla volta dietro
   configurazione di scenario; confronto con baseline, log delle motivazioni,
   nessun superamento di area, risorse e mandato approvati.
5. **Previsioni revisionabili da EXCON.** Calcolo separato, nessuna mutazione
   del mondo, nessuna fuga informativa, gestione di rollback e job obsoleti.

Prima tranche consigliata: laboratorio minimo più leggibilità grafica. Permette
di valutare i successivi cambiamenti di comportamento con evidenze confrontabili.

## Verifiche di questa scansione e limiti

- `cargo test --release --offline -p sim -p fire`: **50 test passati**, inclusi
  14 test operativi. Non è stata eseguita l'intera suite workspace.
- Consultati README/guide, specifiche 00–03, TODO, laboratorio, renderer,
  routing/soppressione, esposizione, forecast e composer del progetto sorgente;
  confronto con i corrispondenti moduli e documenti TTX.
- Nessuna nuova build o esecuzione del gioco sorgente, nessuna misura delle
  prestazioni, nessuna validazione operativa o live LLM in questa scansione.
- Esempi di divergenza documentale sorgente: `01-GIOCO.md` parla di portata
  100 m e ricarica 10 min; `rocca/src/game.rs` definisce rispettivamente 70 m
  e 12 min. La suite di confronto va eseguita con configurazione effettiva
  registrata; i numeri nei documenti non bastano a fissare una taratura.
- Per ogni porting conservare provenienza e condizioni EUPL-1.2 già indicate
  nell'integrazione esistente.
