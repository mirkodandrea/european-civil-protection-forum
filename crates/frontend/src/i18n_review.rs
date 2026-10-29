//! Player-UI strings for the EXCON verdict on a proposal (partial approval,
//! acknowledgement, per-action outcome), in every player language.
const KEYS: [&str; 4] = ["st_partially_approved", "st_acknowledged", "act_approved", "act_not_approved"];
const IT: [&str; 4] = ["Approvata in parte", "Presa visione · nessuna azione da eseguire", "Approvata", "Non approvata"];
const EN: [&str; 4] = ["Partially approved", "Acknowledged · nothing to execute", "Approved", "Not approved"];
const FR: [&str; 4] = ["Approuvée en partie", "Prise en compte · rien à exécuter", "Approuvée", "Non approuvée"];
const DE: [&str; 4] = ["Teilweise genehmigt", "Zur Kenntnis genommen · nichts auszuführen", "Genehmigt", "Nicht genehmigt"];
const ES: [&str; 4] = ["Aprobada en parte", "Recibido · nada que ejecutar", "Aprobada", "No aprobada"];
const PT: [&str; 4] = ["Aprovada em parte", "Tomado conhecimento · nada a executar", "Aprovada", "Não aprovada"];
const RO: [&str; 4] = ["Aprobată parțial", "Luat la cunoștință · nimic de executat", "Aprobată", "Neaprobată"];
const EL: [&str; 4] = ["Εγκρίθηκε εν μέρει", "Ελήφθη υπόψη · τίποτα προς εκτέλεση", "Εγκρίθηκε", "Δεν εγκρίθηκε"];
const UK: [&str; 4] = ["Схвалено частково", "Взято до відома · нічого виконувати", "Схвалено", "Не схвалено"];
const HY: [&str; 4] = ["Մասնակի հաստատված", "Ընդունված է ի գիտություն · կատարելու բան չկա", "Հաստատված", "Չհաստատված"];
const KA: [&str; 4] = ["ნაწილობრივ დამტკიცდა", "მიღებულია ცნობად · შესასრულებელი არაფერია", "დამტკიცდა", "არ დამტკიცდა"];
const AZ: [&str; 4] = ["Qismən təsdiqləndi", "Nəzərə alındı · icra ediləcək bir şey yoxdur", "Təsdiqləndi", "Təsdiqlənmədi"];

pub fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    let table: &[&str; 4] = match lang {
        "it" => &IT,
        "en" => &EN,
        "fr" => &FR,
        "de" => &DE,
        "es" => &ES,
        "pt" => &PT,
        "ro" => &RO,
        "el" => &EL,
        "uk" => &UK,
        "hy" => &HY,
        "ka" => &KA,
        "az" => &AZ,
        // Unknown languages use English, like the other catalogues.
        _ => &EN,
    };
    KEYS.iter().position(|k| *k == key).map(|i| table[i])
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_language_translates_every_key() {
        for l in ["it", "en", "fr", "de", "es", "pt", "ro", "el", "uk", "hy", "ka", "az"] {
            for k in super::KEYS {
                let s = super::lookup(l, k).unwrap();
                assert!(!s.is_empty() && s != k, "{l}/{k}");
            }
        }
        assert_ne!(super::lookup("it", "act_approved"), super::lookup("en", "act_approved"));
    }
}
