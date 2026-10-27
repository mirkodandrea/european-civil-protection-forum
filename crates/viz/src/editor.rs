//! Authoring controls send edits to the headless draft; scene systems only read it.
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use scenario::{
    editor::{Document, Selection},
    Scenario,
};
use std::sync::{Arc, Mutex};
use view::{Have, ViewSource, ViewState};

pub trait Storage: Send + Sync + 'static {
    fn list(&self) -> Result<Vec<Scenario>, String>;
    fn save(&self, scene: &Scenario) -> Result<(), String>;
    fn download(&self, _json: &str, _id: &str) -> Result<String, String> {
        Err("Esportazione download disponibile nel browser".into())
    }
}
#[derive(Resource)]
pub struct Editor {
    document: Arc<Mutex<Document>>,
    storage: Box<dyn Storage>,
    library: Vec<Scenario>,
    tool: String,
    selected: Option<Selection>,
    properties: String,
    json: String,
    show_json: bool,
    general: Option<String>,
    radius: f32,
    elevation: f32,
    fuel: i32,
    road_start: Option<scenario::Pos>,
    road_open: Option<usize>,
    press: Option<Vec2>,
    message: String,
    new_id: String,
    cells: usize,
    spacing: f32,
    pending: Option<Scenario>,
}
struct DraftSource(Arc<Mutex<Document>>);
impl ViewSource for DraftSource {
    fn scenario(&mut self) -> Result<Arc<Scenario>, String> {
        Ok(self.0.lock().map_err(|e| e.to_string())?.scene.clone())
    }
    fn poll_scene(&mut self, current: &Arc<Scenario>) -> Option<Arc<Scenario>> {
        let d = self.0.try_lock().ok()?;
        (!Arc::ptr_eq(current, &d.scene)).then(|| d.scene.clone())
    }
    fn poll(&mut self, have: Have) -> Option<ViewState> {
        let d = self.0.try_lock().ok()?;
        if have.generation == d.revision {
            return None;
        }
        let s = &d.scene;
        Some(ViewState {
            generation: d.revision,
            scenario_id: s.id.clone(),
            phase: "editor".into(),
            clock: "Anteprima scenario".into(),
            weather: Some(s.weather),
            cells: vec![0; s.grid.len()],
            arrival: vec![-1; s.grid.len()],
            ..Default::default()
        })
    }
}
pub fn run(scene: Scenario, storage: Box<dyn Storage>) -> Result<(), String> {
    let library = storage.list()?;
    let document = Arc::new(Mutex::new(Document::new(scene).map_err(|e| e.to_string())?));
    let editor = Editor {
        document: document.clone(),
        storage,
        library,
        tool: "select".into(),
        selected: None,
        properties: String::new(),
        json: String::new(),
        show_json: false,
        general: None,
        radius: 60.,
        elevation: 100.,
        fuel: 4,
        road_start: None,
        road_open: None,
        press: None,
        message: String::new(),
        new_id: "nuovo-scenario".into(),
        cells: 64,
        spacing: 20.,
        pending: None,
    };
    crate::run_internal(
        Box::new(DraftSource(document)),
        crate::Info::default(),
        Some(editor),
    );
    Ok(())
}
fn selected_json(s: &Scenario, selection: Selection) -> Option<serde_json::Value> {
    match selection {
        Selection::Building(i) => serde_json::to_value(s.buildings.get(i)?).ok(),
        Selection::Road(i) => serde_json::to_value(s.roads.get(i)?).ok(),
        Selection::Place(i) => serde_json::to_value(s.places.get(i)?).ok(),
        Selection::Ignition(i) => serde_json::to_value(s.initial_ignitions.get(i)?).ok(),
    }
}
fn set_selection(e: &mut Editor, d: &Document, selection: Option<Selection>) {
    e.selected = selection;
    e.properties = selection
        .and_then(|sel| selected_json(&d.scene, sel))
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_default();
}
fn outcome(e: &mut Editor, result: Result<(), String>) {
    e.message = result.err().unwrap_or_else(|| "Modifica applicata".into());
}

