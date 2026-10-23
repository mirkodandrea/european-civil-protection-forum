//! Deterministic policy for player-to-player chat.
//!
//! This is deliberately separate from operational-command validation: EXCON
//! prompts and reviewed actions are not chat and do not pass through it.
use anyhow::{bail, Result};

const BLOCKED: &[&str] = &[
    "asshole",
    "bastard",
    "bitch",
    "bullshit",
    "crap",
    "cunt",
    "damn",
    "dick",
    "dumbass",
    "fuck",
    "fucker",
    "fucking",
    "idiot",
    "idiots",
    "jackass",
    "jerk",
    "motherfucker",
    "moron",
    "piss",
    "prick",
    "shit",
    "shithead",
    "shitty",
    "stupid",
    "twat",
    "wanker",
    "bastarda",
    "bastarde",
    "bastardi",
    "bastardo",
    "cazzo",
    "cazzi",
    "coglione",
    "coglioni",
    "cretino",
    "cretina",
    "cretini",
    "cretine",
    "deficiente",
    "deficienti",
    "fica",
    "figa",
    "fighe",
    "imbecille",
    "imbecilli",
    "idiota",
    "merda",
    "puttana",
    "puttane",
    "puttani",
    "stronza",
    "stronze",
    "stronzo",
    "stronzi",
    "vaffanculo",
    "fanculo",
    "stupido",
    "stupida",
    "stupidi",
    "stupide",
    "mannaggia",
    "connard",
    "connasse",
    "encule",
    "merde",
    "putain",
    "salope",
    "scheisse",
    "arschloch",
    "fotze",
    "hurensohn",
    "mierda",
    "joder",
    "puta",
    "puto",
    "cabron",
    "gilipollas",
    "caralho",
    "foda",
    "babaca",
    "pula",
    "rahat",
    "სულელი",
    "ნაბიჭვარი",
    "ყლე",
    "անասուն",
    "տականք",
    "քաք",
    "պոռնիկ",
    "քած",
    "blyat",
    "suka",
    "хуй",
    "хуи",
    "пизда",
    "блядь",
    "блять",
    "сука",
    "гондон",
    "гівно",
    "хуйня",
    "пиздець",
    "μαλάκας",
    "μαλακας",
    "σκατά",
    "σκατα",
    "πούτσα",
    "πουτσα",
    "γαμώ",
    "γαμω",
];

const AZERI_BLOCKED: &[&str] = &["sik", "sikik", "qehbe", "göt", "got", "alcag", "alçaq"];

fn fold(c: char) -> Option<char> {
    Some(match c {
        '0' => 'o',
        '1' | '!' | '|' => 'i',
        '3' => 'e',
        '4' | '@' => 'a',
        '5' | '$' => 's',
        '7' => 't',
        '8' => 'b',
        'a'..='z' | '2' | '6' | '9' => c,
        'á' | 'à' | 'â' | 'ä' | 'ã' | 'å' => 'a',
        'ç' => 'c',
        'é' | 'è' | 'ê' | 'ë' => 'e',
        'í' | 'ì' | 'î' | 'ï' => 'i',
        'ñ' => 'n',
        'ó' | 'ò' | 'ô' | 'ö' | 'õ' => 'o',
        'ú' | 'ù' | 'û' | 'ü' => 'u',
        'ý' | 'ÿ' => 'y',
        _ if c.is_alphanumeric() => c,
        _ => return None,
    })
}

fn is_blocked(word: &str, language: &str) -> bool {
    BLOCKED.contains(&word) || (language == "az" && AZERI_BLOCKED.contains(&word))
}

fn contains_profanity(text: &str, language: &str) -> bool {
    let mut spaced_letters = String::new();
    for whitespace_word in text.split_whitespace() {
        let folded = whitespace_word
            .chars()
            .flat_map(char::to_lowercase)
            .filter_map(fold)
            .collect::<String>();
        // Full token catches punctuation inserted inside a word (f.u.c.k),
        // while punctuation-separated tokens catch normal sentence endings.
        if is_blocked(&folded, language)
            || whitespace_word
                .split(|c: char| !c.is_alphanumeric())
                .map(|part| {
                    part.chars()
                        .flat_map(char::to_lowercase)
                        .filter_map(fold)
                        .collect::<String>()
                })
                .any(|part| is_blocked(&part, language))
        {
            return true;
        }
        if folded.chars().count() == 1 {
            spaced_letters.push_str(&folded);
            let chars = spaced_letters.chars().collect::<Vec<_>>();
            if (4..=chars.len().min(16)).any(|length| {
                chars.windows(length).any(|window| {
                    let candidate = window.iter().collect::<String>();
                    is_blocked(&candidate, language)
                })
            }) {
                return true;
            }
        } else {
            spaced_letters.clear();
        }
    }
    false
}

/// Reject an insulting or profane player message without echoing its contents.
pub fn validate_player_chat(text: &str, language: &str) -> Result<()> {
    if contains_profanity(text, language) {
        if language == "it" {
            bail!("Messaggio rifiutato: evita insulti e volgarità.");
        }
        bail!("Message rejected: avoid insults and profanity.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_english_and_italian_insults_with_simple_obfuscation() {
        for text in [
            "That is fucking unsafe",
            "what a b!tch",
            "vàffanculo",
            "che strònzo",
            "c.o.g.l.i.o.n.e",
        ] {
            assert!(contains_profanity(text, "en"), "{text}");
        }
        for text in ["Sei stupido", "bastardo", "vaffanculo", "cretine"] {
            assert!(contains_profanity(text, "it"), "{text}");
        }
    }

    #[test]
    fn respects_word_boundaries_and_does_not_echo_offending_text() {
        for text in [
            "Scunthorpe",
            "class begins",
            "assessment",
            "passage",
            "assist",
        ] {
            assert!(!contains_profanity(text, "en"), "{text}");
        }
        assert!(contains_profanity("f u c k", "en"));
        assert!(!contains_profanity("I got your message", "en"));
        assert!(!contains_profanity("Got it", "en"));
        assert!(contains_profanity("GÖT", "az"));
        assert!(!contains_profanity("GÖT", "en"));
        let error = validate_player_chat("you shit", "en")
            .unwrap_err()
            .to_string();
        assert_eq!(error, "Message rejected: avoid insults and profanity.");
        assert!(!error.contains("shit"));
        assert_eq!(
            validate_player_chat("stronzo", "it")
                .unwrap_err()
                .to_string(),
            "Messaggio rifiutato: evita insulti e volgarità."
        );
    }
}
