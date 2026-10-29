//! Layer control of the 2D situation map: which drawn layers are hidden, the
//! compact popover in the map toolbar, and the per-viewer memory of the choice.
//! Presentation only: hiding a layer never changes the world, and clicking the
//! map still returns the zone under the pointer whatever is hidden.
use yew::prelude::*;

/// Bit set of hidden layers; `0` shows everything.
pub type Hidden = u16;

/// Fuel/vegetation raster; when hidden the map has a plain light background.
pub const BASE: Hidden = 1;
pub const GRID: Hidden = 1 << 1;
pub const FOG: Hidden = 1 << 2;
pub const ROADS: Hidden = 1 << 3;
pub const BUILDINGS: Hidden = 1 << 4;
pub const LABELS: Hidden = 1 << 5;
/// Confirmed fire: burning and burnt cells.
pub const FIRE: Hidden = 1 << 6;
/// Cells reached only by some ensemble runs (uncertainty halo).
pub const HALO: Hidden = 1 << 7;
/// Reported fire locations and unverified estimates.
pub const ESTIMATES: Hidden = 1 << 8;
pub const FORECAST: Hidden = 1 << 9;
/// Units, deployments, drones and live drone zones.
pub const UNITS: Hidden = 1 << 10;
/// Fire lines, cleared cells, water drops and wet cells.
pub const WORKS: Hidden = 1 << 11;
/// Road closures/cuts and warned zones.
pub const CLOSURES: Hidden = 1 << 12;
/// Zone codes: column/row axes along the edges and per-cell codes when zoomed.
pub const CODES: Hidden = 1 << 13;

/// Layers in menu order: `(bit, stable storage name, i18n key)`.
pub const LAYERS: [(Hidden, &str, &str); 14] = [
    (BASE, "base", "l_base"),
    (GRID, "grid", "l_grid"),
    (CODES, "codes", "l_codes"),
    (FOG, "fog", "l_fog"),
    (ROADS, "roads", "l_roads"),
    (BUILDINGS, "buildings", "l_buildings"),
    (LABELS, "labels", "l_labels"),
    (FIRE, "fire", "l_fire"),
    (HALO, "halo", "l_halo"),
    (ESTIMATES, "estimates", "l_estimates"),
    (FORECAST, "forecast", "l_forecast"),
    (UNITS, "units", "l_units"),
    (WORKS, "works", "l_works"),
    (CLOSURES, "closures", "l_closures"),
];

/// Menu groups: `(i18n key, layers)`.
pub const GROUPS: [(&str, &[Hidden]); 4] = [
    ("lg_background", &[BASE, GRID, CODES, FOG]),
    ("lg_territory", &[ROADS, BUILDINGS, LABELS]),
    ("lg_fire", &[FIRE, HALO, ESTIMATES, FORECAST]),
    ("lg_operations", &[UNITS, WORKS, CLOSURES]),
];

/// Plain light neutral background used when the base layer is off.
pub const PLAIN_BG: [u8; 3] = [246, 245, 241];

/// Whether a fire raster cell code (`sim::digest` `cell_code`) is drawn.
/// 1 burning, 2 burnt, 3 fire line/cleared, 4 water/retardant, 5 possible (halo).
pub fn fire_code_shown(code: u8, hidden: Hidden) -> bool {
    let bit = match code {
        1 | 2 => FIRE,
        3 | 4 => WORKS,
        5 => HALO,
        _ => return false,
    };
    hidden & bit == 0
}

/// Stable text form: the names of the hidden layers, comma separated.
pub fn encode(hidden: Hidden) -> String {
    LAYERS
        .iter()
        .filter(|(bit, ..)| hidden & bit != 0)
        .map(|(_, name, _)| *name)
        .collect::<Vec<_>>()
        .join(",")
}

/// Inverse of [`encode`]; unknown names are ignored.
pub fn decode(s: &str) -> Hidden {
    s.split(',')
        .filter_map(|name| LAYERS.iter().find(|(_, n, _)| *n == name.trim()))
        .fold(0, |acc, (bit, ..)| acc | bit)
}

/// Storage key per map context: the two player maps (tactical board and
/// Rispondi composer) share one choice; EXCON maps keep their own.
pub fn storage_key(map_id: &str) -> String {
    let context = match map_id {
        "player-map" | "work-map" => "player",
        other => other,
    };
    format!("ttx.map_layers.{context}")
}

fn storage() -> Option<web_sys::Storage> {
    // `local_storage` returns Err when storage is blocked (private mode, policy).
    web_sys::window()?.local_storage().ok().flatten()
}

/// The remembered choice for this map, or everything visible.
pub fn load(map_id: &str) -> Hidden {
    storage()
        .and_then(|s| s.get_item(&storage_key(map_id)).ok().flatten())
        .map(|v| decode(&v))
        .unwrap_or(0)
}

/// Remember the choice; failures (quota, blocked storage) are ignored.
pub fn save(map_id: &str, hidden: Hidden) {
    if let Some(s) = storage() {
        let _ = s.set_item(&storage_key(map_id), &encode(hidden));
    }
}

