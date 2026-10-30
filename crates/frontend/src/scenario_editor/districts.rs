//! "Borghi" tab: the localities `EvacuateDistrict` evacuates. Membership is a
//! list of building ids, picked by clicking the houses on the map.
use super::*;
use scenario::{District, DistrictEvacuation};

/// One building of the territory as the reference lists it:
/// `[id, x, y, occupants, name, kind, shelter_capacity]`.
#[derive(Clone, PartialEq)]
struct Bld {
    id: u32,
    pos: [f64; 2],
    occupants: u32,
    name: Option<String>,
    shelter: u32,
}

fn buildings(o: &Open) -> Vec<Bld> {
    list(&o.reference["buildings"])
        .iter()
        .filter_map(|b| {
            Some(Bld {
                id: b[0].as_u64()? as u32,
                pos: [b[1].as_f64()?, b[2].as_f64()?],
                occupants: b[3].as_u64().unwrap_or(0) as u32,
                name: b[4].as_str().map(str::to_string),
                shelter: b[6].as_u64().unwrap_or(0) as u32,
            })
        })
        .collect()
}

/// Distinguishable fills for the boroughs, cycled.
const HUES: [&str; 8] = ["#d9480f", "#1c7ed6", "#7048e8", "#c2255c", "#0b7285", "#a8721c", "#5c940d", "#e8590c"];

fn hue(i: usize) -> &'static str {
    HUES[i % HUES.len()]
}

/// Adds or removes `id` in district `i`; a house belongs to one borgo only,
/// so adding it takes it away from any other.
fn toggle(districts: &mut [District], i: usize, id: u32) {
    let had = districts.get(i).is_some_and(|d| d.buildings.contains(&id));
    for d in districts.iter_mut() {
        d.buildings.retain(|b| *b != id);
    }
    if !had {
        if let Some(d) = districts.get_mut(i) {
            d.buildings.push(id);
        }
    }
}

#[derive(Properties, Clone)]
pub(super) struct DistrictsProps {
    pub o: Open,
    pub open: UseStateHandle<Option<Open>>,
}

impl PartialEq for DistrictsProps {
    fn eq(&self, _: &Self) -> bool {
        false // the scenario changes with every edit
    }
}

pub(super) fn districts_tab(o: &Open, open: &UseStateHandle<Option<Open>>) -> Html {
    html! {<DistrictsTab o={o.clone()} open={open.clone()}/>}
}

