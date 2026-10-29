//! Strings of the "Risposte" flow (28/10 playtest): the single `Rispondi`
//! composer (recipient, communication or decision, basis), the response
//! states and the conversations started from an inject. Italian and English
//! here; `i18n::t` consults this module first, other languages fall back to
//! English until translated.
use serde_json::Value;

fn it(key: &str) -> Option<&'static str> {
    Some(match key {
        "reply" | "another_reply" | "reply_again" | "conv_respond" | "reply_inject_title" | "respond_title" => "Rispondi",
        "conv_new" => "Avvia conversazione",
        "conv_create" => "Invia e avvia",
        "conv_pick" => "Con quale player?",
        "conv_first" => "Primo messaggio",
        "conv_linked" => "Conversazioni nate da questo inject",
        "conv_origin" => "Nata dall’inject",
        "conv_private_hint" => "Conversazione tra player sull’inject citato sopra; EXCON può leggerla. Un messaggio qui non è una decisione: per registrarla usa Rispondi.",
        "conv_response_tag" => "Risposta registrata",
        "resp_to" => "Destinatario",
        "resp_to_sender" => "{} (mittente dell’inject)",
        "resp_to_players" => "Uno o più player",
        "resp_to_label" => "a {}",
        "resp_kind" => "Che cosa invii?",
        "kind_comm" => "Comunicazione",
        "kind_comm_hint" => "Solo informazione: arriva subito, senza approvazione EXCON.",
        "kind_dec" => "Decisione o azione",
        "kind_dec_hint" => "Cambia la situazione (anche solo narrata): serve il fondamento e la valutazione EXCON.",
        "resp_text" => "Descrivi la tua risposta o decisione",
        "resp_text_help" => "Chi, cosa, come, dove, quando, con quali risorse, perché. Può essere una semplice comunicazione o una decisione con l’azione narrata.",
        "step_basis" => "Fondamento (obbligatorio per una decisione)",
        "basis_plan" => "Piano o procedura",
        "basis_law" => "Legge o norma",
        "basis_judgment" => "Giudizio operativo",
        "basis_hint" => "Su cosa si basa la decisione. Puoi allegare un PDF del piano o della norma.",
        "sim_action" => "Azione di simulazione associata (facoltativa, una sola)",
        "sim_action_none" => "Nessuna azione associata: la decisione resta narrata.",
        "add_action" => "Associa questa azione",
        "send_response" => "Invia",
        "need_kind" => "Scegli se è una comunicazione o una decisione.",
        "need_text" => "Scrivi la risposta.",
        "need_basis" => "Indica il fondamento della decisione.",
        "need_players" => "Scegli almeno un player.",
        "work_need_orders" => "Le risposte si inviano quando il turno è aperto.",
        "response_note" => "Ogni risposta resta nella storia dell’inject. Una comunicazione arriva subito; una decisione viene valutata da EXCON e solo un’azione approvata cambia la simulazione.",
        "rs_sent" => "Inviata",
        "rs_review" => "In valutazione",
        "rs_approved" => "Approvata",
        "rs_executed" => "Eseguita",
        "rs_outcome" => "Esito registrato",
        "kind_tag_comm" => "Comunicazione",
        "kind_tag_dec" => "Decisione",
        "nav_injects" => "Risposte",
        "injects_replies" => "Risposte per inject",
        _ => return None,
    })
}

