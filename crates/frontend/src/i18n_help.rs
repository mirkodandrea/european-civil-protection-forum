//! Plain-language wording for exercise messages (the EXCON term "inject") and
//! for the automatic translation badge, in the languages beyond Italian/English.
//! Takes precedence over `i18n_lang`, whose generated texts still say "inject".
const KEYS: [&str; 9] = [
    "inject",
    "injects_replies",
    "reply_to",
    "reply_inject_title",
    "private_inject",
    "empty_hint",
    "literal_translation_short",
    "literal_translation_hint",
    "inject_info",
];

/// Languages this module covers (everything the player UI offers beyond it/en).
pub const LANGS: [&str; 10] = ["fr", "de", "es", "pt", "ro", "el", "uk", "hy", "ka", "az"];

fn table(lang: &str) -> Option<&'static [&'static str; 9]> {
    Some(match lang {
        "fr" => &[
            "MESSAGE",
            "Messages et réponses",
            "Répondre au message (facultatif)",
            "Répondre au message",
            " · MESSAGE PRIVÉ",
            "Les messages reçus et l’état de vos réponses apparaîtront ici. Vous pouvez aussi proposer une action indépendante.",
            "Traduit automatiquement",
            "Les messages sont traduits automatiquement, mot à mot, dans la langue du destinataire. Le texte original reste celui de l’expéditeur.",
            "Les messages sont des informations que vous recevez pendant l’exercice. Répondez par les conversations avec les participants ou par les propositions opérationnelles depuis la carte.",
        ],
        "de" => &[
            "NACHRICHT",
            "Nachrichten und Antworten",
            "Auf Nachricht antworten (optional)",
            "Auf die Nachricht antworten",
            " · PRIVATE NACHRICHT",
            "Erhaltene Nachrichten und der Status Ihrer Antworten erscheinen hier. Sie können auch eine eigenständige Maßnahme vorschlagen.",
            "Automatisch übersetzt",
            "Nachrichten werden automatisch, Wort für Wort, in die Sprache der Empfängerin oder des Empfängers übersetzt. Der Originaltext bleibt der des Absenders.",
            "Nachrichten sind Informationen, die Sie während der Übung erhalten. Antworten Sie über Gespräche mit Teilnehmenden oder über operative Vorschläge auf der Karte.",
        ],
        "es" => &[
            "MENSAJE",
            "Mensajes y respuestas",
            "Responder al mensaje (opcional)",
            "Responder al mensaje",
            " · MENSAJE PRIVADO",
            "Aquí aparecerán los mensajes recibidos y el estado de sus respuestas. También puede proponer una acción independiente.",
            "Traducido automáticamente",
            "Los mensajes se traducen automáticamente, palabra por palabra, al idioma del destinatario. El texto original sigue siendo el que escribió el remitente.",
            "Los mensajes son información que recibe durante el ejercicio. Responda mediante las conversaciones con los participantes o las propuestas operativas desde el mapa.",
        ],
        "pt" => &[
            "MENSAGEM",
            "Mensagens e respostas",
            "Responder à mensagem (opcional)",
            "Responder à mensagem",
            " · MENSAGEM PRIVADA",
            "As mensagens recebidas e o estado das suas respostas aparecerão aqui. Também pode propor uma ação independente.",
            "Traduzido automaticamente",
            "As mensagens são traduzidas automaticamente, palavra por palavra, para o idioma do destinatário. O texto original continua a ser o escrito pelo remetente.",
            "As mensagens são informações que recebe durante o exercício. Responda através das conversas com os participantes ou das propostas operacionais no mapa.",
        ],
        "ro" => &[
            "MESAJ",
            "Mesaje și răspunsuri",
            "Răspunde la mesaj (opțional)",
            "Răspunde la mesaj",
            " · MESAJ PRIVAT",
            "Mesajele primite și starea răspunsurilor dvs. vor apărea aici. Puteți propune și o acțiune independentă.",
            "Tradus automat",
            "Mesajele sunt traduse automat, cuvânt cu cuvânt, în limba destinatarului. Textul original rămâne cel scris de expeditor.",
            "Mesajele sunt informații pe care le primiți în timpul exercițiului. Răspundeți prin conversațiile cu participanții sau prin propunerile operative de pe hartă.",
        ],
        "el" => &[
            "ΜΗΝΥΜΑ",
            "Μηνύματα και απαντήσεις",
            "Απάντηση στο μήνυμα (προαιρετικά)",
            "Απάντηση στο μήνυμα",
            " · ΙΔΙΩΤΙΚΟ ΜΗΝΥΜΑ",
            "Τα μηνύματα που λαμβάνετε και η κατάσταση των απαντήσεών σας θα εμφανίζονται εδώ. Μπορείτε επίσης να προτείνετε μια ανεξάρτητη ενέργεια.",
            "Αυτόματη μετάφραση",
            "Τα μηνύματα μεταφράζονται αυτόματα, λέξη προς λέξη, στη γλώσσα του παραλήπτη. Το πρωτότυπο κείμενο παραμένει αυτό που έγραψε ο αποστολέας.",
            "Τα μηνύματα είναι πληροφορίες που λαμβάνετε κατά την άσκηση. Απαντήστε μέσω των συνομιλιών με τους συμμετέχοντες ή μέσω επιχειρησιακών προτάσεων από τον χάρτη.",
        ],
        "uk" => &[
            "ПОВІДОМЛЕННЯ",
            "Повідомлення та відповіді",
            "Відповісти на повідомлення (необов’язково)",
            "Відповісти на повідомлення",
            " · ПРИВАТНЕ ПОВІДОМЛЕННЯ",
            "Тут з’являтимуться отримані повідомлення та стан ваших відповідей. Також можна запропонувати незалежну дію.",
            "Перекладено автоматично",
            "Повідомлення автоматично перекладаються слово в слово мовою одержувача. Оригінальний текст залишається таким, як його написав відправник.",
            "Повідомлення — це інформація, яку ви отримуєте під час навчання. Відповідайте через розмови з учасниками або через оперативні пропозиції на карті.",
        ],
        "hy" => &[
            "ՀԱՂՈՐԴԱԳՐՈՒԹՅՈՒՆ",
            "Հաղորդագրություններ և պատասխաններ",
            "Պատասխանել հաղորդագրությանը (ըստ ցանկության)",
            "Պատասխանել հաղորդագրությանը",
            " · ԱՆՁՆԱԿԱՆ ՀԱՂՈՐԴԱԳՐՈՒԹՅՈՒՆ",
            "Ստացված հաղորդագրությունները և ձեր պատասխանների կարգավիճակը կհայտնվեն այստեղ։ Կարող եք նաև առաջարկել անկախ գործողություն։",
            "Թարգմանված է ավտոմատ",
            "Հաղորդագրությունները ավտոմատ կերպով թարգմանվում են բառ առ բառ ստացողի լեզվով։ Բնօրինակ տեքստը մնում է ուղարկողի գրածը։",
            "Հաղորդագրությունները տեղեկություններ են, որոնք ստանում եք վարժանքի ընթացքում։ Պատասխանեք մասնակիցների հետ զրույցների կամ քարտեզից օպերատիվ առաջարկների միջոցով։",
        ],
        "ka" => &[
            "შეტყობინება",
            "შეტყობინებები და პასუხები",
            "შეტყობინებაზე პასუხი (არასავალდებულო)",
            "შეტყობინებაზე პასუხი",
            " · პირადი შეტყობინება",
            "მიღებული შეტყობინებები და თქვენი პასუხების სტატუსი აქ გამოჩნდება. შეგიძლიათ დამოუკიდებელი მოქმედებაც შესთავაზოთ.",
            "ავტომატურად ნათარგმნი",
            "შეტყობინებები ავტომატურად ითარგმნება სიტყვასიტყვით მიმღების ენაზე. ორიგინალური ტექსტი რჩება ისეთი, როგორიც გამგზავნმა დაწერა.",
            "შეტყობინებები არის ინფორმაცია, რომელსაც ვარჯიშის დროს იღებთ. უპასუხეთ მონაწილეებთან საუბრების ან რუკიდან სამოქმედო წინადადებების მეშვეობით.",
        ],
        "az" => &[
            "MESAJ",
            "Mesajlar və cavablar",
            "Mesaja cavab ver (könüllü)",
            "Mesaja cavab ver",
            " · ŞƏXSİ MESAJ",
            "Alınan mesajlar və cavablarınızın statusu burada görünəcək. Müstəqil tədbir də təklif edə bilərsiniz.",
            "Avtomatik tərcümə olunub",
            "Mesajlar avtomatik olaraq, sözbəsöz, alıcının dilinə tərcümə olunur. Orijinal mətn göndərənin yazdığı kimi qalır.",
            "Mesajlar təlim zamanı aldığınız məlumatlardır. İştirakçılarla söhbətlər və ya xəritədən əməliyyat təklifləri vasitəsilə cavab verin.",
        ],
        _ => return None,
    })
}

pub fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    let table = table(lang)?;
    KEYS.iter().position(|k| *k == key).map(|i| table[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_language_covers_every_key_without_the_jargon() {
        for lang in LANGS {
            for key in KEYS {
                let text = lookup(lang, key).unwrap_or_else(|| panic!("{lang}/{key} missing"));
                assert!(!text.trim().is_empty(), "{lang}/{key} empty");
                assert!(!text.to_lowercase().contains("inject"), "{lang}/{key} still says inject");
            }
        }
        assert!(lookup("it", "inject").is_none());
    }

    #[test]
    fn player_languages_are_all_handled() {
        for (id, _) in crate::api::LANGUAGES {
            if !["en", "it"].contains(id) {
                assert!(LANGS.contains(id), "{id} has no plain-language wording");
            }
        }
    }
}
