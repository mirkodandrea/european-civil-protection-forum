//! Strings of the player's response form (basis of the response, PDF
//! attachment, who/what/how/where/when help) and of the EXCON view of them.
//! Own module so the catalogues in `i18n_lang` stay untouched; `i18n::t`
//! consults it for every player language, `i18n_excon::ex` for English.

pub const KEYS: [&str; 17] = [
    "step_basis",
    "basis_plan",
    "basis_law",
    "basis_judgment",
    "basis_hint",
    "doc_label",
    "doc_hint",
    "doc_choose",
    "doc_remove",
    "doc_off",
    "doc_too_big",
    "doc_not_pdf",
    "doc_read_fail",
    "step_narr",
    "narr_help",
    "narr_ph",
    "step_actions",
];

const IT: [&str; 17] = [
    "Su cosa si basa la risposta",
    "Piani",
    "Leggi",
    "Giudizi operativi",
    "Indica il fondamento della decisione: i piani di emergenza, le norme di legge oppure il tuo giudizio operativo.",
    "Documento PDF (facoltativo)",
    "Allega il piano o la norma a cui ti riferisci. Solo PDF, massimo 10 MB.",
    "Scegli un PDF",
    "Rimuovi il PDF",
    "Il PDF si può allegare solo se la risposta si basa su piani o leggi.",
    "Il PDF supera i 10 MB.",
    "Il file scelto non è un PDF.",
    "Impossibile leggere il file.",
    "Narrazione della decisione",
    "Descrivi il flusso decisionale operativo: chi agisce, cosa decide o fa, come lo fa (mezzi e modalità), dove (luogo o zona) e quando (tempi e durata).",
    "Chi: …\nCosa: …\nCome: …\nDove: …\nQuando: …",
    "Azioni sulla simulazione",
];
const EN: [&str; 17] = [
    "What the response is based on",
    "Plans",
    "Laws",
    "Operational judgement",
    "State what the decision rests on: emergency plans, legal provisions, or your own operational judgement.",
    "PDF document (optional)",
    "Attach the plan or law you refer to. PDF only, up to 10 MB.",
    "Choose a PDF",
    "Remove the PDF",
    "A PDF can be attached only when the response is based on plans or laws.",
    "The PDF is larger than 10 MB.",
    "The chosen file is not a PDF.",
    "The file could not be read.",
    "Narrative of the decision",
    "Describe the operational decision flow: who acts, what is decided or done, how (means and manner), where (place or zone) and when (timing and duration).",
    "Who: …\nWhat: …\nHow: …\nWhere: …\nWhen: …",
    "Actions on the simulation",
];
const FR: [&str; 17] = [
    "Sur quoi repose la réponse",
    "Plans",
    "Lois",
    "Jugement opérationnel",
    "Indiquez le fondement de la décision : plans d’urgence, textes de loi ou votre jugement opérationnel.",
    "Document PDF (facultatif)",
    "Joignez le plan ou la norme auxquels vous vous référez. PDF uniquement, 10 Mo maximum.",
    "Choisir un PDF",
    "Retirer le PDF",
    "Un PDF ne peut être joint que si la réponse repose sur des plans ou des lois.",
    "Le PDF dépasse 10 Mo.",
    "Le fichier choisi n’est pas un PDF.",
    "Impossible de lire le fichier.",
    "Récit de la décision",
    "Décrivez le déroulement de la décision opérationnelle : qui agit, ce qui est décidé ou fait, comment (moyens et modalités), où (lieu ou zone) et quand (délais et durée).",
    "Qui : …\nQuoi : …\nComment : …\nOù : …\nQuand : …",
    "Actions sur la simulation",
];
const DE: [&str; 17] = [
    "Worauf die Antwort beruht",
    "Pläne",
    "Gesetze",
    "Operatives Urteil",
    "Geben Sie die Grundlage der Entscheidung an: Einsatzpläne, Rechtsvorschriften oder Ihr eigenes operatives Urteil.",
    "PDF-Dokument (optional)",
    "Fügen Sie den Plan oder die Rechtsnorm bei, auf die Sie sich beziehen. Nur PDF, höchstens 10 MB.",
    "PDF auswählen",
    "PDF entfernen",
    "Ein PDF kann nur angehängt werden, wenn die Antwort auf Plänen oder Gesetzen beruht.",
    "Das PDF ist größer als 10 MB.",
    "Die gewählte Datei ist kein PDF.",
    "Die Datei konnte nicht gelesen werden.",
    "Beschreibung der Entscheidung",
    "Beschreiben Sie den operativen Entscheidungsablauf: wer handelt, was entschieden oder getan wird, wie (Mittel und Vorgehen), wo (Ort oder Zone) und wann (Zeitpunkt und Dauer).",
    "Wer: …\nWas: …\nWie: …\nWo: …\nWann: …",
    "Maßnahmen in der Simulation",
];
const ES: [&str; 17] = [
    "En qué se basa la respuesta",
    "Planes",
    "Leyes",
    "Juicio operativo",
    "Indique el fundamento de la decisión: planes de emergencia, normas legales o su propio juicio operativo.",
    "Documento PDF (opcional)",
    "Adjunte el plan o la norma a la que se refiere. Solo PDF, máximo 10 MB.",
    "Elegir un PDF",
    "Quitar el PDF",
    "Solo se puede adjuntar un PDF si la respuesta se basa en planes o leyes.",
    "El PDF supera los 10 MB.",
    "El archivo elegido no es un PDF.",
    "No se pudo leer el archivo.",
    "Narración de la decisión",
    "Describa el flujo de la decisión operativa: quién actúa, qué se decide o se hace, cómo (medios y modo), dónde (lugar o zona) y cuándo (plazos y duración).",
    "Quién: …\nQué: …\nCómo: …\nDónde: …\nCuándo: …",
    "Acciones en la simulación",
];
const PT: [&str; 17] = [
    "Em que se baseia a resposta",
    "Planos",
    "Leis",
    "Julgamento operacional",
    "Indique o fundamento da decisão: planos de emergência, normas legais ou o seu próprio julgamento operacional.",
    "Documento PDF (opcional)",
    "Anexe o plano ou a norma a que se refere. Apenas PDF, máximo de 10 MB.",
    "Escolher um PDF",
    "Remover o PDF",
    "Só é possível anexar um PDF se a resposta se basear em planos ou leis.",
    "O PDF excede 10 MB.",
    "O ficheiro escolhido não é um PDF.",
    "Não foi possível ler o ficheiro.",
    "Narração da decisão",
    "Descreva o fluxo da decisão operacional: quem age, o que se decide ou faz, como (meios e modo), onde (local ou zona) e quando (prazos e duração).",
    "Quem: …\nO quê: …\nComo: …\nOnde: …\nQuando: …",
    "Ações na simulação",
];
const RO: [&str; 17] = [
    "Pe ce se bazează răspunsul",
    "Planuri",
    "Legi",
    "Judecată operațională",
    "Indicați temeiul deciziei: planuri de urgență, norme legale sau propria judecată operațională.",
    "Document PDF (opțional)",
    "Atașați planul sau norma la care vă referiți. Doar PDF, maximum 10 MB.",
    "Alege un PDF",
    "Elimină PDF-ul",
    "Un PDF poate fi atașat doar dacă răspunsul se bazează pe planuri sau legi.",
    "PDF-ul depășește 10 MB.",
    "Fișierul ales nu este un PDF.",
    "Fișierul nu a putut fi citit.",
    "Descrierea deciziei",
    "Descrieți fluxul deciziei operaționale: cine acționează, ce se decide sau se face, cum (mijloace și mod), unde (loc sau zonă) și când (termene și durată).",
    "Cine: …\nCe: …\nCum: …\nUnde: …\nCând: …",
    "Acțiuni în simulare",
];
const EL: [&str; 17] = [
    "Στο τι βασίζεται η απάντηση",
    "Σχέδια",
    "Νόμοι",
    "Επιχειρησιακή κρίση",
    "Δηλώστε το έρεισμα της απόφασης: σχέδια έκτακτης ανάγκης, νομικές διατάξεις ή τη δική σας επιχειρησιακή κρίση.",
    "Έγγραφο PDF (προαιρετικό)",
    "Επισυνάψτε το σχέδιο ή τη διάταξη στην οποία αναφέρεστε. Μόνο PDF, έως 10 MB.",
    "Επιλογή PDF",
    "Αφαίρεση του PDF",
    "Μπορεί να επισυναφθεί PDF μόνο αν η απάντηση βασίζεται σε σχέδια ή νόμους.",
    "Το PDF υπερβαίνει τα 10 MB.",
    "Το αρχείο που επιλέξατε δεν είναι PDF.",
    "Δεν ήταν δυνατή η ανάγνωση του αρχείου.",
    "Αφήγηση της απόφασης",
    "Περιγράψτε τη ροή της επιχειρησιακής απόφασης: ποιος ενεργεί, τι αποφασίζεται ή γίνεται, πώς (μέσα και τρόπος), πού (τόπος ή ζώνη) και πότε (χρόνος και διάρκεια).",
    "Ποιος: …\nΤι: …\nΠώς: …\nΠού: …\nΠότε: …",
    "Ενέργειες στην προσομοίωση",
];
const UK: [&str; 17] = [
    "На чому ґрунтується відповідь",
    "Плани",
    "Закони",
    "Оперативна оцінка",
    "Вкажіть підставу рішення: плани реагування, правові норми або власну оперативну оцінку.",
    "Документ PDF (необов’язково)",
    "Додайте план або норму, на які ви посилаєтеся. Лише PDF, до 10 МБ.",
    "Обрати PDF",
    "Видалити PDF",
    "PDF можна додати лише тоді, коли відповідь ґрунтується на планах або законах.",
    "PDF перевищує 10 МБ.",
    "Обраний файл не є PDF.",
    "Не вдалося прочитати файл.",
    "Опис рішення",
    "Опишіть хід оперативного рішення: хто діє, що вирішується або робиться, як (засоби і спосіб), де (місце або зона) і коли (терміни та тривалість).",
    "Хто: …\nЩо: …\nЯк: …\nДе: …\nКоли: …",
    "Дії в симуляції",
];
const HY: [&str; 17] = [
    "Ինչի վրա է հիմնված պատասխանը",
    "Ծրագրեր",
    "Օրենքներ",
    "Գործառնական դատողություն",
    "Նշեք որոշման հիմքը՝ արտակարգ իրավիճակների ծրագրեր, իրավական նորմեր կամ ձեր սեփական գործառնական դատողությունը։",
    "PDF փաստաթուղթ (ըստ ցանկության)",
    "Կցեք ծրագիրը կամ նորմը, որին հղում եք անում։ Միայն PDF, առավելագույնը 10 ՄԲ։",
    "Ընտրել PDF",
    "Հեռացնել PDF-ը",
    "PDF կարելի է կցել միայն այն դեպքում, երբ պատասխանը հիմնված է ծրագրերի կամ օրենքների վրա։",
    "PDF-ը գերազանցում է 10 ՄԲ-ը։",
    "Ընտրված ֆայլը PDF չէ։",
    "Ֆայլը հնարավոր չէ կարդալ։",
    "Որոշման նկարագրություն",
    "Նկարագրեք գործառնական որոշման ընթացքը՝ ով է գործում, ինչ է որոշվում կամ արվում, ինչպես (միջոցներ և եղանակ), որտեղ (վայր կամ գոտի) և երբ (ժամկետներ և տևողություն)։",
    "Ո՞վ՝ …\nԻ՞նչ՝ …\nԻնչպե՞ս՝ …\nՈրտե՞ղ՝ …\nԵ՞րբ՝ …",
    "Գործողություններ սիմուլյացիայում",
];
const KA: [&str; 17] = [
    "რას ეფუძნება პასუხი",
    "გეგმები",
    "კანონები",
    "ოპერატიული შეფასება",
    "მიუთითეთ გადაწყვეტილების საფუძველი: საგანგებო გეგმები, სამართლებრივი ნორმები ან თქვენი ოპერატიული შეფასება.",
    "PDF დოკუმენტი (არასავალდებულო)",
    "დაურთეთ გეგმა ან ნორმა, რომელზეც დაყრდნობილი ხართ. მხოლოდ PDF, მაქსიმუმ 10 მბ.",
    "აირჩიეთ PDF",
    "PDF-ის წაშლა",
    "PDF-ის დართვა შესაძლებელია მხოლოდ მაშინ, როცა პასუხი ეფუძნება გეგმებს ან კანონებს.",
    "PDF აღემატება 10 მბ-ს.",
    "არჩეული ფაილი არ არის PDF.",
    "ფაილის წაკითხვა ვერ მოხერხდა.",
    "გადაწყვეტილების აღწერა",
    "აღწერეთ ოპერატიული გადაწყვეტილების მიმდინარეობა: ვინ მოქმედებს, რა წყდება ან კეთდება, როგორ (საშუალებები და ხერხი), სად (ადგილი ან ზონა) და როდის (ვადები და ხანგრძლივობა).",
    "ვინ: …\nრა: …\nროგორ: …\nსად: …\nროდის: …",
    "მოქმედებები სიმულაციაში",
];
const AZ: [&str; 17] = [
    "Cavab nəyə əsaslanır",
    "Planlar",
    "Qanunlar",
    "Əməliyyat qiymətləndirməsi",
    "Qərarın əsasını göstərin: fövqəladə hal planları, hüquqi normalar və ya öz əməliyyat qiymətləndirməniz.",
    "PDF sənəd (könüllü)",
    "İstinad etdiyiniz plan və ya normanı əlavə edin. Yalnız PDF, ən çoxu 10 MB.",
    "PDF seçin",
    "PDF-i sil",
    "PDF yalnız cavab planlara və ya qanunlara əsaslandıqda əlavə edilə bilər.",
    "PDF 10 MB-dan böyükdür.",
    "Seçilən fayl PDF deyil.",
    "Fayl oxuna bilmədi.",
    "Qərarın təsviri",
    "Əməliyyat qərarının gedişini təsvir edin: kim fəaliyyət göstərir, nə qərara alınır və ya edilir, necə (vasitələr və üsul), harada (yer və ya zona) və nə vaxt (müddət və davamiyyət).",
    "Kim: …\nNə: …\nNecə: …\nHarada: …\nNə vaxt: …",
    "Simulyasiyadakı hərəkətlər",
];