fn en(key: &str) -> Option<&'static str> {
    Some(match key {
        "reply" | "another_reply" | "reply_again" | "conv_respond" | "reply_inject_title" | "respond_title" => "Reply",
        "conv_new" => "Start conversation",
        "conv_create" => "Send and start",
        "conv_pick" => "With which player?",
        "conv_first" => "First message",
        "conv_linked" => "Conversations started from this inject",
        "conv_origin" => "Started from the inject",
        "conv_private_hint" => "Conversation between players about the inject quoted above; EXCON can read it. A message here is not a decision: use Reply to record one.",
        "conv_response_tag" => "Recorded response",
        "resp_to" => "Recipient",
        "resp_to_sender" => "{} (sender of the inject)",
        "resp_to_players" => "One or more players",
        "resp_to_label" => "to {}",
        "resp_kind" => "What are you sending?",
        "kind_comm" => "Communication",
        "kind_comm_hint" => "Information only: delivered at once, no EXCON approval.",
        "kind_dec" => "Decision or action",
        "kind_dec_hint" => "Changes the situation (even if only narrated): needs a basis and EXCON review.",
        "resp_text" => "Describe your response or decision",
        "resp_text_help" => "Who, what, how, where, when, with which resources, why. It can be a simple communication or a decision with the action narrated.",
        "step_basis" => "Basis (required for a decision)",
        "basis_plan" => "Plan or procedure",
        "basis_law" => "Law or regulation",
        "basis_judgment" => "Operational judgement",
        "basis_hint" => "What the decision rests on. You can attach a PDF of the plan or regulation.",
        "sim_action" => "Associated simulation action (optional, only one)",
        "sim_action_none" => "No associated action: the decision stays narrated.",
        "add_action" => "Associate this action",
        "send_response" => "Send",
        "need_kind" => "Choose whether it is a communication or a decision.",
        "need_text" => "Write the response.",
        "need_basis" => "State the basis of the decision.",
        "need_players" => "Choose at least one player.",
        "work_need_orders" => "Responses are sent while the turn is open.",
        "response_note" => "Every response stays in the inject's history. A communication is delivered at once; a decision is reviewed by EXCON and only an approved action changes the simulation.",
        "rs_sent" => "Sent",
        "rs_review" => "Under review",
        "rs_approved" => "Approved",
        "rs_executed" => "Executed",
        "rs_outcome" => "Outcome recorded",
        "kind_tag_comm" => "Communication",
        "kind_tag_dec" => "Decision",
        "nav_injects" => "Responses",
        "injects_replies" => "Responses by inject",
        _ => return None,
    })
}

/// Italian for `it`, English for every other language (until translated).
pub fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    match lang {
        "it" => it(key),
        "en" => en(key),
        // Other languages keep their catalogue for the keys it already has.
        _ => None,
    }
}

/// English fallback for keys only this module knows (other languages).
pub fn fallback(key: &str) -> Option<&'static str> {
    en(key)
}

/// The state of one response, as a message key: Inviata, In valutazione,
/// Approvata, Eseguita, Esito registrato (plus clarification, rejection,
/// acknowledgement, failure). Works on a player proposal or on the `response`
/// summary of an EXCON row (`outcomes` / `executed` counts).
pub fn response_state(p: &Value) -> &'static str {
    let status = p["status"].as_str().unwrap_or("");
    let review = p["review"].as_str().or(p["decision"]["review"].as_str()).unwrap_or("");
    let outcomes = p["narrative_outcomes"].as_array().map_or(0, Vec::len) as u64 + p["outcomes"].as_u64().unwrap_or(0);
    if p["communication"] == true || status == "informational" {
        return "rs_sent";
    }
    match status {
        "parsing" | "pending" => "rs_review",
        "needs_clarification" => "clarif_needed",
        "rejected" => "st_rejected",
        "acknowledged" => "st_acknowledged",
        "superseded" => "st_superseded",
        "rolled_back" => "st_rolled_back",
        _ if outcomes > 0 => "rs_outcome",
        "failed" => "st_failed",
        "applied" => "rs_executed",
        "approved" if review == "partially_approved" => "st_partially_approved",
        "approved" => "rs_approved",
        _ => "rs_review",
    }
}

/// CSS tone of a response state.
pub fn state_tone(key: &str) -> &'static str {
    match key {
        "rs_review" | "clarif_needed" => "wait",
        "st_rejected" | "st_failed" => "bad",
        "st_partially_approved" => "partial",
        _ => "done",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn states_are_distinct_and_never_ambiguous() {
        let s = |v: Value| response_state(&v);
        assert_eq!(s(json!({"status":"informational","communication":true})), "rs_sent");
        assert_eq!(s(json!({"status":"pending"})), "rs_review");
        assert_eq!(s(json!({"status":"approved","narrative":true})), "rs_approved");
        assert_eq!(s(json!({"status":"approved","narrative_outcomes":[{"text":"ok"}]})), "rs_outcome");
        assert_eq!(s(json!({"status":"applied"})), "rs_executed");
        assert_eq!(s(json!({"status":"applied","outcomes":1})), "rs_outcome");
        assert_eq!(s(json!({"status":"approved","review":"partially_approved"})), "st_partially_approved");
        for k in ["rs_sent", "rs_review", "rs_approved", "rs_executed", "rs_outcome"] {
            assert!(!lookup("it", k).unwrap().contains("attesa"), "{k}");
            assert!(lookup("en", k).is_some());
        }
    }
}
