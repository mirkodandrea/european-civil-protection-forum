//! Map legend with shapes and textures next to colours (colour-blind safe).
//! Strings are Italian/English pairs; other player languages come from `i18n_more`.
//! The glyphs and textures here mirror what `map.rs` draws.
use crate::fuel_palette::{rgb, Family, COAST, SEA};
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
        "warned" => pair(l, "Zona allertata (ora)", "Warned zone (time)"),
        "warned_short" => pair(l, "Allertata", "Warned"),
        "threatened" => pair(l, "Edificio minacciato", "Threatened building"),
        "damaged" => pair(l, "Danneggiato", "Damaged"),
        "destroyed" => pair(l, "Distrutto", "Destroyed"),
        "evacuating" => pair(l, "In evacuazione", "Evacuating"),
        "evacuated" => pair(l, "Evacuato", "Evacuated"),
        "unit" => pair(l, "Risorsa (rettangolo)", "Resource (rectangle)"),
        "target" => pair(l, "Destinazione missione", "Mission target"),
        "reported" => pair(l, "Incendio segnalato", "Reported fire"),
        "title" => pair(l, "Legenda", "Legend"),
        "grass" => pair(l, "Prateria", "Grassland"),
        "broadleaves" => pair(l, "Latifoglie", "Broadleaves"),
        "shrubs" => pair(l, "Arbusteti", "Shrubs"),
        "conifers" => pair(l, "Conifere", "Conifers"),
        "nonveg" => pair(l, "Non vegetato", "Non-vegetated"),
        "sea" => pair(l, "Mare / lago", "Sea / lake"),
        "susceptibility" => pair(l, "più scuro = più suscettibile", "darker = more susceptible"),
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
    // Base cover: one swatch per family, low → high susceptibility left to right,
    // in the colours `fuel_palette` paints.
    let css = |c: [u8; 3]| format!("rgb({},{},{})", c[0], c[1], c[2]);
    let cover = |family: Family, key: &'static str| {
        let style = format!(
            "background:linear-gradient(90deg,{} 0 33%,{} 33% 67%,{} 67%)",
            css(rgb(family, 0)),
            css(rgb(family, 1)),
            css(rgb(family, 2))
        );
        html! {<li><span class="lg-sw" style={style} aria-hidden="true"></span>{lg(l, key)}</li>}
    };
    let nonveg = format!("background:{}", css(rgb(Family::NonVegetated, 1)));
    let sea = format!("background:{};box-shadow:inset 3px 0 0 {}", css(SEA), css(COAST));
    html! {<>
        <ul class="map-legend map-legend-sym map-legend-cover" aria-label={lg(l, "title")}>
            {cover(Family::Grassland, "grass")}{cover(Family::Broadleaves, "broadleaves")}
            {cover(Family::Shrubs, "shrubs")}{cover(Family::Conifers, "conifers")}
            <li><span class="lg-sw" style={nonveg} aria-hidden="true"></span>{lg(l, "nonveg")}</li>
            <li><span class="lg-sw" style={sea} aria-hidden="true"></span>{lg(l, "sea")}</li>
            <li class="lg-note">{lg(l, "susceptibility")}</li>
        </ul>
        <ul class="map-legend map-legend-sym" aria-label={lg(l, "title")}>
            {sw("lg-fire", "fire")}{sw("lg-burnt", "burnt")}{sw("lg-possible", "possible")}{sw("lg-water", "water")}
            {sw("lg-line", "line")}{sw("lg-forecast", "forecast")}{sw("lg-cut", "cut")}{sw("lg-closure", "police_closure")}{sw("lg-warned", "warned")}
            {gl("▲", "g-threat", "threatened")}{gl("◆", "g-dmg", "damaged")}{gl("✕", "g-dest", "destroyed")}
            {gl("➜", "g-evac", "evacuating")}{gl("○", "g-done", "evacuated")}
            {sw("lg-unit", "unit")}{gl("◇", "g-target", "target")}{gl("◯", "g-rep", "reported")}
        </ul>
    </>}
}
