//! The player's working screen (inject panel, reply composer, clarification
//! answers) as plain data plus one message callback. `App` builds it from its
//! own state; the workflow bench (`bench`) builds it from a fake server, so
//! both drive the same rendering code.
use crate::{
    api::{list, text},
    Msg,
};
use serde_json::Value;
use std::collections::BTreeMap;
use yew::prelude::*;

pub struct PlayerView {
    /// `/api/player/<role>` payload.
    pub state: Value,
    /// `/api/static` payload.
    pub stat: Value,
    pub own: String,
    /// UI fields of the player's page (`work:*`, `inj:*`, `clar:*`, and the
    /// per-context reply drafts of `reply_composer`).
    pub fields: BTreeMap<String, String>,
    pub busy: bool,
    pub error: String,
    /// Active view ("map" shows the tactical page).
    pub active: String,
    /// Roles and contacts, for display names.
    pub roles: Vec<Value>,
    /// Draw the situation maps (the bench has no territory to draw).
    pub maps: bool,
    /// Every interaction goes back to the owner as an `App` message.
    pub link: Callback<Msg>,
}

impl PlayerView {
    pub fn lang(&self) -> String {
        match text(&self.state["player"]["language"]) {
            "" => "it".into(),
            l => l.into(),
        }
    }
    pub fn field(&self, key: &str, default: &str) -> String {
        self.fields
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.into())
    }
    pub fn messages(&self) -> &[Value] {
        list(&self.state["inbox"])
    }
    pub fn name(&self, id: &str) -> String {
        if id == "master" {
            // Players never read "EXCON": exercise control speaks as a persona.
            match text(&self.state["default_persona"]) {
                "" => "EXCON".into(),
                p => p.into(),
            }
        } else {
            self.roles
                .iter()
                .find(|r| r["id"] == id)
                .map(|r| text(&r["name"]).to_string())
                .unwrap_or(id.into())
        }
    }
    pub fn contacts(&self) -> Vec<Value> {
        self.roles
            .iter()
            .filter(|r| r["id"] != self.own)
            .cloned()
            .collect()
    }
    /// A callback that sends one message.
    pub fn cb<I: 'static>(&self, f: impl Fn(I) -> Msg + 'static) -> Callback<I> {
        let link = self.link.clone();
        Callback::from(move |i| link.emit(f(i)))
    }
}

/// `Msg::OpenInjectDetail`: a row of the inject list (empty: back to the list).
pub fn open_inject_detail(fields: &mut BTreeMap<String, String>, key: String) {
    for k in ["inj:conv", "inj:conv_reply"] {
        fields.remove(k);
    }
    fields.insert("inj:open".into(), key);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opening_an_inject_leaves_any_conversation() {
        let mut f = BTreeMap::new();
        f.insert("inj:conv".to_string(), "2".to_string());
        f.insert("reply:inj:3".to_string(), "Evacuiamo F4".to_string());
        open_inject_detail(&mut f, "3".into());
        assert_eq!(f.get("inj:open").map(String::as_str), Some("3"));
        assert!(!f.contains_key("inj:conv"));
        // Drafts are kept per inject or conversation, never thrown away by navigation.
        assert_eq!(f.get("reply:inj:3").map(String::as_str), Some("Evacuiamo F4"));
    }
}
