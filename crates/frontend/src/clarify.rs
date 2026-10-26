//! Player side of the EXCON clarification threads: a chat-like conversation
//! (EXCON asks, the player answers in text, as many rounds as EXCON wants).
//! Answers go through the ordinary reply endpoint, `reply_to` being EXCON's
//! last question, so the server links them to the same thread.
use crate::{
    api::{self, list, text},
    i18n, Action, ActionKind, App, Msg,
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
        "evidence": {"basis": {"kind": "operational_judgment", "reference": "Answer to EXCON clarification"}}})
}

impl App {
    pub fn view_clarifications(&self, ctx: &Context<Self>, can_act: bool) -> Html {
        let threads = list(&self.state["clarification_threads"]);
        if threads.is_empty() {
            return html! {};
        }
        let l = self.lang();
        let l = l.as_str();
        let own = self.own.clone();
        let card = |t: &Value| {
            let id = t["id"].to_string();
            let status = text(&t["status"]).to_string();
            let key = format!("clar:{id}");
            let draft = self.field(&key, "");
            let question = last_question(t);
            let can_answer = status == "awaiting_player" && question.is_some();
            let send = {
                let own = own.clone();
                let key = key.clone();
                let draft = draft.clone();
                ctx.link().callback(move |_| {
                    let mut a = Action::post(
                        format!("/api/player/{own}/reply"),
                        answer_body(&draft, question.unwrap_or(0)),
                    );
                    a.kind = ActionKind::ClearField(key.clone());
                    Msg::Action(a)
                })
            };
            html! {<article class={classes!("clarification-thread",format!("is-{status}"))} data-clarification-thread={id.clone()} key={id.clone()}>
                <header><h3>{i18n::t(l,"thread_title")}</h3>
                    <span class="pill">{match status.as_str(){"awaiting_player"=>i18n::t(l,"clarif_needed"),"closed"=>i18n::t(l,"thread_closed"),_=>i18n::t(l,"thread_wait")}}</span></header>
                <ol class="clarify-chat">{for list(&t["entries"]).iter().map(|e|{
                    let mine=e["author"]=="player";
                    html!{<li key={e["message"].to_string()} class={classes!("clarify-msg",if mine{"mine"}else{"theirs"})}>
                        <small><b>{if mine{i18n::t(l,"thread_you")}else{"EXCON"}}</b>{format!(" · {}",text(&e["clock"]))}</small>
                        <p>{text(&e["text"])}</p></li>}
                })}</ol>
                if can_answer{<div class="clarify-reply">
                    <textarea aria-label={i18n::t(l,"thread_answer")} placeholder={i18n::t(l,"thread_answer")} value={draft.clone()} oninput={{let key=key.clone();ctx.link().callback(move |e|Msg::Field(key.clone(),api::input(e)))}}/>
                    <button class="primary" data-send-clarification={id.clone()} disabled={self.busy||!can_act||draft.trim().is_empty()} onclick={send}>{i18n::t(l,"thread_send")}</button></div>}
            </article>}
        };
        let (open, closed): (Vec<&Value>, Vec<&Value>) =
            threads.iter().partition(|t| t["status"] != "closed");
        html! {<section class="clarification-threads" id="clarification-threads">
            {for open.into_iter().map(|t| card(t))}
            if !closed.is_empty(){<details class="clarification-closed"><summary>{i18n::t(l,"thread_closed")}</summary>{for closed.into_iter().map(|t| card(t))}</details>}
        </section>}
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