pub(crate) fn draw(
    mut contexts: EguiContexts,
    editor: Option<ResMut<Editor>>,
    windows: Query<&Window>,
    mut cameras: Query<&mut Camera, With<Camera3d>>,
) {
    let Some(mut editor) = editor else { return };
    let e = &mut *editor;
    let document = e.document.clone();
    let Ok(mut d) = document.try_lock() else {
        return;
    };
    let ctx = contexts.ctx_mut();
    egui::TopBottomPanel::top("editor-toolbar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.heading("Editor scenario 3D");
            ui.label(format!(
                "{}{}",
                d.scene.name,
                if d.dirty() { " • non salvato" } else { "" }
            ));
            if ui
                .add_enabled(d.can_undo(), egui::Button::new("Annulla"))
                .clicked()
            {
                d.undo();
                set_selection(e, &d, None);
            }
            if ui
                .add_enabled(d.can_redo(), egui::Button::new("Ripristina"))
                .clicked()
            {
                d.redo();
                set_selection(e, &d, None);
            }
            if ui.button("Salva nella libreria").clicked() {
                match e.storage.save(&d.scene) {
                    Ok(()) => {
                        d.mark_saved();
                        e.message = "Scenario salvato nella libreria".into();
                        if let Ok(lib) = e.storage.list() {
                            e.library = lib;
                        }
                    }
                    Err(err) => e.message = err,
                }
            }
            if ui.button("Scenario e meteo").clicked() {
                e.general = Some(
                    serde_json::to_string(&serde_json::json!({
                        "name":d.scene.name,"description":d.scene.description,
                        "start_clock_s":d.scene.start_clock_s,"turn_minutes":d.scene.turn_minutes,
                        "weather":d.scene.weather
                    }))
                    .unwrap(),
                );
            }
            if ui.button("Importa / esporta JSON").clicked() {
                e.json = d.export().unwrap_or_default();
                e.show_json = true;
            }
        });
    });
    egui::SidePanel::left("editor-tools")
        .default_width(280.)
        .resizable(true)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Progetto");
                ui.label("Identificativo nuovo scenario / copia");
                ui.text_edit_singleline(&mut e.new_id);
                ui.horizontal(|ui| {
                    ui.label("Celle per lato");
                    ui.add(egui::DragValue::new(&mut e.cells).range(8..=256));
                });
                ui.horizontal(|ui| {
                    ui.label("Metri per cella");
                    ui.add(egui::DragValue::new(&mut e.spacing).range(5. ..=100.));
                });
                if ui.button("Crea territorio vuoto").clicked() {
                    match scenario::editor::blank(&e.new_id, e.cells, e.spacing) {
                        Ok(s) => e.pending = Some(s),
                        Err(err) => e.message = err.to_string(),
                    }
                }
                if ui.button("Clona scenario corrente").clicked() {
                    let id = e.new_id.clone();
                    outcome(e, d.clone_as(&id).map_err(|err| err.to_string()));
                }
                ui.collapsing("Apri scenario esistente", |ui| {
                    for s in &e.library {
                        if ui.button(&s.name).on_hover_text(&s.id).clicked() {
                            e.pending = Some(s.clone());
                        }
                    }
                });
                ui.separator();
                ui.heading("Strumenti");
                for (id, label) in [
                    ("select", "Seleziona"),
                    ("height", "Quota terreno"),
                    ("fuel", "Vegetazione"),
                    ("building", "Aggiungi edificio"),
                    ("road", "Strada (polilinea)"),
                    ("place", "Aggiungi luogo"),
                    ("ignition", "Aggiungi innesco"),
                ] {
                    if ui
                        .selectable_value(&mut e.tool, id.to_string(), label)
                        .clicked()
                    {
                        e.road_start = None;
                        e.road_open = None;
                    }
                }
                if e.tool == "height" || e.tool == "fuel" {
                    ui.add(egui::Slider::new(&mut e.radius, 10. ..=500.).text("Raggio m"));
                    if e.tool == "height" {
                        ui.add(
                            egui::DragValue::new(&mut e.elevation)
                                .range(-1000. ..=5000.)
                                .prefix("Quota m "),
                        );
                    } else {
                        egui::ComboBox::from_label("Combustibile")
                            .selected_text(e.fuel.to_string())
                            .show_ui(ui, |ui| {
                                if d.scene.fuel_definitions.is_empty() {
                                    for n in 1..=7 {
                                        ui.selectable_value(
                                            &mut e.fuel,
                                            n,
                                            scenario::fuel::name(n),
                                        );
                                    }
                                } else {
                                    for f in &d.scene.fuel_definitions {
                                        ui.selectable_value(&mut e.fuel, f.id, &f.name);
                                    }
                                }
                            });
                    }
                }
                if e.road_start.is_some() || e.road_open.is_some() {
                    ui.label(if e.road_open.is_some() {
                        "Clic: aggiungi vertice. «Termina strada» per chiudere."
                    } else {
                        "Scegli il secondo punto sulla mappa."
                    });
                    if ui.button("Termina strada").clicked() {
                        e.road_start = None;
                        e.road_open = None;
                    }
                }
                ui.separator();
                ui.heading("Oggetti");
                let s = d.scene.clone();
                for (i, b) in s.buildings.iter().enumerate() {
                    let sel = Selection::Building(i);
                    if ui
                        .selectable_label(
                            e.selected == Some(sel),
                            format!("Edificio {} · {}", b.id, b.name.as_deref().unwrap_or("")),
                        )
                        .clicked()
                    {
                        set_selection(e, &d, Some(sel));
                    }
                }
                for (i, r) in s.roads.iter().enumerate() {
                    let sel = Selection::Road(i);
                    if ui
                        .selectable_label(e.selected == Some(sel), format!("Strada · {}", r.name))
                        .clicked()
                    {
                        set_selection(e, &d, Some(sel));
                    }
                }
                for (i, p) in s.places.iter().enumerate() {
                    let sel = Selection::Place(i);
                    if ui
                        .selectable_label(e.selected == Some(sel), format!("Luogo · {}", p.name))
                        .clicked()
                    {
                        set_selection(e, &d, Some(sel));
                    }
                }
                for (i, _) in s.initial_ignitions.iter().enumerate() {
                    let sel = Selection::Ignition(i);
                    if ui
                        .selectable_label(e.selected == Some(sel), format!("Innesco {}", i + 1))
                        .clicked()
                    {
                        set_selection(e, &d, Some(sel));
                    }
                }
            });
        });
    if let Some(selection) = e.selected {
        egui::SidePanel::right("editor-inspector")
            .default_width(290.)
            .show(ctx, |ui| {
                ui.heading("Proprietà oggetto");
                ui.label("Modifica posizione, nome e parametri; Applica conferma la bozza.");
                if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&e.properties) {
                    let before = value.clone();
                    property_fields(ui, &mut value, "");
                    if before != value {
                        e.properties = serde_json::to_string_pretty(&value).unwrap();
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button("Applica").clicked() {
                        let result = d.edit(|s| {
                            match selection {
                                Selection::Building(i) => {
                                    s.buildings[i] = serde_json::from_str(&e.properties)?
                                }
                                Selection::Road(i) => {
                                    s.roads[i] = serde_json::from_str(&e.properties)?
                                }
                                Selection::Place(i) => {
                                    s.places[i] = serde_json::from_str(&e.properties)?
                                }
                                Selection::Ignition(i) => {
                                    s.initial_ignitions[i] = serde_json::from_str(&e.properties)?
                                }
                            };
                            Ok(())
                        });
                        outcome(e, result.map_err(|err| err.to_string()));
                    }
                    if ui.button("Elimina").clicked() {
                        outcome(e, d.delete(selection).map_err(|err| err.to_string()));
                        set_selection(e, &d, None);
                    }
                });
            });
    }
    egui::TopBottomPanel::bottom("editor-status").show(ctx,|ui| {
        ui.label(&e.message);
        ui.small("Clic: applica strumento · trascina: sposta vista · destro: ruota · rotella: zoom · R: inquadra territorio");
    });
    if e.pending.is_some() {
        let mut apply = !d.dirty();
        if d.dirty() {
            egui::Window::new("Modifiche non salvate").collapsible(false).show(ctx,|ui| {
            ui.label("Aprire il progetto scelto sostituisce la bozza. Puoi annullare l’apertura e salvarla.");
            if ui.button("Apri e sostituisci bozza").clicked(){apply=true;}
            if ui.button("Annulla apertura").clicked(){e.pending=None;}
        });
        }
        if apply {
            if let Some(s) = e.pending.take() {
                outcome(
                    e,
                    d.edit(|next| {
                        *next = s;
                        Ok(())
                    })
                    .map_err(|err| err.to_string()),
                );
                set_selection(e, &d, None);
                e.road_start = None;
                        e.road_open = None;
            }
        }
    }
    if let (Ok(window), Ok(mut camera)) = (windows.get_single(), cameras.get_single_mut()) {
        let rect = ctx.available_rect();
        let scale = window.scale_factor();
        camera.viewport = Some(bevy::render::camera::Viewport {
            physical_position: UVec2::new((rect.min.x * scale) as u32, (rect.min.y * scale) as u32),
            physical_size: UVec2::new(
                (rect.width() * scale).max(1.) as u32,
                (rect.height() * scale).max(1.) as u32,
            ),
            ..Default::default()
        });
    }
    if let Some(mut general) = e.general.take() {
        let mut keep = true;
        egui::Window::new("Scenario e meteo")
            .open(&mut keep)
            .show(ctx, |ui| {
                if let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&general) {
                    property_fields(ui, &mut value, "");
                    general = serde_json::to_string(&value).unwrap();
                }
                if ui.button("Applica parametri").clicked() {
                    let result = d.edit(|s| {
                        let edits: scenario::ScenarioEdits = serde_json::from_str(&general)?;
                        s.apply_edits(&edits)
                    });
                    outcome(e, result.map_err(|err| err.to_string()));
                }
            });
        if keep {
            e.general = Some(general);
        }
    }
    if e.show_json {
        let mut open = true;
        egui::Window::new("Scenario completo · JSON trasferibile").open(&mut open).default_size([700.,500.]).show(ctx,|ui| {
            ui.label("Include territorio, ruoli, risorse, meteo e inject. Per importare un pack, aprilo dalla libreria.");
            ui.horizontal(|ui| {
                if ui.button("Aggiorna esportazione").clicked(){e.json=d.export().unwrap_or_default();}
                #[cfg(not(target_arch="wasm32"))]
                if ui.button("Copia JSON").clicked(){ui.output_mut(|o|o.copied_text=e.json.clone());}
                #[cfg(target_arch="wasm32")]
                if ui.button("Scarica JSON").clicked(){e.message=e.storage.download(&e.json,&d.scene.id).unwrap_or_else(|err|err);}
                if ui.button("Importa e sostituisci bozza").clicked(){let json=e.json.clone();outcome(e,d.import(&json).map_err(|err|err.to_string()));set_selection(e,&d,None);}
            });
            egui::ScrollArea::both().show(ui,|ui| {ui.add(egui::TextEdit::multiline(&mut e.json).code_editor().desired_width(f32::INFINITY));});
        });
        e.show_json = open;
    }
}
fn property_fields(ui: &mut egui::Ui, v: &mut serde_json::Value, key: &str) {
    use serde_json::Value;
    let key = match key {
        "name" => "Nome",
        "description" => "Descrizione",
        "kind" => "Tipo",
        "pos" | "at" => "Posizione (est, nord) m",
        "occupants" => "Occupanti",
        "size" => "Dimensioni m",
        "shelter_capacity" => "Capienza rifugio",
        "radius_m" => "Raggio m",
        "sector" => "Settore",
        "points" => "Vertici strada (est, nord) m",
        "class" => "Classe strada",
        "drivable" => "Percorribile",
        "oneway" => "Senso unico",
        "visual_kind" => "Variante grafica",
        "aliases" => "Nomi alternativi",
        "weather" => "Meteo",
        "wind_dir_deg" => "Vento: direzione di provenienza °",
        "wind_speed_kmh" => "Vento km/h",
        "moisture_pct" => "Umidità combustibile %",
        "turn_minutes" => "Durata turno min",
        "start_clock_s" => "Ora iniziale (secondi da Day 1 00:00)",
        _ => key,
    };
    match v {
        Value::Object(fields) => {
            for (k, v) in fields {
                property_fields(ui, v, k);
            }
        }
        Value::Array(a) => {
            ui.collapsing(key, |ui| {
                for (i, v) in a.iter_mut().enumerate() {
                    property_fields(ui, v, &i.to_string());
                }
            });
        }
        Value::String(s) => {
            ui.horizontal(|ui| {
                ui.label(key);
                ui.text_edit_singleline(s);
            });
        }
        Value::Number(n) => {
            let mut x = n.as_f64().unwrap_or(0.);
            let unsigned = n.is_u64();
            let signed = n.is_i64();
            ui.horizontal(|ui| {
                ui.label(key);
                if ui.add(egui::DragValue::new(&mut x)).changed() {
                    if unsigned {
                        *v = Value::from(x.max(0.) as u64);
                    } else if signed {
                        *v = Value::from(x as i64);
                    } else {
                        *v = Value::from(x);
                    }
                }
            });
        }
        Value::Bool(b) => {
            ui.checkbox(b, key);
        }
        Value::Null => {
            ui.label(format!("{key}: —"));
        }
    }
}

