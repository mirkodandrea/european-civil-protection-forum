//! The player's working screen (inject panel, response composer, clarification
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
    /// UI fields of the player's composer (`work:*`, `inj:*`, `clar:*`).
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
    pub fn staged(&self) -> Vec<Value> {
        self.fields
            .get("work:staged")
            .and_then(|v| serde_json::from_str(v).ok())
            .unwrap_or_default()
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
    /// A callback that sends any number of messages.
    pub fn batch<I: 'static, M: IntoIterator<Item = Msg>>(
        &self,
        f: impl Fn(I) -> M + 'static,
    ) -> Callback<I> {
        let link = self.link.clone();
        Callback::from(move |i| f(i).into_iter().for_each(|m| link.emit(m)))
    }
}

/// Composer fields cleared once a response has been sent.
const SENT_KEYS: &[&str] = &[
    "work:location",
    "work:destination",
    "work:qty",
    "work:reply_to",
    "work:clarify",
    "work:narrative",
    "work:staged",
    "work:conversation",
    "work:basis",
    "work:file_data",
    "work:file_name",
    "work:file_size",
    "work:file_error",
    "work:sugg",
    "work:sugg_for",
    "work:sugg_busy",
    "work:kind",
    "work:to",
    "work:to_players",
];

pub fn set_staged(fields: &mut BTreeMap<String, String>, staged: Vec<Value>) {
    if staged.is_empty() {
        fields.remove("work:staged");
    } else {
        fields.insert("work:staged".into(), Value::Array(staged).to_string());
    }
}
fn staged_of(fields: &BTreeMap<String, String>) -> Vec<Value> {
    fields
        .get("work:staged")
        .and_then(|v| serde_json::from_str(v).ok())
        .unwrap_or_default()
}
/// Set the response's simulation action (`Msg::StageActions`). A response
/// carries at most one: a new one replaces the previous. An action makes the
/// response a decision.
pub fn stage(fields: &mut BTreeMap<String, String>, commands: Vec<Value>) {
    let Some(last) = commands.into_iter().last() else { return };
    set_staged(fields, vec![last]);
    fields.insert("work:kind".into(), "decision".into());
    // The form (or an AI suggestion) has been turned into a staged action.
    for key in ["work:location", "work:destination"] {
        fields.remove(key);
    }
}
/// `Msg::Unstage`.
pub fn unstage(fields: &mut BTreeMap<String, String>, index: usize) {
    let mut staged = staged_of(fields);
    if index < staged.len() {
        staged.remove(index);
    }
    set_staged(fields, staged);
}
/// Open the response composer on message `parent` (`Msg::OpenWork`, parent resolved).
/// Returns true when the draft was reset (another message than before).
pub fn open_work(fields: &mut BTreeMap<String, String>, parent: String) -> bool {
    fields.remove("work:clarify");
    fields.remove("work:conversation");
    let reset = fields
        .get("work:reply_to")
        .map(String::as_str)
        .unwrap_or("")
        != parent;
    if reset {
        for key in [
            "work:staged",
            "work:narrative",
            "work:basis",
            "work:file_data",
            "work:file_name",
            "work:file_size",
            "work:file_error",
            "work:sugg",
            "work:sugg_for",
            "work:location",
            "work:destination",
            "work:kind",
            "work:to",
            "work:to_players",
        ] {
            fields.remove(key);
        }
    }
    fields.insert("work:reply_to".into(), parent);
    fields.insert("work:open".into(), "true".into());
    reset
}
/// `Msg::OpenWorkConv`: answer the inject quoted by conversation `conv`
/// (its delivery `parent`) from inside it. The recipients default to the
/// other participants, whom the conversation is with.
pub fn open_work_conv(fields: &mut BTreeMap<String, String>, state: &Value, own: &str, parent: String, conv: String) -> bool {
    let reset = open_work(fields, parent);
    let others: Vec<String> = list(&state["linked_conversations"])
        .iter()
        .find(|c| c["id"].to_string() == conv)
        .map(|c| list(&c["participants"]).iter().map(text).filter(|p| *p != own).map(String::from).collect())
        .unwrap_or_default();
    if reset || !fields.contains_key("work:to") {
        fields.insert("work:to".into(), "players".into());
        fields.insert("work:to_players".into(), others.join(","));
    }
    fields.insert("work:conversation".into(), conv);
    reset
}
/// `Msg::OpenInjectDetail`: a row of the inject list (empty: back to the list).
pub fn open_inject_detail(fields: &mut BTreeMap<String, String>, key: String) {
    for k in [
        "inj:conv",
        "inj:newconv",
        "inj:choose",
        "inj:conv_text",
        "inj:conv_reply",
        "inj:conv_people",
    ] {
        fields.remove(k);
    }
    fields.insert("inj:open".into(), key);
}
/// The response was sent (`ActionKind::WorkSubmit`).
pub fn response_sent(fields: &mut BTreeMap<String, String>) {
    fields.insert("work:open".into(), "false".into());
    for key in SENT_KEYS {
        fields.remove(*key);
    }
}
/// A linked conversation was created (`ActionKind::ConvStart`), with its id when known.
pub fn conversation_started(fields: &mut BTreeMap<String, String>, id: Option<u64>) {
    for key in [
        "inj:newconv",
        "inj:choose",
        "inj:conv_text",
        "inj:conv_people",
        "inj:conv_reply",
    ] {
        fields.remove(key);
    }
    if let Some(id) = id {
        fields.insert("inj:conv".into(), id.to_string());
    }
}
/// A conversation message was sent (`ActionKind::ConvSend`).
pub fn conversation_sent(fields: &mut BTreeMap<String, String>) {
    fields.remove("inj:conv_text");
    fields.remove("inj:conv_reply");
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn composer_fields_follow_the_workflow() {
        let mut f = BTreeMap::new();
        assert!(open_work(&mut f, "7".into()));
        stage(
            &mut f,
            vec![
                json!({"type":"IssueWarning","zone":"F4"}),
                json!({"type":"CloseRoad","road":"r1"}),
            ],
        );
        // One action per response: the last one wins, and it makes a decision.
        assert_eq!(staged_of(&f), vec![json!({"type":"CloseRoad","road":"r1"})]);
        assert_eq!(f.get("work:kind").map(String::as_str), Some("decision"));
        // Reopening the same message keeps the draft; another one resets it.
        assert!(!open_work(&mut f, "7".into()));
        assert_eq!(staged_of(&f).len(), 1);
        unstage(&mut f, 0);
        assert!(staged_of(&f).is_empty());
        stage(&mut f, vec![json!({"type":"IssueWarning","zone":"F4"})]);
        assert!(open_work(&mut f, "8".into()));
        assert!(staged_of(&f).is_empty());
        response_sent(&mut f);
        assert_eq!(f.get("work:open").map(String::as_str), Some("false"));
        assert!(!f.contains_key("work:reply_to"));
        open_inject_detail(&mut f, "3".into());
        conversation_started(&mut f, Some(2));
        assert_eq!(f.get("inj:conv").map(String::as_str), Some("2"));
        // From a conversation the response goes to the other participants.
        let state = json!({"linked_conversations":[{"id":2,"participants":["regional_cp","fire_commander","mayor"]}]});
        open_work_conv(&mut f, &state, "fire_commander", "9".into(), "2".into());
        assert_eq!(f.get("work:to").map(String::as_str), Some("players"));
        assert_eq!(f.get("work:to_players").map(String::as_str), Some("regional_cp,mayor"));
        assert_eq!(f.get("work:conversation").map(String::as_str), Some("2"));
    }
}
