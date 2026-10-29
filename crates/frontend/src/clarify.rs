//! Player side of the EXCON clarification threads: the text-answer composer
//! (EXCON asks, the player answers in text, as many rounds as EXCON wants).
//! The conversation itself is drawn by `inject_panel`.
//! Answers go through the ordinary reply endpoint, `reply_to` being EXCON's
//! last question, so the server links them to the same thread.
use crate::{
    api::{self, list},
    i18n, Action, ActionKind, Msg,
};
use serde_json::{json, Value};
use yew::prelude::*;

/// Id of EXCON's latest message in the thread (the one an answer replies to).
pub fn last_question(thread: &Value) -> Option<u64> {
    list(&thread["entries"])
        .iter()
        .rev()
        .find(|e| e["author"] == "excon")
        .and_then(|e| e["message"].as_u64())
}

/// Body of the answer request.
pub fn answer_body(text: &str, reply_to: u64) -> Value {
    json!({"text": text.trim(), "reply_to": reply_to,
        "evidence": {"basis": {"kind": "operational_judgment", "reference": "Answer to clarification"}}})
}

impl crate::player_view::PlayerView {
    /// Text-answer composer of a clarification awaiting the player (Ctrl/Cmd+Enter sends).
    pub fn clarify_composer(&self, can_act: bool, t: &Value, extra: Html) -> Html {
        let l = self.lang();
        let l = l.as_str();
        let id = t["id"].to_string();
        let key = format!("clar:{id}");
        let draft = self.field(&key, "");
        let question = last_question(t).unwrap_or(0);
        let send = {
            let own = self.own.clone();
            let key = key.clone();
            let draft = draft.clone();
            self.cb(move |_: ()| {
                let mut a = Action::post(format!("/api/player/{own}/reply"), answer_body(&draft, question));
                a.kind = ActionKind::ClearField(key.clone());
                Msg::Action(a)
            })
        };
        let ready = !self.busy && can_act && !draft.trim().is_empty();
        html! {<div class="clarify-reply" data-clarification-thread={id.clone()}>
            <textarea aria-label={i18n::t(l,"thread_answer")} placeholder={i18n::t(l,"thread_answer")} value={draft.clone()} oninput={{let key=key.clone();self.cb(move |e|Msg::Field(key.clone(),api::input(e)))}}
                onkeydown={{let send=send.clone();Callback::from(move |e:KeyboardEvent|{if ready&&e.key()=="Enter"&&(e.ctrl_key()||e.meta_key()){e.prevent_default();send.emit(());}})}}/>
            <div class="clarify-actions"><span class="clarify-links">{extra}<small class="muted">{i18n::t(l,"thread_hint")}</small></span>
                <button class="primary" data-send-clarification={id} disabled={!ready} onclick={send.reform(|_: MouseEvent| ())}>{i18n::t(l,"thread_send")}</button></div></div>}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn answers_reply_to_the_latest_excon_message() {
        let t = json!({"entries":[{"author":"player","message":1},{"author":"excon","message":2},{"author":"player","message":3},{"author":"excon","message":4}]});
        assert_eq!(last_question(&t), Some(4));
        assert_eq!(last_question(&json!({"entries":[]})), None);
        let b = answer_body("  F4 ", 4);
        assert_eq!((b["text"].as_str(), b["reply_to"].as_u64()), (Some("F4"), Some(4)));
        assert_eq!(b["evidence"]["basis"]["kind"], "operational_judgment");
    }
}
