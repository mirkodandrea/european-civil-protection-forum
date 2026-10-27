//! Who exercise control speaks as. Players never see "EXCON": whatever EXCON
//! writes to a player carries a persona (a scenario role, an excon part, an
//! inject sender or free text). Without one the inject's sender applies, then
//! the scenario's default persona.
use crate::{Exercise, Message, MsgKind, MASTER};
use serde_json::{json, Value};

/// Longest persona name stored.
const MAX_PERSONA: usize = 80;

/// Is `name` one of the labels that mean "exercise control" itself?
pub fn is_excon_label(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || n.eq_ignore_ascii_case("excon") || n.eq_ignore_ascii_case(MASTER)
}

fn clean(s: &str) -> Option<String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!is_excon_label(&s)).then(|| s.chars().take(MAX_PERSONA).collect())
}

impl Exercise {
    /// Persona to store for something written now: the explicit `requested`
    /// name, else the sender of the inject (`inject_id` = delivery id) the
    /// text belongs to, else `None` (the scenario default applies on display).
    pub fn persona_for(&self, requested: Option<&str>, inject_id: Option<u64>) -> Option<String> {
        if let Some(p) = requested.and_then(clean) {
            return Some(p);
        }
        let root = inject_id?;
        self.messages
            .iter()
            .find(|m| m.inject_id == Some(root) && m.kind == MsgKind::Inject)
            .and_then(|m| clean(&m.from))
    }

    /// Name shown to a player speaking `language`: the persona, else the default.
    pub fn persona_name(&self, persona: Option<&str>, language: &str) -> String {
        match persona.filter(|p| !is_excon_label(p)) {
            Some(p) => p.to_string(),
            None => self.world.scn.default_persona_name(language),
        }
    }

    /// Persona shown for a message the player reads: `None` when a participant wrote it.
    pub fn message_persona(&self, m: &Message, language: &str) -> Option<String> {
        if self.players.contains_key(&m.from) {
            return None;
        }
        // Injects carry their simulated sender in `from`.
        let own = m.persona.as_deref().or(Some(m.from.as_str()));
        Some(self.persona_name(own, language))
    }

    pub fn set_message_persona(&mut self, message: u64, persona: Option<String>) {
        if let Some(m) = self.messages.iter_mut().find(|m| m.id == message) {
            m.persona = persona;
        }
    }

    /// Choices EXCON has when it speaks to players.
    pub fn personas(&self) -> Value {
        let scn = &self.world.scn;
        let mut senders: Vec<String> = Vec::new();
        for s in scn
            .excon_roles
            .iter()
            .chain(scn.msel.iter().map(|m| &m.sender))
        {
            let s = s.trim();
            if !is_excon_label(s) && !senders.iter().any(|x| x == s) {
                senders.push(s.to_string());
            }
        }
        json!({
            "roles": scn.roles.iter().map(|r| json!({"id": r.id, "name": r.name})).collect::<Vec<_>>(),
            "senders": senders,
            "default": scn.default_persona_name("it"),
            "default_en": scn.default_persona_name("en"),
        })
    }

    /// Replace any leftover "EXCON" in a text a player reads by the default persona.
    pub fn scrub_for_player(&self, text: &str, language: &str) -> String {
        if !text.contains("EXCON") {
            return text.to_string();
        }
        text.replace("EXCON", &self.persona_name(None, language))
    }
}
