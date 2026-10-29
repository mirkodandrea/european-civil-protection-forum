//! The benches: each a small scenario of three or four interactions, the
//! columns it shows and its steps. A step's status is read from the fake
//! server and the players' composer fields, never stored.
use super::fake::{Fake, Proposal};
use serde_json::json;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Column {
    Excon,
    Player(&'static str),
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Done,
    Active,
    Pending,
}
impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Done => "done",
            Status::Active => "active",
            Status::Pending => "pending",
        }
    }
}

/// What the step checks look at.
pub struct World<'a> {
    pub fake: &'a Fake,
    pub fields: &'a BTreeMap<String, BTreeMap<String, String>>,
}
impl World<'_> {
    fn field(&self, role: &str, key: &str) -> &str {
        self.fields
            .get(role)
            .and_then(|f| f.get(key))
            .map(String::as_str)
            .unwrap_or("")
    }
    /// The latest response of `role` still in play.
    fn latest(&self, role: &str) -> Option<&Proposal> {
        self.fake
            .proposals
            .iter()
            .rev()
            .find(|p| p.role == role && p.status != "superseded")
    }
    fn inject_to(&self, role: &str) -> Option<u64> {
        self.fake
            .messages
            .iter()
            .filter(|m| m.kind == "inject" && m.to == role)
            .map(|m| m.id)
            .max()
    }
    /// The player has the inject open in their list.
    fn opened(&self, role: &str) -> bool {
        self.inject_to(role)
            .is_some_and(|id| self.field(role, "inj:open") == id.to_string())
    }
    fn decided(&self, role: &str) -> bool {
        self.latest(role).is_some_and(|p| p.decision.is_some())
    }
}

pub struct Step {
    /// Who acts.
    pub side: Column,
    pub text: (&'static str, &'static str),
    pub done: fn(&World) -> bool,
}

pub struct BenchDef {
    pub id: &'static str,
    pub title: (&'static str, &'static str),
    pub about: (&'static str, &'static str),
    pub columns: &'static [Column],
    pub seed: fn(&mut Fake),
    pub steps: &'static [Step],
}

/// Done steps stay done; the first open one is active.
pub fn statuses(b: &BenchDef, w: &World) -> Vec<Status> {
    let mut active = false;
    b.steps
        .iter()
        .map(|s| {
            if (s.done)(w) {
                Status::Done
            } else if !active {
                active = true;
                Status::Active
            } else {
                Status::Pending
            }
        })
        .collect()
}

const FIRE: (&str, &str, &str) = (
    "I1",
    "Fumo sopra Valverde",
    "Il 112 segnala fumo denso sulla collina a nord del paese, vento verso le case della zona F4.",
);
const TRAFFIC: (&str, &str, &str) = (
    "I2",
    "Code sulla SP12",
    "La SP12 verso Pineta è congestionata: i curiosi rallentano i mezzi di soccorso.",
);

