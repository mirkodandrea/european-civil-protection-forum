//! Player-UI strings before the exercise starts (language confirmed, waiting
//! for the EXCON), in every player language.
const KEYS: [&str; 2] = ["await_start", "await_start_note"];
const IT: [&str; 2] = ["In attesa dell’avvio dell’esercitazione", "La lingua è confermata. L’esercitazione inizierà quando il controllo dell’esercitazione la avvierà."];
const EN: [&str; 2] = ["Waiting for the exercise to start", "Your language is confirmed. The exercise begins when exercise control starts it."];
const FR: [&str; 2] = ["En attente du début de l’exercice", "Votre langue est confirmée. L’exercice commencera lorsque le contrôle de l’exercice le lancera."];
const DE: [&str; 2] = ["Warten auf den Beginn der Übung", "Ihre Sprache ist bestätigt. Die Übung beginnt, sobald die Übungsleitung sie startet."];
const ES: [&str; 2] = ["Esperando el inicio del ejercicio", "Su idioma está confirmado. El ejercicio comenzará cuando el control del ejercicio lo inicie."];
const PT: [&str; 2] = ["A aguardar o início do exercício", "O seu idioma está confirmado. O exercício começa quando o controlo do exercício o iniciar."];
const RO: [&str; 2] = ["Se așteaptă începerea exercițiului", "Limba dumneavoastră este confirmată. Exercițiul începe când conducerea exercițiului îl pornește."];
const EL: [&str; 2] = ["Αναμονή για την έναρξη της άσκησης", "Η γλώσσα σας επιβεβαιώθηκε. Η άσκηση ξεκινά όταν τη θέσει σε λειτουργία ο έλεγχος της άσκησης."];
const UK: [&str; 2] = ["Очікування початку навчань", "Вашу мову підтверджено. Навчання розпочнуться, коли керівництво навчань їх запустить."];
const HY: [&str; 2] = ["Սպասում ենք զորավարժության մեկնարկին", "Ձեր լեզուն հաստատված է։ Զորավարժությունը կսկսվի, երբ զորավարժության ղեկավարությունը այն մեկնարկի։"];
const KA: [&str; 2] = ["სწავლების დაწყების მოლოდინი", "თქვენი ენა დადასტურებულია. სწავლება დაიწყება, როცა სწავლების ხელმძღვანელობა მას გაუშვებს."];
const AZ: [&str; 2] = ["Təlimin başlamasını gözləyirik", "Diliniz təsdiqləndi. Təlim, təlim rəhbərliyi onu başlatdıqda başlayacaq."];

pub fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    let table: &[&str; 2] = match lang {
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
    }
}