pub(crate) fn interact(
    mut contexts: EguiContexts,
    editor: Option<ResMut<Editor>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    buttons: Res<ButtonInput<MouseButton>>,
    scn: Res<crate::Scn>,
    mut gizmos: Gizmos,
) {
    let Some(mut e) = editor else { return };
    let Ok(window) = windows.get_single() else {
        return;
    };
    let Ok((camera, transform)) = cameras.get_single() else {
        return;
    };
    let s = &scn.0;
    let ctx = contexts.ctx_mut();
    let viewport = ctx.available_rect();
    let viewport_origin = camera
        .logical_viewport_rect()
        .map(|r| r.min)
        .unwrap_or_default();
    let painter = ctx
        .layer_painter(egui::LayerId::new(
            egui::Order::Background,
            egui::Id::new("editor-labels"),
        ))
        .with_clip_rect(viewport);
    for name in s.zones.names() {
        if let Some(b) = s.zone_bounds(&name) {
            let at = [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5];
            if let Some(p) = camera.world_to_viewport(transform, crate::frame::to_bevy(s, at, 5.)) {
                painter.text(
                    egui::pos2(p.x + viewport_origin.x, p.y + viewport_origin.y),
                    egui::Align2::CENTER_CENTER,
                    name,
                    egui::FontId::proportional(13.),
                    egui::Color32::WHITE,
                );
            }
        }
    }
    let selected_position = match e.selected {
        Some(Selection::Building(i)) => s
            .buildings
            .get(i)
            .map(|b| (b.pos, b.size[0].max(b.size[1]))),
        Some(Selection::Place(i)) => s.places.get(i).map(|p| (p.pos, 25.)),
        _ => None,
    };
    if let Some((at, radius)) = selected_position {
        gizmos.circle(
            crate::frame::to_bevy(s, at, 6.),
            Dir3::Y,
            radius,
            Color::srgb(1., 1., 0.),
        );
    }
    if let Some(Selection::Road(i)) = e.selected {
        if let Some(road) = s.roads.get(i) {
            for pair in road.points.windows(2) {
                gizmos.line(
                    crate::frame::to_bevy(s, pair[0], 8.),
                    crate::frame::to_bevy(s, pair[1], 8.),
                    Color::srgb(1., 1., 0.),
                );
            }
        }
    }
    // Markers are authored objects, never simulated fire.
    for (i, p) in s.initial_ignitions.iter().enumerate() {
        let at = crate::frame::to_bevy(s, p.at, 8.);
        gizmos.circle(
            at,
            Dir3::Y,
            p.radius_m,
            if e.selected == Some(Selection::Ignition(i)) {
                Color::srgb(1., 1., 0.)
            } else {
                Color::srgb(1., 0.3, 0.)
            },
        );
    }
    for p in &s.places {
        let at = crate::frame::to_bevy(s, p.pos, 8.);
        gizmos.line(at, at + Vec3::Y * 70., Color::srgb(0.1, 0.8, 1.));
    }
    if let Some(start) = e.road_start {
        let p = crate::frame::to_bevy(s, start, 10.);
        gizmos.sphere(p, Quat::IDENTITY, 12., Color::srgb(1., 1., 0.));
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if contexts.ctx_mut().wants_pointer_input() {
        if buttons.just_pressed(MouseButton::Left) {
            e.press = None;
        }
        return;
    }
    // Ray march the same bilinear terrain surface used by presentation.
    let Some(ray) = camera.viewport_to_world(transform, cursor - viewport_origin) else {
        return;
    };
    let mut previous = 0.;
    let mut hit = None;
    for i in 1..=1024 {
        let t = i as f32 * (s.grid.width_m().max(s.grid.height_m()) * 8. / 1024.);
        let p = ray.get_point(t);
        let world = crate::frame::to_world(s, p);
        if scenario::editor::inside(s, world) && p.y <= crate::frame::to_bevy(s, world, 0.).y {
            let mut lo = previous;
            let mut hi = t;
            for _ in 0..14 {
                let mid = (lo + hi) * 0.5;
                let p = ray.get_point(mid);
                let at = crate::frame::to_world(s, p);
                if p.y > crate::frame::to_bevy(s, at, 0.).y {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            hit = Some(crate::frame::to_world(s, ray.get_point(hi)));
            break;
        }
        previous = t;
    }
    if let Some(at) = hit {
        if e.tool == "height" || e.tool == "fuel" {
            gizmos.circle(
                crate::frame::to_bevy(s, at, 5.),
                Dir3::Y,
                e.radius,
                Color::srgb(1., 1., 0.),
            );
        }
        if buttons.just_pressed(MouseButton::Left) {
            e.press = Some(cursor);
        }
        if buttons.just_released(MouseButton::Left)
            && e.press.take().is_some_and(|p| p.distance(cursor) < 6.)
        {
            let document = e.document.clone();
            let Ok(mut d) = document.try_lock() else {
                return;
            };
            if !Arc::ptr_eq(s, &d.scene) {
                return;
            }
            let result = match e.tool.as_str() {
                "select" => {
                    let mut objects: Vec<(f32, Selection)> = vec![];
                    for (i, b) in s.buildings.iter().enumerate() {
                        objects.push((
                            scenario::editor::distance(at, b.pos),
                            Selection::Building(i),
                        ));
                    }
                    for (i, p) in s.places.iter().enumerate() {
                        objects.push((scenario::editor::distance(at, p.pos), Selection::Place(i)));
                    }
                    for (i, p) in s.initial_ignitions.iter().enumerate() {
                        objects
                            .push((scenario::editor::distance(at, p.at), Selection::Ignition(i)));
                    }
                    let selection = objects
                        .into_iter()
                        .filter(|(dist, _)| *dist < 80.)
                        .min_by(|a, b| a.0.total_cmp(&b.0))
                        .map(|(_, sel)| sel);
                    set_selection(&mut e, &d, selection);
                    Ok(())
                }
                "height" => d.brush(at, e.radius, Some(e.elevation), None),
                "fuel" => d.brush(at, e.radius, None, Some(e.fuel)),
                "road" => {
                    if let Some(i) = e.road_open {
                        d.extend_road(i, at)
                            .map(|_| set_selection(&mut e, &d, Some(Selection::Road(i))))
                    } else if let Some(start) = e.road_start.take() {
                        d.add("road", start, Some(at)).map(|sel| {
                            if let Selection::Road(i) = sel {
                                e.road_open = Some(i);
                            }
                            set_selection(&mut e, &d, Some(sel))
                        })
                    } else {
                        e.road_start = Some(at);
                        Ok(())
                    }
                }
                _ => {
                    let tool = e.tool.clone();
                    d.add(&tool, at, None)
                        .map(|sel| set_selection(&mut e, &d, Some(sel)))
                }
            };
            outcome(&mut e, result.map_err(|err| err.to_string()));
        }
    } else if buttons.just_released(MouseButton::Left) {
        e.press = None;
    }
}

// DOM paste events feed egui through a queue; no unstable clipboard API is needed.
static PASTES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
pub fn paste(text: String) {
    if let Ok(mut queue) = PASTES.lock() {
        queue.push(text);
    }
}
pub(crate) fn paste_input(
    editor: Option<Res<Editor>>,
    mut input: Query<&mut bevy_egui::EguiInput>,
) {
    if editor.is_none() {
        return;
    }
    if let (Ok(mut queue), Ok(mut input)) = (PASTES.lock(), input.get_single_mut()) {
        for text in queue.drain(..) {
            input.0.events.push(egui::Event::Paste(text));
        }
    }
}