fn table(lang: &str) -> Option<&'static [&'static str; 17]> {
    Some(match lang {
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
        _ => return None,
    })
}

pub fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    let i = KEYS.iter().position(|k| *k == key)?;
    table(lang).map(|t| t[i])
}

/// English for the Italian EXCON strings of the reply view.
pub fn ex_en(it: &str) -> Option<&'static str> {
    Some(match it {
        "Base della risposta" => "Basis of the response",
        "Piani" => "Plans",
        "Leggi" => "Laws",
        "Giudizi operativi" => "Operational judgement",
        "Documento PDF" => "PDF document",
        "Apri PDF" => "Open PDF",
        "Chiudi PDF" => "Close PDF",
        "Apri in una scheda" => "Open in a tab",
        "Scarica" => "Download",
        "Caricamento del PDF…" => "Loading the PDF…",
        "Impossibile mostrare il PDF qui (visualizzatore non raggiungibile?). Aprilo in una scheda o scaricalo." => "The PDF cannot be shown here (viewer unreachable?). Open it in a tab or download it.",
        "Documento non disponibile." => "Document not available.",
        "Risposta da gestire" => "Reply to handle",
        "Nessuna risposta da gestire." => "No reply to handle.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_language_has_every_key() {
        for l in ["it", "en", "fr", "de", "es", "pt", "ro", "el", "uk", "hy", "ka", "az"] {
            for k in KEYS {
                let v = lookup(l, k).unwrap_or_else(|| panic!("{l}/{k}"));
                assert!(!v.trim().is_empty(), "{l}/{k}");
            }
            // The five-line placeholder keeps one line per question.
            assert_eq!(lookup(l, "narr_ph").unwrap().lines().count(), 5, "{l}");
            for k in ["doc_hint", "doc_too_big"] {
                assert!(lookup(l, k).unwrap().contains("10"), "{l}/{k}");
            }
        }
        assert!(lookup("xx", "narr_ph").is_none());
    }
}
