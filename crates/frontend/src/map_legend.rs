//! Map legend with shapes and textures next to colours (colour-blind safe).
//! Self-contained strings (Italian / English; other languages fall back to English).
//! The glyphs and textures here mirror what `map.rs` draws.
use yew::prelude::*;

pub fn lg(lang: &str, key: &'static str) -> &'static str {
    let en = lang != "it" || crate::i18n::is_en();
    match (key, en) {
        ("fire", false) => "Fronte attivo",
        ("fire", true) => "Active fire front",
        ("burnt", false) => "Area bruciata",
        ("burnt", true) => "Burnt area",
        ("possible", false) => "Possibile (ensemble)",
        ("possible", true) => "Possible (ensemble)",
        ("water", false) => "Acqua / ritardante",
        ("water", true) => "Water / retardant",
        ("line", false) => "Linea di contenimento",
        ("line", true) => "Containment line",
        ("forecast", false) => "Previsione",
        ("forecast", true) => "Forecast",
        ("cut", false) => "Strada tagliata dal fuoco",
        ("cut", true) => "Road cut by fire",
        ("police_closure", false) => "Chiusura polizia",
        ("police_closure", true) => "Police closure",
        ("threatened", false) => "Edificio minacciato",
        ("threatened", true) => "Threatened building",
        ("damaged", false) => "Danneggiato",
        ("damaged", true) => "Damaged",
        ("destroyed", false) => "Distrutto",
        ("destroyed", true) => "Destroyed",
        ("evacuating", false) => "In evacuazione",
        ("evacuating", true) => "Evacuating",
        ("evacuated", false) => "Evacuato",
        ("evacuated", true) => "Evacuated",
        ("unit", false) => "Risorsa (rettangolo)",
        ("unit", true) => "Resource (rectangle)",
        ("target", false) => "Destinazione missione",
        ("target", true) => "Mission target",
        ("reported", false) => "Incendio segnalato",
        ("reported", true) => "Reported fire",
        ("title", false) => "Legenda",
        ("title", true) => "Legend",
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
