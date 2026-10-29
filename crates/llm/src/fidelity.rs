//! Wording fidelity: deterministic checks on what the model says about a
//! player's words.
//!
//! The model must not change the quantities, conditions, places or legal force
//! of a decision, nor invent content. Prompts ask for that (see
//! [`FIDELITY_RULES`]); this module checks what can be checked without a model:
//!
//! * every action may carry a verbatim **quote** of the player's words; a
//!   quote that is not in the player's text is invented, and its action is
//!   dropped;
//! * a summary or reason that uses **binding legal wording** (ban, prohibition,
//!   ordinance, obligation…) when the player's text has none is replaced by the
//!   player's own words (an invitation stays an invitation);
//! * the **modality** of a reply (decision, intention, invitation, request,
//!   report, question) labels the summary, so an intention ("I will send a
//!   team") is never read as an executed action.

/// Prompt rules shared by every task that rewords a player's text.
pub const FIDELITY_RULES: &str = "WORDING FIDELITY (strict):\n\
- Keep exactly the player's quantities, numbers, times, conditions (if, unless, only when, until), places and scope. Never round, merge, extend or narrow them.\n\
- Keep the legal force of the words. An invitation, appeal, recommendation or advice stays an invitation (\"we invite residents to avoid burning\" is NOT a ban). A ban, prohibition, ordinance, decree or obligation only if the player explicitly states one. Never add powers or legal instruments the player did not claim.\n\
- An intention or plan (\"I will send a team\", \"manderò una squadra\") is an intention, not an executed action. A report is a report, a question is a question, a request to another role is a request.\n\
- A narrative describes what the player says they do; nothing in it has happened in the simulator. Never describe it as executed, sent or completed.\n\
- Do not invent places, resources, facts, numbers, deadlines or reasons that are not in the player's text or the supplied data.\n";

/// Lower-case, punctuation to spaces, single spaces.
pub fn normalize(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `quote` is the player's own wording: it occurs in `original`
/// (ignoring case and punctuation), or every word of it does, in order (so
/// an elided quote "evacuate … F4" still counts).
pub fn quote_in(original: &str, quote: &str) -> bool {
    let (o, q) = (normalize(original), normalize(quote));
    if q.is_empty() {
        return false;
    }
    if format!(" {o} ").contains(&format!(" {q} ")) {
        return true;
    }
    let words: Vec<&str> = o.split(' ').collect();
    let mut at = 0;
    for w in q.split(' ') {
        match words[at..].iter().position(|x| *x == w) {
            Some(i) => at += i + 1,
            None => return false,
        }
    }
    true
}

/// Binding-language stems (word prefixes) in the exercise languages.
const BINDING: &[&str] = &[
    // English
    "prohibit", "forbid", "mandatory", "compulsory", "ordinance", "decree", "obligat", "outlaw",
    // Italian
    "divieto", "divieti", "vietat", "vieta", "proibi", "obblig", "ordinanz", "decreto", "interdett",
    // French
    "interdi", "obligatoire", "arrêté", "arrete",
    // German
    "verbot", "untersag", "pflicht", "verordnung",
    // Spanish / Portuguese
    "prohib", "obligatori", "obrigat", "decreto",
    // Romanian
    "interzi", "ordonan",
];
/// Whole words with binding meaning (prefixes would catch "bank", "banner").
const BINDING_WORDS: &[&str] = &["ban", "bans", "banned", "banning"];

/// Negations that make a following binding term harmless ("not a ban").
const NEGATIONS: &[&str] = &["not", "no", "non", "nicht", "kein", "keine", "pas", "nu", "não"];

/// The first binding-language term of `text` that is not negated, if any.
pub fn binding_term(text: &str) -> Option<String> {
    let n = normalize(text);
    let words: Vec<&str> = n.split(' ').collect();
    words
        .iter()
        .enumerate()
        .find(|(i, w)| {
            (BINDING_WORDS.contains(w) || BINDING.iter().any(|s| w.starts_with(s)))
                && !words[i.saturating_sub(2)..*i].iter().any(|p| NEGATIONS.contains(p))
        })
        .map(|(_, w)| w.to_string())
}

/// A binding term the model wrote in `generated` while the player's
/// `original` has no binding wording at all: the model turned an invitation
/// into a ban (or similar). Cross-language equivalence is not checked: if
/// the player used any binding word, the model may too.
pub fn escalation(original: &str, generated: &str) -> Option<String> {
    if binding_term(original).is_some() {
        return None;
    }
    binding_term(generated)
}

/// First `max` characters of `text`, with an ellipsis when cut.
pub fn excerpt(text: &str, max: usize) -> String {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max {
        t
    } else {
        format!("{}…", t.chars().take(max).collect::<String>().trim_end())
    }
}

/// What a reply does, as the model classified it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modality {
    Decision,
    Intention,
    Invitation,
    Request,
    Report,
    Question,
    Unknown,
}

