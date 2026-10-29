//! Map legend with shapes and textures next to colours (colour-blind safe).
//! Strings are Italian/English pairs; other player languages come from `i18n_more`.
//! The glyphs and textures here mirror what `map.rs` draws.
use crate::i18n::pair;
use yew::prelude::*;

pub fn lg(lang: &str, key: &'static str) -> &'static str {
    // The EXCON console passes "it" and may read English.
    let l = match lang {
        "it" if crate::i18n::is_en() => "en",
        other => other,
    };
    match key {
        "fire" => pair(l, "Fronte attivo", "Active fire front"),
        "burnt" => pair(l, "Area bruciata", "Burnt area"),
        "possible" => pair(l, "Possibile (ensemble)", "Possible (ensemble)"),
        "water" => pair(l, "Acqua / ritardante", "Water / retardant"),
        "line" => pair(l, "Linea di contenimento", "Containment line"),
        "forecast" => pair(l, "Previsione", "Forecast"),
        "cut" => pair(l, "Strada tagliata dal fuoco", "Road cut by fire"),
        "police_closure" => pair(l, "Chiusura polizia", "Police closure"),
        "threatened" => pair(l, "Edificio minacciato", "Threatened building"),
        "damaged" => pair(l, "Danneggiato", "Damaged"),
        "destroyed" => pair(l, "Distrutto", "Destroyed"),
        "evacuating" => pair(l, "In evacuazione", "Evacuating"),
        "evacuated" => pair(l, "Evacuato", "Evacuated"),
        "unit" => pair(l, "Risorsa (rettangolo)", "Resource (rectangle)"),
        "target" => pair(l, "Destinazione missione", "Mission target"),
        "reported" => pair(l, "Incendio segnalato", "Reported fire"),
        "title" => pair(l, "Legenda", "Legend"),
        _ => "",
    }
}

#[derive(Properties, PartialEq)]
pub struct LegendProps {
    pub lang: AttrValue,
}

#[function_component(MapLegend)]
pub fn map_legend(props: &LegendProps) -> Html {
    let l = props.lang.as_str();
    let sw = |class: &'static str, key: &'static str| {
        html! {<li><span class={classes!("lg-sw", class)} aria-hidden="true"></span>{lg(l, key)}</li>}
    };
    let gl = |glyph: &'static str, class: &'static str, key: &'static str| {
        html! {<li><span class={classes!("lg-gl", class)} aria-hidden="true">{glyph}</span>{lg(l, key)}</li>}
    };
    html! {
        <ul class="map-legend map-legend-sym" aria-label={lg(l, "title")}>
            {sw("lg-fire", "fire")}{sw("lg-burnt", "burnt")}{sw("lg-possible", "possible")}{sw("lg-water", "water")}
            {sw("lg-line", "line")}{sw("lg-forecast", "forecast")}{sw("lg-cut", "cut")}{sw("lg-closure", "police_closure")}
            {gl("▲", "g-threat", "threatened")}{gl("◆", "g-dmg", "damaged")}{gl("✕", "g-dest", "destroyed")}
            {gl("➜", "g-evac", "evacuating")}{gl("○", "g-done", "evacuated")}
            {sw("lg-unit", "unit")}{gl("◇", "g-target", "target")}{gl("◯", "g-rep", "reported")}
        </ul>
    }
}
