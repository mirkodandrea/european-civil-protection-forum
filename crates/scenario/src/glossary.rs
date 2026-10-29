//! Display-language texts of the built-in Valverde scenario and of the common
//! roles and resources. Scenario content is authored in English; players and
//! the EXCON read it in their own language.
//!
//! Texts are matched as whole strings (the English source is the key), so one
//! table localises every place the string appears. Scenarios add or override
//! entries with `Scenario::translations`; whatever is missing is translated by
//! the LLM at runtime when available and otherwise stays in English.
use std::collections::BTreeMap;

use serde_json::Value;

/// Languages of the short-name rows below, in column order (Italian has its own table).
const NAME_LANGS: [&str; 10] = ["fr", "de", "es", "hy", "ka", "uk", "ro", "az", "el", "pt"];

/// Names of the standard roles and resources: English, then `NAME_LANGS`.
const NAMES: &[(&str, [&str; 10])] = &[
    ("Mayor of Valverde", ["Maire de Valverde", "Bürgermeister von Valverde", "Alcalde de Valverde", "Վալվերդեի քաղաքապետ", "ვალვერდეს მერი", "Мер Вальверде", "Primarul orașului Valverde", "Valverde bələdiyyə sədri", "Δήμαρχος του Valverde", "Presidente da Câmara de Valverde"]),
    ("Fire and Rescue Commander", ["Commandant des pompiers et secours", "Einsatzleiter Feuerwehr und Rettung", "Jefe de Bomberos y Rescate", "Հրշեջ-փրկարարական ծառայության հրամանատար", "ხანძარსაწინააღმდეგო და სამაშველო სამსახურის მეთაური", "Командир пожежно-рятувальної служби", "Comandant pompieri și salvare", "Yanğınsöndürmə və xilasetmə komandiri", "Διοικητής Πυροσβεστικής και Διάσωσης", "Comandante dos Bombeiros e Socorro"]),
    ("Regional Civil Protection", ["Protection civile régionale", "Regionaler Katastrophenschutz", "Protección Civil regional", "Տարածաշրջանային քաղաքացիական պաշտպանություն", "რეგიონული სამოქალაქო დაცვა", "Регіональний підрозділ цивільного захисту", "Protecția Civilă regională", "Regional Mülki Müdafiə", "Περιφερειακή Πολιτική Προστασία", "Proteção Civil regional"]),
    ("Road engines", ["Autopompes", "Löschfahrzeuge", "Autobombas", "Հրշեջ ավտոմեքենաներ", "სახანძრო ავტომანქანები", "Пожежні автоцистерни", "Autospeciale de stingere", "Yanğınsöndürən avtomobillər", "Πυροσβεστικά οχήματα", "Auto-tanques"]),
    ("Ground crews", ["Équipes au sol", "Bodentrupps", "Cuadrillas terrestres", "Ցամաքային խմբեր", "სახმელეთო ბრიგადები", "Наземні бригади", "Echipe terestre", "Quru qüvvələr qrupları", "Επίγειες ομάδες", "Equipas terrestres"]),
    ("Air tankers", ["Avions bombardiers d'eau", "Löschflugzeuge", "Aviones cisterna", "Ջրային ինքնաթիռներ", "წყლის სამფრქვევი თვითმფრინავები", "Пожежні літаки", "Aeronave de stingere", "Yanğınsöndürən təyyarələr", "Εναέρια μέσα κατάσβεσης", "Aviões-tanque"]),
    ("Buses", ["Autobus", "Busse", "Autobuses", "Ավտոբուսներ", "ავტობუსები", "Автобуси", "Autobuze", "Avtobuslar", "Λεωφορεία", "Autocarros"]),
    ("Monitoring drones", ["Drones de surveillance", "Überwachungsdrohnen", "Drones de vigilancia", "Մոնիտորինգի ԱԹՍ-ներ", "მონიტორინგის დრონები", "Дрони моніторингу", "Drone de monitorizare", "Monitorinq dronları", "Drones παρακολούθησης", "Drones de monitorização"]),
    ("Police patrols", ["Patrouilles de police", "Polizeistreifen", "Patrullas de policía", "Ոստիկանական պարեկներ", "პოლიციის პატრულები", "Поліцейські патрулі", "Patrule de poliție", "Polis patrulları", "Περιπολίες αστυνομίας", "Patrulhas da polícia"]),
];