impl Modality {
    pub fn parse(s: &str) -> Modality {
        match s.trim().to_lowercase().as_str() {
            "decision" | "order" => Modality::Decision,
            "intention" | "plan" => Modality::Intention,
            "invitation" | "appeal" | "recommendation" | "advice" => Modality::Invitation,
            "request" => Modality::Request,
            "report" | "information" => Modality::Report,
            "question" => Modality::Question,
            _ => Modality::Unknown,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Modality::Decision => "decision",
            Modality::Intention => "intention",
            Modality::Invitation => "invitation",
            Modality::Request => "request",
            Modality::Report => "report",
            Modality::Question => "question",
            Modality::Unknown => "",
        }
    }
    /// English label put in front of the summary for the EXCON.
    pub fn label(self) -> &'static str {
        match self {
            Modality::Intention => "Intention (not executed): ",
            Modality::Invitation => "Invitation/appeal (not a ban or order): ",
            Modality::Request => "Request: ",
            Modality::Report => "Report: ",
            Modality::Question => "Question: ",
            Modality::Decision | Modality::Unknown => "",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_must_be_the_players_words() {
        let o = "Invitiamo la popolazione di F4 a evitare abbruciamenti e fuochi.";
        assert!(quote_in(o, "invitiamo la popolazione di F4 a evitare abbruciamenti"));
        assert!(quote_in(o, "Invitiamo … evitare abbruciamenti"));
        assert!(!quote_in(o, "divieto di abbruciamento"));
        assert!(!quote_in(o, ""));
        assert!(!quote_in("evacuate F4", "evacuate F5"));
    }

    #[test]
    fn an_invitation_never_becomes_a_ban() {
        let invite = "Il sindaco invita i cittadini a evitare abbruciamenti nella zona F4";
        assert_eq!(escalation(invite, "divieto di abbruciamento").as_deref(), Some("divieto"));
        assert_eq!(escalation(invite, "Burning ban in F4").as_deref(), Some("ban"));
        assert_eq!(escalation(invite, "Prohibit burning in F4").as_deref(), Some("prohibit"));
        assert_eq!(escalation(invite, "Invitation to avoid burning in F4"), None);
        assert_eq!(escalation(invite, "Warn the bank of F4"), None, "whole words only");
        assert_eq!(escalation(invite, "Invitation (not a ban) to avoid burning"), None);
        // The player's own binding words may be reported.
        let ban = "Emetto un'ordinanza di divieto di abbruciamento in F4";
        assert_eq!(escalation(ban, "Burning ban in F4"), None);
    }

    #[test]
    fn modality_labels_intentions() {
        assert_eq!(Modality::parse("Intention"), Modality::Intention);
        assert!(Modality::parse("intention").label().contains("not executed"));
        assert_eq!(Modality::parse("whatever"), Modality::Unknown);
        assert_eq!(Modality::Decision.label(), "");
    }
}