fn deliver(
    f: &mut Fake,
    inject: (&'static str, &'static str, &'static str),
    to: &[&'static str],
) -> u64 {
    f.plan(inject.0, inject.1, inject.2, to);
    let delivery = f
        .send(
            inject.0,
            &to.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
        )
        .expect("seed inject");
    f.delivered(to[0], delivery).map(|m| m.id).unwrap_or(0)
}

const EXCON_MAYOR: &[Column] = &[Column::Excon, Column::Player("mayor")];
const EXCON_POLICE: &[Column] = &[Column::Excon, Column::Player("police")];

pub static BENCHES: &[BenchDef] = &[
    BenchDef {
        id: "inject",
        title: ("Inject → narrativa", "Inject → narrative"),
        about: ("L’EXCON invia un inject, il Sindaco lo legge e risponde con una narrativa, l’EXCON la trova in Risposte.", "EXCON sends an inject, the Mayor reads it and answers with a narrative, EXCON finds it in Responses."),
        columns: EXCON_MAYOR,
        seed: |f| f.plan(FIRE.0, FIRE.1, FIRE.2, &["mayor"]),
        steps: &[
            Step { side: Column::Excon, text: ("invia l’inject (Inject pianificati → Invia…)", "sends the inject (Planned injects → Send…)"), done: |w| w.inject_to("mayor").is_some() },
            Step { side: Column::Player("mayor"), text: ("apre l’inject nella sua lista", "opens the inject in their list"), done: |w| w.opened("mayor") || w.latest("mayor").is_some() },
            Step { side: Column::Player("mayor"), text: ("risponde con una narrativa", "answers with a narrative"), done: |w| w.latest("mayor").is_some_and(|p| p.narrative) },
            Step { side: Column::Excon, text: ("apre la risposta in Risposte e approva l’iniziativa", "opens the response in Responses and approves the initiative"), done: |w| w.decided("mayor") },
        ],
    },
    BenchDef {
        id: "suggest",
        title: ("Suggerimenti AI al player", "AI suggestions to the player"),
        about: ("La Polizia scrive una narrativa: l’AI (simulata) propone azioni, la Polizia le aggiunge e invia; l’EXCON vede le azioni.", "The Police writes a narrative: the (simulated) AI proposes actions, the Police adds them and sends; EXCON sees the actions."),
        columns: EXCON_POLICE,
        seed: |f| {
            deliver(f, TRAFFIC, &["police"]);
        },
        steps: &[
            Step { side: Column::Player("police"), text: ("apre l’inject e sceglie «Rispondi»", "opens the inject and picks “Reply”"), done: |w| w.field("police", "work:open") == "true" || w.latest("police").is_some() },
            Step { side: Column::Player("police"), text: ("scrive la narrativa (≥ 20 caratteri): arrivano i suggerimenti", "writes the narrative (≥ 20 characters): suggestions arrive"), done: |w| w.fake.trace.player_suggestions > 0 },
            Step { side: Column::Player("police"), text: ("associa un’azione suggerita (una sola per risposta)", "associates one suggested action (one per response)"), done: |w| !w.field("police", "work:staged").is_empty() || w.latest("police").is_some_and(|p| !p.commands.is_empty()) },
            Step { side: Column::Player("police"), text: ("invia: l’EXCON vede narrativa e azioni in Risposte", "sends: EXCON sees narrative and actions in Responses"), done: |w| w.latest("police").is_some_and(|p| p.narrative && !p.commands.is_empty()) },
        ],
    },
    BenchDef {
        id: "excon-suggest",
        title: ("EXCON: suggerisci azioni", "EXCON: suggest actions"),
        about: ("Il Sindaco ha risposto solo con una narrativa. L’EXCON chiede all’AI (simulata) le azioni corrispondenti, le aggiunge e approva; il Sindaco vede l’esito.", "The Mayor answered with a narrative only. EXCON asks the (simulated) AI for matching actions, adds them and approves; the Mayor sees the outcome."),
        columns: EXCON_MAYOR,
        seed: |f| {
            let inject = deliver(f, FIRE, &["mayor"]);
            f.respond("mayor", "Apriamo la palestra comunale come area di accoglienza e avvisiamo la popolazione della zona F4.", Vec::new(), Some(inject), None).expect("seed response");
        },
        steps: &[
            Step { side: Column::Excon, text: ("apre la risposta e preme «Suggerisci azioni»", "opens the response and presses “Suggest actions”"), done: |w| w.fake.trace.excon_suggestions > 0 },
            Step { side: Column::Excon, text: ("aggiunge le azioni (verificate)", "adds the actions (checked)"), done: |w| w.fake.trace.rechecks > 0 || w.decided("mayor") },
            Step { side: Column::Excon, text: ("approva", "approves"), done: |w| w.latest("mayor").is_some_and(|p| p.status == "approved" && !p.commands.is_empty()) },
            Step { side: Column::Player("mayor"), text: ("apre l’inject e vede l’esito", "opens the inject and sees the outcome"), done: |w| w.decided("mayor") && w.opened("mayor") },
        ],
    },
    BenchDef {
        id: "clarify",
        title: ("Chiarimento", "Clarification"),
        about: ("Il Sindaco chiede di evacuare «la zona»: l’EXCON chiede un chiarimento, il Sindaco risponde nel thread, l’EXCON chiude il chiarimento.", "The Mayor asks to evacuate “the area”: EXCON asks for a clarification, the Mayor answers in the thread, EXCON closes it."),
        columns: EXCON_MAYOR,
        seed: |f| {
            let inject = deliver(f, FIRE, &["mayor"]);
            f.respond("mayor", "Evacuiamo subito la zona minacciata.", vec![json!({"type":"EvacuateZone","zone":"F4"})], Some(inject), None).expect("seed response");
        },
        steps: &[
            Step { side: Column::Excon, text: ("scrive la domanda nella nota e preme «Chiedi chiarimento»", "writes the question in the note and presses “Ask for clarification”"), done: |w| !w.fake.threads.is_empty() },
            Step { side: Column::Player("mayor"), text: ("apre l’inject e risponde nel thread", "opens the inject and answers in the thread"), done: |w| w.fake.threads.iter().any(|t| t.entries.iter().skip_while(|e| e.0 == "player").any(|e| e.0 == "player")) },
            Step { side: Column::Excon, text: ("chiude il chiarimento", "closes the clarification"), done: |w| w.fake.threads.iter().any(|t| t.status == "closed") },
        ],
    },
    BenchDef {
        id: "conversation",
        title: ("Conversazione tra player", "Conversation between players"),
        about: ("Sindaco e Polizia ricevono lo stesso inject e ne parlano in una conversazione collegata; la risposta che ne esce porta con sé la trascrizione, che l’EXCON legge in Risposte.", "Mayor and Police receive the same inject and discuss it in a linked conversation; the response coming out of it carries the transcript, which EXCON reads in Responses."),
        columns: &[Column::Excon, Column::Player("mayor"), Column::Player("police")],
        seed: |f| {
            deliver(f, FIRE, &["mayor", "police"]);
        },
        steps: &[
            Step { side: Column::Player("mayor"), text: ("apre l’inject e avvia una conversazione con la Polizia", "opens the inject and starts a conversation with the Police"), done: |w| !w.fake.conversations.is_empty() },
            Step { side: Column::Player("police"), text: ("apre la conversazione e risponde", "opens the conversation and replies"), done: |w| w.fake.conversations.iter().any(|c| c.messages.iter().any(|m| m.1 == "police")) },
            Step { side: Column::Player("mayor"), text: ("risponde all’inject dalla conversazione", "answers the inject from the conversation"), done: |w| w.fake.proposals.iter().any(|p| p.via_conversation.is_some()) },
            Step { side: Column::Excon, text: ("vede la risposta e la trascrizione in Risposte e decide", "sees the response and the transcript in Responses and decides"), done: |w| w.fake.proposals.iter().any(|p| p.via_conversation.is_some() && p.decision.is_some()) },
        ],
    },
    BenchDef {
        id: "decide",
        title: ("Decisione sulle azioni", "Decision on actions"),
        about: ("La Polizia ha proposto due azioni. L’EXCON approva, approva in parte (togli la spunta e scrivi il motivo), modifica o rifiuta (serve una nota); la Polizia vede l’esito.", "The Police proposed two actions. EXCON approves, approves in part (untick and give the reason), edits or rejects (a note is needed); the Police sees the outcome."),
        columns: EXCON_POLICE,
        seed: |f| {
            let inject = deliver(f, TRAFFIC, &["police"]);
            let commands = super::fake::canned_suggestions("police").into_iter().map(|c| c.0).collect();
            f.respond("police", "", commands, Some(inject), None).expect("seed response");
        },
        steps: &[
            Step { side: Column::Player("police"), text: ("ha inviato due azioni (già fatto)", "has sent two actions (already done)"), done: |w| w.latest("police").is_some() },
            Step { side: Column::Excon, text: ("apre la risposta e approva, approva in parte o rifiuta", "opens the response and approves, approves in part or rejects"), done: |w| w.decided("police") },
            Step { side: Column::Player("police"), text: ("apre l’inject e vede l’esito per ogni azione", "opens the inject and sees the outcome of each action"), done: |w| w.decided("police") && w.opened("police") },
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bench_starts_with_an_active_step_and_seeds_cleanly() {
        for b in BENCHES {
            let mut f = Fake::new("it");
            (b.seed)(&mut f);
            let fields = BTreeMap::new();
            let s = statuses(
                b,
                &World {
                    fake: &f,
                    fields: &fields,
                },
            );
            assert!((3..=4).contains(&s.len()), "{}", b.id);
            assert_eq!(
                s.iter().filter(|s| **s == Status::Active).count(),
                1,
                "{}",
                b.id
            );
            assert!(
                s.iter()
                    .skip_while(|s| **s == Status::Done)
                    .all(|s| *s != Status::Done),
                "{}",
                b.id
            );
        }
    }

    #[test]
    fn steps_follow_the_fake_server() {
        let b = BENCHES.iter().find(|b| b.id == "clarify").unwrap();
        let mut f = Fake::new("it");
        (b.seed)(&mut f);
        let fields = BTreeMap::new();
        let pid = f.proposals[0].id;
        f.handle(
            "master",
            &format!("/api/master/proposals/{pid}/clarify"),
            Some(json!({"note":"Quale zona?"})),
        )
        .unwrap();
        let s = statuses(
            b,
            &World {
                fake: &f,
                fields: &fields,
            },
        );
        assert_eq!(s, vec![Status::Done, Status::Active, Status::Pending]);
    }
}