/// Italian texts of the built-in scenario: names, descriptions, inject titles and senders.
const ITALIAN: &[(&str, &str)] = &[
    ("Mayor of Valverde", "Sindaco di Valverde"),
    ("Fire and Rescue Commander", "Comandante dei Vigili del fuoco e soccorso"),
    ("Regional Civil Protection", "Protezione civile regionale"),
    ("Road engines", "Autobotti"),
    ("Ground crews", "Squadre a terra"),
    ("Air tankers", "Mezzi aerei"),
    ("Buses", "Autobus"),
    ("Police patrols", "Pattuglie di polizia"),
    ("Monitoring drones", "Droni di monitoraggio"),
    ("Keeps the map of up to 4 zones updated every 5 minutes while on station (40 minutes), then returns and recharges for 15.", "Mantiene aggiornata la mappa di al massimo 4 zone ogni 5 minuti mentre è in posizione (40 minuti), poi rientra e si ricarica per 15."),
    ("Local authority for Valverde and Pineta: warnings, evacuation orders, shelters, municipal police.", "Autorità locale di Valverde e Pineta: avvisi, ordini di evacuazione, rifugi, polizia municipale."),
    ("Commands firefighting: fire lines (fuel breaks) with ground crews, water drops with air tankers, aerial monitoring with a drone.", "Dirige lo spegnimento: linee tagliafuoco (fasce senza combustibile) con le squadre a terra, lanci d'acqua con i mezzi aerei, monitoraggio dall'alto con un drone."),
    ("Regional coordination: requests reinforcements (regional and international/UCPM), opens shelters, can order evacuations and warnings.", "Coordinamento regionale: richiede rinforzi (regionali e internazionali/UCPM), apre rifugi, può ordinare evacuazioni e avvisi."),
    ("2,500 L tank; 60 m hose reach; refills at the staging base.", "Cisterna da 2.500 L; getto fino a 60 m; si rifornisce alla base di partenza."),
    ("Each crew can cut up to 600 m of fire line (fuel break) per turn.", "Ogni squadra può tracciare fino a 600 m di linea tagliafuoco (fascia senza combustibile) per turno."),
    ("Each air tanker makes one water drop (about 60 m radius) per turn.", "Ogni mezzo aereo effettua un lancio d'acqua (raggio di circa 60 m) per turno."),
    ("50 seats each; needed to evacuate schools and hospitals. Committed for the turn.", "50 posti ciascuno; servono per evacuare scuole e ospedali. Impegnati per il turno."),
    ("Each patrol can close one road to civilian traffic per turn.", "Ogni pattuglia può chiudere una strada al traffico civile per turno."),
    ("Fire reported on Pine Hill", "Incendio segnalato sulla Collina dei Pini"),
    ("School in session", "Scuola in attività"),
    ("Fire behaviour estimate", "Stima del comportamento del fuoco"),
    ("Wind strengthening", "Il vento si rafforza"),
    ("Northern Road congestion", "Congestione sulla Strada Nord"),
    ("Fuel station threatened", "Distributore di carburante minacciato"),
    ("ERCC monitoring", "Monitoraggio ERCC"),
    ("Local capacity exhausted", "Capacità locale esaurita"),
    ("112 Dispatch", "Centrale 112"),
    ("Fire operations room", "Sala operativa antincendio"),
    ("Regional weather service", "Servizio meteorologico regionale"),
    ("Municipal police", "Polizia municipale"),
    ("ERCC (EU Emergency Response Coordination Centre)", "ERCC (Centro di coordinamento della risposta alle emergenze dell'UE)"),
];

/// Built-in translations into `lang` keyed by English source text.
pub fn builtin(lang: &str) -> BTreeMap<String, String> {
    if lang == "it" {
        return ITALIAN.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    }
    let Some(col) = NAME_LANGS.iter().position(|l| *l == lang) else {
        return BTreeMap::new();
    };
    NAMES.iter().map(|(k, v)| (k.to_string(), v[col].to_string())).collect()
}

/// Replaces every string of `v` that equals a key of `map` with its translation.
/// Only whole strings change, so identifiers and free text are left alone.
pub fn localize_value(v: &mut Value, map: &BTreeMap<String, String>) {
    if map.is_empty() {
        return;
    }
    match v {
        Value::String(s) => {
            if let Some(t) = map.get(s.as_str()) {
                *s = t.clone();
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| localize_value(x, map)),
        Value::Object(o) => o.values_mut().for_each(|x| localize_value(x, map)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supported_language_has_every_name() {
        for lang in NAME_LANGS.iter().chain(&["it"]) {
            let m = builtin(lang);
            for (en, _) in NAMES {
                assert!(m.get(*en).is_some_and(|t| !t.is_empty() && t != en), "{lang}: {en}");
            }
        }
        assert!(builtin("en").is_empty());
    }

    #[test]
    fn localizes_whole_strings_only() {
        let mut v = serde_json::json!({"name": "Buses", "id": "buses", "t": ["Buses are here"]});
        localize_value(&mut v, &builtin("it"));
        assert_eq!(v["name"], "Autobus");
        assert_eq!(v["id"], "buses");
        assert_eq!(v["t"][0], "Buses are here");
    }
}