#[function_component(DistrictsTab)]
fn districts_view(props: &DistrictsProps) -> Html {
    let (o, open) = (&props.o, &props.open);
    let selected = use_state(|| 0usize);
    let map = use_node_ref();
    let districts = o.pack.edits.districts.clone().unwrap_or_default();
    let pace = o.pack.edits.district_evacuation.unwrap_or_default();
    let all = buildings(o);
    let g = geom(o);
    let n = districts.len();
    let sel = (*selected).min(n.saturating_sub(1));
    let owner = |id: u32| districts.iter().position(|d| d.buildings.contains(&id));
    let people = |d: &District| -> u32 { d.buildings.iter().filter_map(|id| all.iter().find(|b| b.id == *id)).map(|b| b.occupants).sum() };
    let gatherings: Vec<(String, String)> = std::iter::once((String::new(), ex("Area di raccolta più vicina").to_string()))
        .chain(all.iter().filter(|b| b.shelter > 0).filter_map(|b| b.name.clone()).map(|n| (n.clone(), n)))
        .collect();

    let items = html! {<>
        <h5 class="md-group" title={ex("Località evacuabili in blocco con «Evacua borgo».")}>{ex("Borghi")}</h5>
        {for districts.iter().enumerate().map(|(i, d)| {
            let bad = d.id.is_empty() || d.buildings.is_empty() || districts.iter().filter(|x| x.id == d.id).count() > 1;
            md_item(i == sel && n > 0, { let selected = selected.clone(); Callback::from(move |_| selected.set(i)) }, html!{<>
                <span class="dist-swatch" style={format!("background:{}", hue(i))}/>
                <b class="md-id">{d.id.clone()}</b>
                { if bad { html!{<span class="pill pending" title={ex("Identificativo vuoto o duplicato, oppure nessuna casa.")}>{"!"}</span>} } else { Html::default() } }
                <span class="md-name">{d.name.clone()}</span>
                <small class="md-sub">{crate::exfmt!("{} case", "{} houses", d.buildings.len())}{" · "}{crate::exfmt!("{} persone", "{} people", people(d))}</small>
            </>})
        })}
        { if n == 0 { html!{<p class="muted">{ex("Nessun borgo definito: il comando «Evacua borgo» non è disponibile.")}</p>} } else { Html::default() } }
    </>};
    let tools = html! {
        <button class="primary" onclick={{
            let (selected, add) = (selected.clone(), upd(open, |p, _: MouseEvent| {
                let l = p.edits.districts.get_or_insert_with(Vec::new);
                let k = l.len() + 1;
                l.push(District { id: format!("borgo_{k}"), name: format!("Borgo {k}"), buildings: vec![], gathering: None });
            }));
            Callback::from(move |e| { selected.set(n); add.emit(e) })
        }}>{ex("+ Borgo")}</button>
    };

    let set = |f: fn(&mut District, String)| {
        upd(open, move |p, v: String| if let Some(d) = p.edits.districts.as_mut().and_then(|l| l.get_mut(sel)) { f(d, v) })
    };
    let detail = match districts.get(sel) {
        None => html! {<p class="muted">{ex("Aggiungi un borgo, poi clicca sulla mappa le case che ne fanno parte.")}</p>},
        Some(d) => {
            let duplicate = districts.iter().filter(|x| x.id == d.id).count() > 1;
            html! {<div class="rr-body" key={sel}>
                <div class="settings-fields">
                    {field("Nome", "Nome del borgo, come lo scrive l’EXCON: «Evacua borgo di …» lo trova per nome o per identificativo.", d.name.clone(), set(|d, v| d.name = v))}
                    {field("Identificativo", "Codice breve unico: solo a-z, 0-9 e _.", d.id.clone(), set(|d, v| d.id = v))}
                    {choice("Area di raccolta", "Edificio con capienza dove si radunano gli evacuati quando l’ordine non indica una destinazione. Senza, vale il rifugio aperto o l’uscita dalla mappa più vicini.", d.gathering.as_deref().unwrap_or(""), &with_current(&gatherings, d.gathering.as_deref().unwrap_or("")), set(|d, v| d.gathering = Some(v).filter(|v| !v.is_empty())))}
                </div>
                { if duplicate { html!{<p class="error">{ex("Identificativo già usato da un altro borgo.")}</p>} } else { Html::default() } }
                <p class="muted">{crate::exfmt!("{} case · {} persone. Clicca una casa sulla mappa per aggiungerla o toglierla; una casa appartiene a un solo borgo.", "{} houses · {} people. Click a house on the map to add or remove it; a house belongs to one borgo only.", d.buildings.len(), people(d))}</p>
                <button type="button" disabled={d.buildings.is_empty()} onclick={upd(open, move |p, _: MouseEvent| if let Some(d) = p.edits.districts.as_mut().and_then(|l| l.get_mut(sel)) { d.buildings.clear() })}>{ex("Svuota il borgo")}</button>
                {confirm_delete("Elimina borgo", "Elimina il borgo: le sue case tornano non assegnate.",
                    upd(open, move |p, _: MouseEvent| { if let Some(l) = p.edits.districts.as_mut() { if sel < l.len() { l.remove(sel); } } }))}
            </div>}
        }
    };

    let radius = g.width / 260.0;
    let onclick = {
        let (all, map, open) = (all.clone(), map.clone(), open.clone());
        map_click(&map.clone(), g, Callback::from(move |(at, _): ([f32; 2], Option<usize>)| {
            if n == 0 {
                return;
            }
            let reach = g.width / 80.0;
            let nearest = all
                .iter()
                .map(|b| (((b.pos[0] - at[0] as f64).powi(2) + (b.pos[1] - at[1] as f64).powi(2)).sqrt(), b.id))
                .filter(|(d, _)| *d <= reach)
                .min_by(|a, b| a.0.total_cmp(&b.0));
            let (Some((_, id)), Some(cur)) = (nearest, &*open) else { return };
            let mut next = cur.clone();
            if let Some(l) = next.pack.edits.districts.as_mut() {
                toggle(l, sel, id);
                next.dirty = true;
                open.set(Some(next));
            }
        }))
    };
    let dots = html! {<>{for all.iter().map(|b| {
        let (fill, r) = match owner(b.id) {
            Some(i) if i == sel => (hue(i), radius * 1.5),
            Some(i) => (hue(i), radius),
            None => ("#8a948c", radius * 0.8),
        };
        let (cx, cy) = (b.pos[0].to_string(), (g.height - b.pos[1]).to_string());
        if b.name.is_some() {
            let s = r * 1.8;
            html!{<rect x={(b.pos[0] - s / 2.0).to_string()} y={(g.height - b.pos[1] - s / 2.0).to_string()} width={s.to_string()} height={s.to_string()} fill={fill} stroke="#222" stroke-width={(r * 0.2).to_string()} opacity={if owner(b.id).is_none() { "0.7" } else { "1" }}><title>{b.name.clone().unwrap_or_default()}</title></rect>}
        } else {
            html!{<circle cx={cx} cy={cy} r={r.to_string()} fill={fill} opacity={if owner(b.id).is_none() { "0.7" } else { "1" }}/>}
        }
    })}</>};
    let unassigned = all.iter().filter(|b| b.name.is_none() && owner(b.id).is_none()).count();

    html! {<div class="rr-tab dist-tab">
        <p class="muted">{ex("I borghi sono le località che l’EXCON può evacuare con un solo ordine, case sparse comprese. Le case si scelgono sulla mappa.")}</p>
        <div class="settings-fields dist-pace">
            {number("Persone al minuto (partenze)", "Quante persone del borgo partono ogni minuto, dalle case più vicine alla strada.", fmt(pace.per_min as f64), upd(open, move |p, v: String| if let Some(x) = num::<f32>(&v) { let e = p.edits.district_evacuation.get_or_insert_with(DistrictEvacuation::default); e.per_min = x }))}
            {number("Preparazione (min)", "Minuti tra l’ordine e le prime partenze (massimo 240).", pace.prep_min.to_string(), upd(open, move |p, v: String| if let Some(x) = num::<u32>(&v) { let e = p.edits.district_evacuation.get_or_insert_with(DistrictEvacuation::default); e.prep_min = x }))}
        </div>
        {master_detail(items, tools, html! {<>
            {detail}
            <svg ref={map} class="ignition-map dist-map" viewBox={format!("0 0 {} {}", g.width, g.height)} style={format!("aspect-ratio:{}/{}", g.width, g.height)} role="img" aria-label={ex("Mappa delle case dei borghi")} {onclick}>
                {map_base(o, g)}
                {dots}
            </svg>
            <small class="muted">{crate::exfmt!("{} case senza borgo (grigie). Quadrati: edifici con nome.", "{} houses without a borgo (grey). Squares: named buildings.", unassigned)}</small>
        </>})}
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(id: &str, b: &[u32]) -> District {
        District { id: id.into(), name: id.into(), buildings: b.to_vec(), gathering: None }
    }

    #[test]
    fn a_house_moves_between_boroughs_and_toggles_off() {
        let mut l = vec![d("a", &[1, 2]), d("b", &[3])];
        toggle(&mut l, 1, 2);
        assert_eq!((l[0].buildings.clone(), l[1].buildings.clone()), (vec![1], vec![3, 2]));
        toggle(&mut l, 1, 2);
        assert_eq!(l[1].buildings, vec![3]);
    }
}
