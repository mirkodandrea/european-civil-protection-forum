//! Player-UI translations beyond Italian/English that the per-language
//! catalogues (`i18n_lang`, `i18n_reply`, ...) do not have: player keys of
//! `i18n::en` and the English side of `i18n::pair` texts.
//!
//! The data is `i18n_more.json` (`{lang: {source: text}}`, source = key id or
//! English text), filled by `scripts/i18n/player_catalogue.py`. The tests below
//! fail on any gap, and on player screens that bypass the catalogues.
use std::collections::HashMap;
use std::sync::OnceLock;

type Catalogue = HashMap<String, HashMap<String, String>>;

fn catalogue() -> &'static Catalogue {
    static CATALOGUE: OnceLock<Catalogue> = OnceLock::new();
    CATALOGUE.get_or_init(|| serde_json::from_str(include_str!("i18n_more.json")).unwrap_or_default())
}

pub fn lookup(lang: &str, source: &str) -> Option<&'static str> {
    catalogue()
        .get(lang)?
        .get(source)
        .map(String::as_str)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [&str; 10] = ["fr", "de", "es", "pt", "ro", "el", "uk", "hy", "ka", "az"];
    /// Screens only players see: every text must go through the catalogues.
    const PLAYER_FILES: [&str; 8] = [
        "inject_panel.rs",
        "tactical.rs",
        "function.rs",
        "map.rs",
        "map_legend.rs",
        "clarify.rs",
        "player_view.rs",
        "label_layout.rs",
    ];

    fn src(name: &str) -> String {
        std::fs::read_to_string(format!("{}/src/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap()
    }

    /// Whether `body` calls the EXCON catalogue `ex("...")`.
    fn calls_ex(body: &str) -> bool {
        body.match_indices("ex(\"").any(|(i, _)| {
            !body[..i]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        })
    }

    #[test]
    fn every_player_key_is_translated_in_every_language() {
        let body = src("i18n.rs");
        let en_body = &body[body.find("fn en(key").unwrap()..];
        let keys: Vec<&str> = en_body
            .lines()
            .filter_map(|l| l.trim().strip_prefix('"'))
            .filter_map(|l| l.split_once("\" =>").map(|(k, _)| k))
            .collect();
        assert!(keys.len() > 200, "parsed {} keys", keys.len());
        let mut missing = Vec::new();
        for lang in LANGS {
            for key in &keys {
                let key: &'static str = Box::leak(key.to_string().into_boxed_str());
                let translated = crate::i18n_help::lookup(lang, key)
                    .or_else(|| crate::i18n_lang::lookup(lang, key))
                    .or_else(|| crate::i18n_function::lookup(lang, key))
                    .or_else(|| crate::i18n_place::lookup(lang, key))
                    .or_else(|| crate::i18n_reply::lookup(lang, key))
                    .or_else(|| crate::i18n_lobby::lookup(lang, key))
                    .or_else(|| crate::i18n_review::lookup(lang, key))
                    .or_else(|| lookup(lang, key));
                match translated {
                    None => missing.push(format!("{lang}/{key}")),
                    Some(t) => assert_eq!(
                        t.matches("{}").count(),
                        crate::i18n::en(key).matches("{}").count(),
                        "{lang}/{key}: placeholders differ"
                    ),
                }
            }
        }
        assert!(
            missing.is_empty(),
            "{} missing player translations (run scripts/i18n/player_catalogue.py): {missing:?}",
            missing.len()
        );
    }

    #[test]
    fn every_pair_text_is_translated_in_every_language() {
        let dir = format!("{}/src", env!("CARGO_MANIFEST_DIR"));
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let body = std::fs::read_to_string(&path).unwrap();
            for (i, _) in body.match_indices("pair(") {
                if body[..i].ends_with("fn ") {
                    continue;
                }
                // Arguments: language, Italian literal, English literal (the source).
                let call = &body[i..];
                let Some(end) = call.find(')') else { continue };
                let quoted: Vec<&str> = call[..end].split('"').skip(1).step_by(2).collect();
                let (2.., Some(en)) = (quoted.len(), quoted.last()) else { continue };
                for lang in LANGS {
                    if lookup(lang, en).is_none() {
                        missing.push(format!("{lang}/{en}"));
                    }
                }
            }
        }
        assert!(
            missing.is_empty(),
            "{} missing pair translations (run scripts/i18n/player_catalogue.py): {missing:?}",
            missing.len()
        );
    }

    #[test]
    fn player_screens_use_the_catalogues() {
        for name in PLAYER_FILES {
            let body = src(name);
            assert!(
                !calls_ex(&body) && !body.contains("exfmt!"),
                "{name}: the EXCON catalogue (Italian/English) is not for player screens; use i18n::t or i18n::pair"
            );
            for pattern in ["l==\"it\"", "l == \"it\"", "lang==\"it\"", "lang == \"it\""] {
                assert!(
                    !body.contains(&format!("if {pattern}")),
                    "{name}: an Italian/English branch leaves other languages in English; use i18n::pair"
                );
            }
        }
    }

    #[test]
    fn pair_reads_other_languages_from_the_catalogue() {
        assert_eq!(crate::i18n::pair("it", "Notifiche", "Notifications"), "Notifiche");
        assert_eq!(crate::i18n::pair("en", "Notifiche", "Notifications"), "Notifications");
        assert_eq!(crate::i18n::pair("fr", "Notifiche", "Notifications"), lookup("fr", "Notifications").unwrap());
        assert_eq!(crate::i18n::pair("xx", "Notifiche", "Notifications"), "Notifications");
    }
}
