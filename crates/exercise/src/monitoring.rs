//! What a monitoring drone (or a ground reconnaissance crew, a one-off
//! sample) teaches its owner. The simulator samples the world
//! at each update instant of simulated time ([`sim::MonitorSample`]); here every
//! sample becomes a [`KnowledgeEvent`] of source `Monitoring`, tied to no
//! message so the inbox is not flooded. Only mission start and end send one
//! Situation message. A rollback forgets the observations after the restored
//! instant (the drone's own state comes back with the world).
use crate::knowledge::{KnowledgeEvent, KnowledgeSource};
use crate::{Exercise, MsgKind, MASTER};
use serde_json::json;
use sim::{MonitorSample, SampleKind};

/// What a drone sees.
pub const DRONE_LAYERS: [&str; 5] = ["fire", "buildings", "roads", "lines", "units"];
/// Who players hear a drone observation from.
pub const DRONE_SOURCE: &str = "Drone";
/// Who players hear a ground reconnaissance report from.
pub const RECON_SOURCE: &str = "Reconnaissance crew";
pub const RECON_SOURCE_IT: &str = "Squadra di ricognizione";

impl Exercise {
    /// Take up the samples the simulator recorded since the last call. Safe to
    /// call after any advance; a late call changes nothing but when the
    /// events are logged.
    pub(crate) fn absorb_monitoring(&mut self) {
        for sample in self.world.take_monitor_samples() {
            if self.players.contains_key(&sample.role) {
                self.absorb_sample(sample);
            }
        }
    }

    fn absorb_sample(&mut self, s: MonitorSample) {
        let start = self.world.scn.start_clock_s;
        let clock = move |t: i64| scenario::format_clock(start + t);
        let italian = self.language_of(&s.role) == "it";
        let zones = s.zones.join(", ");
        let source = if s.recon {
            if italian { RECON_SOURCE_IT } else { RECON_SOURCE }
        } else {
            DRONE_SOURCE
        };
        if s.kind != SampleKind::End {
            let layers: Vec<String> = DRONE_LAYERS.iter().map(|l| l.to_string()).collect();
            let Ok(observation) = self.zone_observation_from(&s.view, &s.zones, &layers) else {
                return;
            };
            let event = KnowledgeEvent {
                role: s.role.clone(),
                message: 0,
                turn: self.turn,
                t_s: s.t_s,
                clock: clock(s.t_s),
                source: KnowledgeSource::Monitoring,
                observation,
                timeline: self.log.iter().filter(|e| e.kind == "rollback").count(),
                source_name: Some(source.into()),
                layers,
            };
            self.record("map_information_acquired", MASTER, json!(&event));
            self.knowledge_events.push(event);
        }
        let (title, text, text_en) = match s.kind {
            SampleKind::Update => return,
            SampleKind::Start if s.recon => {
                let at = clock(s.t_s);
                (
                    if italian { "Ricognizione completata" } else { "Reconnaissance report" },
                    if italian {
                        format!("La squadra di ricognizione ha osservato la zona {zones} alle {at}: la mappa di questa zona è aggiornata a quell’ora. La squadra resta impiegata finché l’EXCON non ne conferma il rientro.")
                    } else {
                        format!("The reconnaissance crew observed zone {zones} at {at}: the map of this zone is updated to that time. The crew stays deployed until EXCON confirms its return.")
                    },
                    format!("The reconnaissance crew observed zone {zones} at {at}: the map of this zone is updated to that time. The crew stays deployed until EXCON confirms its return."),
                )
            }
            SampleKind::Start => (
                if italian { "Drone in posizione" } else { "Drone on station" },
                if italian {
                    format!("Il drone è in posizione su {zones}. La mappa di queste zone si aggiorna da sola finché dura la batteria.")
                } else {
                    format!("The drone is on station over {zones}. The map of these zones updates by itself while its battery lasts.")
                },
                format!("The drone is on station over {zones}. The map of these zones updates by itself while its battery lasts."),
            ),
            SampleKind::End => {
                let back = s.available_s.map(&clock);
                let why = s.reason.clone().unwrap_or_default();
                (
                    if italian { "Drone rientrato" } else { "Drone returned" },
                    match (&back, italian) {
                        (Some(b), true) => format!("Il drone lascia la zona ({why}) e rientra alla base: le zone {zones} non si aggiornano più. Sarà di nuovo disponibile alle {b}."),
                        (Some(b), false) => format!("The drone leaves the area ({why}) and returns to base: zones {zones} no longer update. It will be available again at {b}."),
                        (None, true) => format!("Il drone è stato richiamato ({why}): le zone {zones} non si aggiornano più."),
                        (None, false) => format!("The drone has been recalled ({why}): zones {zones} no longer update."),
                    },
                    match &back {
                        Some(b) => format!("The drone leaves the area ({why}) and returns to base: zones {zones} no longer update. It will be available again at {b}."),
                        None => format!("The drone has been recalled ({why}): zones {zones} no longer update."),
                    },
                )
            }
        };
        let mid = self.add_message(
            MASTER,
            &s.role,
            MsgKind::Situation,
            Some(title),
            &text,
            &text_en,
            None,
        );
        let persona = self.persona_for(Some(source), None);
        self.set_message_persona(mid, persona);
    }

    /// A rollback to `t_s` forgets what drones saw afterwards.
    pub(crate) fn forget_monitoring_since_restore(&mut self) {
        let t = self.world.time_s();
        self.knowledge_events
            .retain(|e| !matches!(e.source, KnowledgeSource::Monitoring) || e.t_s <= t);
    }
}
