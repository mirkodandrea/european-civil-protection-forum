//! Place-validation hints of the action form for the languages beyond Italian/English.
//! `i18n::t` falls back to it after `i18n_lang` and `i18n_function`.
const KEYS: [&str; 3] = ["place_switched", "place_is_building", "place_not_zone"];
const FR: [&str; 3] = [
    "« {} » est un bâtiment : l’action est passée à « {} ».",
    "« {} » est un bâtiment, pas une zone : choisissez l’action sur un bâtiment ou saisissez un code de zone (par ex. F4).",
    "« {} » n’est pas un code de zone (par ex. F4) ni un lieu connu : choisissez dans la liste.",
];
const DE: [&str; 3] = [
    "„{}“ ist ein Gebäude: Die Aktion wurde auf „{}“ umgestellt.",
    "„{}“ ist ein Gebäude, keine Zone: Wählen Sie die Gebäude-Aktion oder geben Sie einen Zonencode ein (z. B. F4).",
    "„{}“ ist weder ein Zonencode (z. B. F4) noch ein bekannter Ort: Wählen Sie aus der Liste.",
];
const ES: [&str; 3] = [
    "«{}» es un edificio: la acción ha cambiado a «{}».",
    "«{}» es un edificio, no una zona: elija la acción de edificio o escriba un código de zona (p. ej. F4).",
    "«{}» no es un código de zona (p. ej. F4) ni un lugar conocido: elija de la lista.",
];
const PT: [&str; 3] = [
    "«{}» é um edifício: a ação mudou para «{}».",
    "«{}» é um edifício, não uma zona: escolha a ação de edifício ou escreva um código de zona (p. ex. F4).",
    "«{}» não é um código de zona (p. ex. F4) nem um local conhecido: escolha da lista.",
];
const RO: [&str; 3] = [
    "„{}” este o clădire: acțiunea a fost schimbată în „{}”.",
    "„{}” este o clădire, nu o zonă: alegeți acțiunea pentru clădire sau scrieți un cod de zonă (de ex. F4).",
    "„{}” nu este un cod de zonă (de ex. F4) și nici un loc cunoscut: alegeți din listă.",
];
const EL: [&str; 3] = [
    "Το «{}» είναι κτίριο: η ενέργεια άλλαξε σε «{}».",
    "Το «{}» είναι κτίριο και όχι ζώνη: επιλέξτε την ενέργεια για κτίριο ή γράψτε κωδικό ζώνης (π.χ. F4).",
    "Το «{}» δεν είναι κωδικός ζώνης (π.χ. F4) ούτε γνωστός τόπος: επιλέξτε από τη λίστα.",
];
const UK: [&str; 3] = [
    "«{}» — це будівля: дію змінено на «{}».",
    "«{}» — це будівля, а не зона: оберіть дію для будівлі або введіть код зони (напр. F4).",
    "«{}» не є кодом зони (напр. F4) і не є відомим місцем: оберіть зі списку.",
];
const HY: [&str; 3] = [
    "«{}»-ը շենք է. գործողությունը փոխվեց «{}»-ի։",
    "«{}»-ը շենք է, ոչ թե գոտի. ընտրեք շենքի գործողությունը կամ գրեք գոտու կոդ (օր. F4)։",
    "«{}»-ը ոչ գոտու կոդ է (օր. F4), ոչ էլ հայտնի վայր. ընտրեք ցանկից։",
];
const KA: [&str; 3] = [
    "„{}“ შენობაა: მოქმედება შეიცვალა „{}“-ზე.",
    "„{}“ შენობაა და არა ზონა: აირჩიეთ შენობის მოქმედება ან ჩაწერეთ ზონის კოდი (მაგ. F4).",
    "„{}“ არც ზონის კოდია (მაგ. F4), არც ცნობილი ადგილი: აირჩიეთ სიიდან.",
];
const AZ: [&str; 3] = [
    "“{}” binadır: əməliyyat “{}” olaraq dəyişdirildi.",
    "“{}” binadır, zona deyil: bina əməliyyatını seçin və ya zona kodu yazın (məs. F4).",
    "“{}” nə zona kodudur (məs. F4), nə də məlum yerdir: siyahıdan seçin.",
];

pub fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    let table: &[&str; 3] = match lang {
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
    };
    KEYS.iter().position(|k| *k == key).map(|i| table[i])
}