/// Label of a layer-control string; Italian and English in the main catalogue,
/// a few more languages here, English otherwise.
pub fn label(lang: &str, key: &'static str) -> &'static str {
    lookup(lang, key).unwrap_or_else(|| crate::i18n::t(lang, key))
}

const KEYS: [&str; 11] = [
    "l_base",
    "l_codes",
    "l_halo",
    "l_estimates",
    "l_works",
    "l_closures",
    "lg_background",
    "lg_territory",
    "lg_fire",
    "lg_operations",
    "l_show_all",
];
const FR: [&str; 11] = [
    "Fond (végétation)",
    "Codes de zone",
    "Halo d’incertitude",
    "Signalements et estimations",
    "Lignes d’arrêt et largages",
    "Fermetures et alertes",
    "Fond",
    "Territoire",
    "Incendie",
    "Opérations",
    "Tout afficher",
];
const DE: [&str; 11] = [
    "Grundkarte (Vegetation)",
    "Zonencodes",
    "Unsicherheitssaum",
    "Meldungen und Schätzungen",
    "Brandschneisen und Abwürfe",
    "Sperrungen und Warnungen",
    "Hintergrund",
    "Gelände",
    "Feuer",
    "Einsatz",
    "Alles zeigen",
];
const ES: [&str; 11] = [
    "Base (vegetación)",
    "Códigos de zona",
    "Halo de incertidumbre",
    "Avisos y estimaciones",
    "Cortafuegos y descargas",
    "Cierres y alertas",
    "Fondo",
    "Territorio",
    "Incendio",
    "Operaciones",
    "Mostrar todo",
];
fn lookup(lang: &str, key: &str) -> Option<&'static str> {
    let table = match lang {
        "fr" => &FR,
        "de" => &DE,
        "es" => &ES,
        _ => return None,
    };
    KEYS.iter().position(|k| *k == key).map(|i| table[i])
}

#[derive(Properties, PartialEq)]
pub struct LayerControlProps {
    pub hidden: Hidden,
    pub onchange: Callback<Hidden>,
    pub lang: AttrValue,
}

/// Toolbar button with a small popover of grouped checkboxes.
#[function_component(LayerControl)]
pub fn layer_control(props: &LayerControlProps) -> Html {
    let lang = props.lang.as_str();
    let t = |k: &'static str| label(lang, k);
    let hidden = props.hidden;
    let row = |bit: Hidden| {
        let key = LAYERS.iter().find(|(b, ..)| *b == bit).map(|l| l.2).unwrap_or("");
        let onchange = props.onchange.reform(move |_: Event| hidden ^ bit);
        html! {<label data-layer={LAYERS.iter().find(|(b, ..)| *b == bit).map(|l| l.1).unwrap_or("")}>
            <input type="checkbox" checked={hidden & bit == 0} {onchange}/>{t(key)}</label>}
    };
    let show_all = props.onchange.reform(|_: MouseEvent| 0);
    html! {<details class={classes!("map-layers", (hidden != 0).then_some("filtered"))}>
        <summary title={t("map_layers")} aria-label={t("map_layers")}>{"☰"}</summary>
        <div role="group" aria-label={t("map_layers")}>
            {for GROUPS.iter().map(|(gkey, bits)| html!{<fieldset><legend>{t(gkey)}</legend>{for bits.iter().map(|b| row(*b))}</fieldset>})}
            <button type="button" class="map-layers-all" onclick={show_all} disabled={hidden == 0}>{t("l_show_all")}</button>
        </div>
    </details>}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trip_and_ignore_unknown_names() {
        assert_eq!(encode(0), "");
        assert_eq!(decode(""), 0);
        let h = BASE | HALO | CLOSURES;
        assert_eq!(encode(h), "base,halo,closures");
        assert_eq!(decode(&encode(h)), h);
        assert_eq!(decode("base, nonsense ,fog"), BASE | FOG);
        let all = LAYERS.iter().fold(0, |a, l| a | l.0);
        assert_eq!(decode(&encode(all)), all);
    }

    #[test]
    fn layers_are_distinct_and_each_in_exactly_one_group() {
        for (i, a) in LAYERS.iter().enumerate() {
            assert_eq!(a.0.count_ones(), 1);
            assert!(LAYERS[i + 1..].iter().all(|b| b.0 != a.0 && b.1 != a.1));
            let groups = GROUPS.iter().filter(|(_, bits)| bits.contains(&a.0)).count();
            assert_eq!(groups, 1, "{} in {groups} groups", a.1);
        }
    }

    #[test]
    fn fire_codes_follow_their_layer() {
        assert!(fire_code_shown(1, BASE) && fire_code_shown(5, BASE));
        assert!(!fire_code_shown(1, FIRE) && !fire_code_shown(2, FIRE));
        assert!(fire_code_shown(5, FIRE) && !fire_code_shown(5, HALO));
        assert!(!fire_code_shown(3, WORKS) && !fire_code_shown(4, WORKS));
        assert!(!fire_code_shown(0, 0));
    }

    #[test]
    fn player_maps_share_a_context_excon_maps_do_not() {
        assert_eq!(storage_key("player-map"), storage_key("work-map"));
        assert_ne!(storage_key("sit-map-canvas"), storage_key("ctl-modal-map"));
    }

    #[test]
    fn every_label_is_translated_in_it_and_en() {
        for key in LAYERS.iter().map(|l| l.2).chain(GROUPS.iter().map(|g| g.0)).chain(["l_show_all", "map_layers"]) {
            for lang in ["it", "en", "fr", "de", "es", "uk"] {
                let s = label(lang, key);
                assert!(!s.is_empty() && s != key, "{lang}:{key} -> {s}");
            }
            assert_ne!(label("it", key), label("en", key), "{key} not translated in it");
        }
    }
}
