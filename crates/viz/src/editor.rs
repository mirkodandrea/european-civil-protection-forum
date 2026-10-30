//! Authoring controls send edits to the headless draft; scene systems only read it.
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use scenario::{
    editor::{BuildingSpec, Document, Selection, FACILITY_PRESETS, VISUAL_KINDS},
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
    /// "push" (raise, Shift lowers) or "level" (set to `elevation`).
    sculpt_mode: String,
    strength: f32,
    stroking: bool,
    stroke_clock: f32,
    last_paint: Option<scenario::Pos>,
    busy_until: f64,
    /// Dabs of the stroke in progress, drawn as a cheap preview until the scene catches up.
    dabs: Vec<(scenario::Pos, f32, bool)>,
    fuel: i32,
    spec: BuildingSpec,
    road_start: Option<scenario::Pos>,
    road_open: Option<usize>,
    press: Option<Vec2>,
    /// Handle being dragged with the select tool, and where the cursor last was.
    drag: Option<Drag>,
    drag_at: Option<scenario::Pos>,
    message: String,
    new_id: String,
    cells: usize,
    spacing: f32,
    pending: Option<Scenario>,
}
#[derive(Clone, Copy, PartialEq)]
enum Drag {
    Object(Selection),
    Vertex(usize, usize),
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
impl Editor {
    /// Hold-to-paint tools take the left button and the wheel from the camera.
    /// A brush is working or has only just stopped.
    pub(crate) fn busy(&self, now: f64) -> bool {
        self.stroking || self.drag.is_some() || now < self.busy_until
    }
    /// The left button belongs to the tool, not to camera panning.
    pub fn captures_pointer(&self) -> bool {
        self.brush_tool() || self.drag.is_some()
    }
    pub fn brush_tool(&self) -> bool {
        self.tool == "height" || self.tool == "fuel"
    }
}

pub fn run(scene: Scenario, storage: Box<dyn Storage>) -> Result<(), String> {
    let library = storage.list()?;
    let first_id = scene.id.clone();
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
        sculpt_mode: "push".into(),
        strength: 15.,
        stroking: false,
        stroke_clock: 0.,
        last_paint: None,
        busy_until: 0.,
        dabs: Vec::new(),
        fuel: 4,
        spec: BuildingSpec::default(),
        road_start: None,
        road_open: None,
        press: None,
        drag: None,
        drag_at: None,
        message: String::new(),
        new_id: first_id,
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
                    ("height", "Quota terreno (pennello)"),
                    ("fuel", "Vegetazione (pennello)"),
                    ("building", "Aggiungi edificio"),
                    ("road", "Strada (polilinea)"),
                    ("place", "Aggiungi luogo"),
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
                    ui.add(egui::Slider::new(&mut e.radius, 10. ..=1000.).text("Raggio m"));
                    ui.small("Rotella: raggio · Ctrl/Cmd + rotella: zoom");
                    if e.tool == "height" {
                        ui.radio_value(&mut e.sculpt_mode, "push".into(), "Alza (Maiusc: abbassa)");
                        ui.radio_value(&mut e.sculpt_mode, "level".into(), "Livella a una quota");
                        if e.sculpt_mode == "push" {
                            ui.add(egui::Slider::new(&mut e.strength, 1. ..=100.).text("Intensità m/s"));
                        } else {
                            ui.add(
                                egui::DragValue::new(&mut e.elevation)
                                    .range(-1000. ..=5000.)
                                    .prefix("Quota m "),
                            );
                        }
                        ui.small("Tieni premuto sulla mappa per modellare il terreno. Sotto 0 m il terreno diventa acqua (laghi, isole).");
                    } else {
                        ui.small("Tieni premuto sulla mappa per dipingere.");
                        egui::ComboBox::from_label("Combustibile")
                            .selected_text(d.scene.fuel_name(e.fuel))
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
                if e.tool == "building" {
                    building_spec_fields(ui, &mut e.spec);
                    ui.small("Q/E: ruota di 15° · clic: posiziona");
                }
                if e.tool == "select" {
                    ui.small("Trascina l’oggetto selezionato per spostarlo. Strada selezionata: trascina i vertici · Maiusc+clic su un tratto: nuovo vertice · X sul vertice: rimuovi.");
                    if let Some(Selection::Road(i)) = e.selected {
                        if ui.button("Aggiungi vertici in coda").clicked() {
                            e.tool = "road".into();
                            e.road_open = Some(i);
                        }
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
                    typed_fields(ui, selection, &mut value, &d.scene);
                    // Keys edited by `typed_fields` are hidden from the generic list.
                    let hidden: &[&str] = match selection {
                        Selection::Building(_) => &["kind", "visual_kind", "rotation_deg"],
                        Selection::Place(_) => &["radius_m", "buildings", "roads"],
                        _ => &[],
                    };
                    let mut rest = value.clone();
                    if let Some(o) = rest.as_object_mut() {
                        hidden.iter().for_each(|k| {
                            o.remove(*k);
                        });
                    }
                    property_fields(ui, &mut rest, "");
                    if let (Some(rest), Some(o)) = (rest.as_object(), value.as_object_mut()) {
                        rest.iter().for_each(|(k, v)| {
                            o.insert(k.clone(), v.clone());
                        });
                    }
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
        ui.small("Seleziona: trascina per spostare oggetti e vertici · Clic: applica strumento · pennelli: tieni premuto · trascina: sposta vista · destro: ruota · rotella: zoom (raggio con i pennelli) · R: inquadra");
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
        // egui's rect can be ahead of the render target for a frame after a
        // resize; wgpu rejects a viewport outside the target.
        // The camera's cached target size can lag the window by a frame too,
        // so the smaller of the two bounds the viewport.
        let surface = UVec2::new(window.physical_width(), window.physical_height());
        let target = camera
            .physical_target_size()
            .map_or(surface, |t| t.min(surface))
            .max(UVec2::splat(2));
        let position = UVec2::new((rect.min.x * scale) as u32, (rect.min.y * scale) as u32)
            .min(target - UVec2::ONE);
        let size = UVec2::new(
            (rect.width() * scale).max(1.) as u32,
            (rect.height() * scale).max(1.) as u32,
        )
        .min(target - position)
        .max(UVec2::ONE);
        camera.viewport = Some(bevy::render::camera::Viewport {
            physical_position: position,
            physical_size: size,
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
const BUILDING_KINDS: [(&str, &str); 7] = [
    ("house", "Abitazione"),
    ("school", "Scuola"),
    ("hospital", "Ospedale"),
    ("fuel_station", "Distributore"),
    ("sports_centre", "Centro sportivo"),
    ("town_hall", "Municipio"),
    ("industrial", "Industria"),
];
fn kind_label(kind: &scenario::BuildingKind) -> &'static str {
    let id = serde_json::to_value(kind).ok();
    BUILDING_KINDS
        .iter()
        .find(|(k, _)| id.as_ref().and_then(|v| v.as_str()) == Some(*k))
        .map_or("?", |(_, l)| l)
}
fn visual_label(kind: &Option<String>) -> &str {
    kind.as_deref().map_or("standard", scenario::editor::visual_label)
}
fn kind_combo(ui: &mut egui::Ui, id: &str, kind: &mut scenario::BuildingKind) {
    egui::ComboBox::from_id_source(id)
        .selected_text(kind_label(kind))
        .show_ui(ui, |ui| {
            for (k, label) in BUILDING_KINDS {
                if let Ok(v) = serde_json::from_value::<scenario::BuildingKind>(k.into()) {
                    ui.selectable_value(kind, v, label);
                }
            }
        });
}
fn visual_combo(ui: &mut egui::Ui, id: &str, visual: &mut Option<String>) {
    egui::ComboBox::from_id_source(id)
        .selected_text(visual_label(visual))
        .show_ui(ui, |ui| {
            ui.selectable_value(visual, None, "standard");
            for k in VISUAL_KINDS {
                ui.selectable_value(visual, Some(k.to_string()), scenario::editor::visual_label(k));
            }
        });
}
fn building_spec_fields(ui: &mut egui::Ui, spec: &mut BuildingSpec) {
    egui::ComboBox::from_label("Modello")
        .selected_text("Scegli struttura…")
        .show_ui(ui, |ui| {
            for (label, kind, visual, size, occupants) in FACILITY_PRESETS {
                if ui.selectable_label(false, *label).clicked() {
                    spec.kind = *kind;
                    spec.visual_kind = visual.map(str::to_owned);
                    spec.size = *size;
                    spec.occupants = *occupants;
                }
            }
        });
    ui.horizontal(|ui| {
        ui.label("Tipo");
        kind_combo(ui, "spec-kind", &mut spec.kind);
    });
    ui.horizontal(|ui| {
        ui.label("Variante");
        visual_combo(ui, "spec-visual", &mut spec.visual_kind);
    });
    ui.horizontal(|ui| {
        ui.label("Dimensioni m");
        ui.add(egui::DragValue::new(&mut spec.size[0]).range(3. ..=400.));
        ui.add(egui::DragValue::new(&mut spec.size[1]).range(3. ..=400.));
    });
    ui.add(egui::Slider::new(&mut spec.rotation_deg, 0. ..=359.).text("Rotazione °"));
    ui.add(egui::DragValue::new(&mut spec.occupants).prefix("Occupanti "));
}
/// Typed editors for the fields JSON text boxes handle badly (enums, optional
/// fields, references). They edit the JSON that «Applica» deserialises.
fn typed_fields(ui: &mut egui::Ui, selection: Selection, v: &mut serde_json::Value, s: &Scenario) {
    use serde_json::Value;
    let Some(obj) = v.as_object_mut() else { return };
    match selection {
        Selection::Building(_) => {
            let mut kind = obj
                .get("kind")
                .and_then(|k| serde_json::from_value(k.clone()).ok())
                .unwrap_or(scenario::BuildingKind::House);
            let mut visual = obj.get("visual_kind").and_then(|k| k.as_str()).map(str::to_owned);
            let mut rotation = obj.get("rotation_deg").and_then(Value::as_f64).unwrap_or(0.) as f32;
            ui.horizontal(|ui| {
                ui.label("Tipo");
                kind_combo(ui, "sel-kind", &mut kind);
            });
            ui.horizontal(|ui| {
                ui.label("Variante");
                visual_combo(ui, "sel-visual", &mut visual);
            });
            ui.add(egui::Slider::new(&mut rotation, 0. ..=359.).text("Rotazione °"));
            ui.small("Q/E ruotano l’edificio selezionato di 15° (poi «Applica»).");
            obj.insert("kind".into(), serde_json::to_value(kind).unwrap_or_default());
            match visual {
                Some(k) => obj.insert("visual_kind".into(), k.into()),
                None => obj.remove("visual_kind"),
            };
            obj.insert("rotation_deg".into(), Value::from(rotation));
            ui.separator();
        }
        Selection::Place(_) => {
            let mut radius = obj.get("radius_m").and_then(Value::as_f64).map(|r| r as f32);
            let mut area = radius.is_some();
            if ui.checkbox(&mut area, "Area associata (cerchio)").changed() {
                radius = area.then_some(radius.unwrap_or(150.));
            }
            if let Some(r) = radius.as_mut() {
                ui.add(egui::Slider::new(r, 20. ..=3000.).text("Raggio m"));
            }
            let at: Option<scenario::Pos> = obj
                .get("pos")
                .and_then(|p| serde_json::from_value(p.clone()).ok());
            let mut buildings: Vec<u32> = obj
                .get("buildings")
                .and_then(|b| serde_json::from_value(b.clone()).ok())
                .unwrap_or_default();
            let mut roads: Vec<String> = obj
                .get("roads")
                .and_then(|b| serde_json::from_value(b.clone()).ok())
                .unwrap_or_default();
            if let (Some(at), Some(r)) = (at, radius) {
                if ui.button("Associa ciò che cade nell’area").clicked() {
                    buildings = s
                        .buildings
                        .iter()
                        .filter(|b| scenario::editor::distance(b.pos, at) <= r)
                        .map(|b| b.id)
                        .collect();
                    roads = s
                        .roads
                        .iter()
                        .filter(|rd| rd.points.iter().any(|p| scenario::editor::distance(*p, at) <= r))
                        .map(|rd| rd.id.clone())
                        .collect();
                }
            }
            ui.collapsing(format!("Edifici associati ({})", buildings.len()), |ui| {
                for b in &s.buildings {
                    let mut on = buildings.contains(&b.id);
                    let label = format!("{} · {}", b.id, b.name.as_deref().unwrap_or(""));
                    if ui.checkbox(&mut on, label).changed() {
                        buildings.retain(|id| *id != b.id);
                        if on {
                            buildings.push(b.id);
                        }
                    }
                }
            });
            ui.collapsing(format!("Strade associate ({})", roads.len()), |ui| {
                for r in &s.roads {
                    let mut on = roads.contains(&r.id);
                    if ui.checkbox(&mut on, &r.name).changed() {
                        roads.retain(|id| *id != r.id);
                        if on {
                            roads.push(r.id.clone());
                        }
                    }
                }
            });
            match radius {
                Some(r) => obj.insert("radius_m".into(), Value::from(r)),
                None => obj.remove("radius_m"),
            };
            for (k, val) in [
                ("buildings", serde_json::to_value(&buildings)),
                ("roads", serde_json::to_value(&roads)),
            ] {
                if let Ok(val) = val {
                    obj.insert(k.into(), val);
                }
            }
            ui.separator();
        }
        _ => {}
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

/// Height above the ground of outlines drawn on the terrain.
const GROUND_LIFT: f32 = 0.7;
/// Polyline that follows the terrain: long segments are split so they do not cut through hills.
fn drape(gizmos: &mut Gizmos, s: &Scenario, points: &[scenario::Pos], closed: bool, colour: Color) {
    let mut path: Vec<Vec3> = Vec::new();
    let n = points.len();
    for k in 0..if closed { n } else { n.saturating_sub(1) } {
        let (a, b) = (points[k], points[(k + 1) % n]);
        let steps = (scenario::editor::distance(a, b) / 4.).ceil().max(1.) as usize;
        for j in 0..steps {
            let f = j as f32 / steps as f32;
            path.push(crate::frame::to_bevy(s, [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], GROUND_LIFT));
        }
    }
    if let Some(last) = points.last() {
        if !closed {
            path.push(crate::frame::to_bevy(s, *last, GROUND_LIFT));
        }
    }
    if closed {
        if let Some(first) = path.first().copied() {
            path.push(first);
        }
    }
    gizmos.linestrip(path, colour);
}
fn drape_circle(gizmos: &mut Gizmos, s: &Scenario, c: scenario::Pos, r: f32, colour: Color) {
    let n = ((r * 0.5) as usize).clamp(24, 96);
    let ring: Vec<_> = (0..n)
        .map(|k| {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect();
    drape(gizmos, s, &ring, true, colour);
}
/// Footprint of a building (or of one about to be placed) with a front-edge tick on +x.
fn drape_footprint(gizmos: &mut Gizmos, s: &Scenario, at: scenario::Pos, size: [f32; 2], rot_deg: f32, colour: Color) {
    let (sin, cos) = rot_deg.to_radians().sin_cos();
    let (hx, hy) = (size[0] * 0.5, size[1] * 0.5);
    let corner = |x: f32, y: f32| [at[0] + x * cos - y * sin, at[1] + x * sin + y * cos];
    drape(gizmos, s, &[corner(-hx, -hy), corner(hx, -hy), corner(hx, hy), corner(-hx, hy)], true, colour);
    drape(gizmos, s, &[corner(0., 0.), corner(hx, 0.)], false, colour);
}

const BRUSH_TICK: f32 = 0.1;
/// Pause after the last brush tick before the scene is rebuilt in full.
const SETTLE_S: f64 = 1.2;

pub(crate) fn interact(
    mut contexts: EguiContexts,
    editor: Option<ResMut<Editor>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    buttons: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    mut wheel: EventReader<MouseWheel>,
    time: Res<Time>,
    scn: Res<crate::Scn>,
    mut gizmos: Gizmos,
) {
    let Some(mut e) = editor else { return };
    let wheel_lines: f32 = wheel
        .read()
        .map(|w| match w.unit {
            MouseScrollUnit::Line => w.y,
            MouseScrollUnit::Pixel => w.y / 50.,
        })
        .sum();
    // A brush stroke is one undo step; it ends when the button is released.
    if e.stroking && !buttons.pressed(MouseButton::Left) {
        let document = e.document.clone();
        let locked = document.try_lock();
        if let Ok(mut d) = locked {
            d.end_stroke();
            e.stroking = false;
            e.last_paint = None;
        }
    }
    // A drag is one undo step; its last position is applied when the button is released.
    if e.drag.is_some() && !buttons.pressed(MouseButton::Left) {
        let document = e.document.clone();
        let locked = document.try_lock();
        if let Ok(mut d) = locked {
            let (drag, at) = (e.drag.take(), e.drag_at.take());
            if let (Some(drag), Some(at)) = (drag, at) {
                let result = match drag {
                    Drag::Object(sel) => d.move_object(sel, at),
                    Drag::Vertex(i, v) => d.move_road_vertex(i, v, at),
                };
                outcome(&mut e, result.map_err(|err| err.to_string()));
            }
            d.end_stroke();
            let selected = e.selected;
            set_selection(&mut e, &d, selected);
        }
    }
    // Q/E rotate the building about to be placed, or the selected one.
    let turn = f32::from(keys.just_pressed(KeyCode::KeyE)) - f32::from(keys.just_pressed(KeyCode::KeyQ));
    if turn != 0. && !contexts.ctx_mut().wants_keyboard_input() {
        let step = -15. * turn; // E turns clockwise seen from above
        if e.tool == "building" {
            e.spec.rotation_deg = (e.spec.rotation_deg + step).rem_euclid(360.);
        } else if let Some(Selection::Building(i)) = e.selected {
            let document = e.document.clone();
            let locked = document.try_lock();
            if let Ok(mut d) = locked {
                let result = d.edit(|s| {
                    if let Some(b) = s.buildings.get_mut(i) {
                        b.rotation_deg = (b.rotation_deg + step).rem_euclid(360.);
                    }
                    Ok(())
                });
                outcome(&mut e, result.map_err(|err| err.to_string()));
                set_selection(&mut e, &d, Some(Selection::Building(i)));
            }
        }
    }
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
    if let Some(Selection::Building(i)) = e.selected {
        if let Some(b) = s.buildings.get(i) {
            drape_footprint(&mut gizmos, s, b.pos, b.size, b.rotation_deg, Color::srgb(1., 1., 0.));
        }
    }
    // On-map actions for the selected building or place.
    if let Some(sel @ (Selection::Building(_) | Selection::Place(_))) = e.selected {
        let (at, name) = match sel {
            Selection::Building(i) => s.buildings.get(i).map(|b| (b.pos, format!("Edificio {}", b.id))),
            Selection::Place(i) => s.places.get(i).map(|p| (p.pos, p.name.clone())),
            _ => None,
        }
        .unzip();
        let screen = at.and_then(|at| {
            camera.world_to_viewport(transform, crate::frame::to_bevy(s, at, 30.))
        });
        let mut action = None;
        if let Some(p) = screen {
            egui::Area::new(egui::Id::new("editor-object-bar"))
                .fixed_pos(egui::pos2(p.x + viewport_origin.x - 70., p.y + viewport_origin.y - 40.))
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(name.unwrap_or_default());
                            if matches!(sel, Selection::Building(_)) {
                                if ui.button("⟲").on_hover_text("Ruota −15° (Q)").clicked() {
                                    action = Some("ccw");
                                }
                                if ui.button("⟳").on_hover_text("Ruota +15° (E)").clicked() {
                                    action = Some("cw");
                                }
                            }
                            if ui.button("🗑").on_hover_text("Elimina (Canc)").clicked() {
                                action = Some("delete");
                            }
                        });
                    });
                });
        }
        if keys.just_pressed(KeyCode::Delete) && !ctx.wants_keyboard_input() {
            action = Some("delete");
        }
        if let Some(action) = action {
            let document = e.document.clone();
            let locked = document.try_lock();
            if let Ok(mut d) = locked {
                let result = match (action, sel) {
                    ("delete", _) => d.delete(sel),
                    (_, Selection::Building(i)) => {
                        let step = if action == "cw" { -15. } else { 15. };
                        d.edit(|s| {
                            if let Some(b) = s.buildings.get_mut(i) {
                                b.rotation_deg = (b.rotation_deg + step).rem_euclid(360.);
                            }
                            Ok(())
                        })
                    }
                    _ => Ok(()),
                };
                let gone = action == "delete" && result.is_ok();
                outcome(&mut e, result.map_err(|err| err.to_string()));
                set_selection(&mut e, &d, if gone { None } else { Some(sel) });
            }
        }
    }
    let selected_position = match e.selected {
        Some(Selection::Place(i)) => s.places.get(i).map(|p| (p.pos, 25.)),
        _ => None,
    };
    if let Some((at, radius)) = selected_position {
        drape_circle(&mut gizmos, s, at, radius, Color::srgb(1., 1., 0.));
    }
    if let Some(Selection::Road(i)) = e.selected {
        if let Some(road) = s.roads.get(i) {
            drape(&mut gizmos, s, &road.points, false, Color::srgb(1., 1., 0.));
            for (v, p) in road.points.iter().enumerate() {
                let end = v == 0 || v + 1 == road.points.len();
                let hot = e.drag == Some(Drag::Vertex(i, v));
                gizmos.sphere(
                    crate::frame::to_bevy(s, *p, 3.),
                    Quat::IDENTITY,
                    if hot { 9. } else { 6. },
                    if end { Color::srgb(1., 0.55, 0.1) } else { Color::srgb(1., 1., 0.) },
                );
            }
        }
    }
    for (i, p) in s.places.iter().enumerate() {
        let at = crate::frame::to_bevy(s, p.pos, 8.);
        gizmos.line(at, at + Vec3::Y * 70., Color::srgb(0.1, 0.8, 1.));
        if let Some(r) = p.radius_m {
            let selected = e.selected == Some(Selection::Place(i));
            drape_circle(&mut gizmos, s, p.pos, r, Color::srgb(0.1, 0.8, 1.).with_alpha(if selected { 0.9 } else { 0.35 }));
        }
    }
    // Members of the selected place: buildings ringed, roads traced.
    if let Some(Selection::Place(i)) = e.selected {
        if let Some(place) = s.places.get(i) {
            for b in s.buildings.iter().filter(|b| place.buildings.contains(&b.id)) {
                drape_circle(&mut gizmos, s, b.pos, b.size[0].max(b.size[1]) * 0.8, Color::srgb(0.1, 0.8, 1.));
            }
            for r in s.roads.iter().filter(|r| place.roads.contains(&r.id)) {
                drape(&mut gizmos, s, &r.points, false, Color::srgb(0.1, 0.8, 1.));
            }
        }
    }
    if let Some(start) = e.road_start {
        let p = crate::frame::to_bevy(s, start, 10.);
        gizmos.sphere(p, Quat::IDENTITY, 12., Color::srgb(1., 1., 0.));
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let ctx = contexts.ctx_mut();
    if ctx.wants_pointer_input() || ctx.pointer_latest_pos().is_some_and(|p| !ctx.available_rect().contains(p)) {
        if buttons.just_pressed(MouseButton::Left) {
            e.press = None;
        }
        return;
    }
    let command = [KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]
        .into_iter()
        .any(|k| keys.pressed(k));
    if e.brush_tool() && wheel_lines != 0. && !command {
        e.radius = (e.radius * (0.1 * wheel_lines).exp()).clamp(10., 1000.);
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
    // The rebuilt scene already shows the dabs; until then draw them directly.
    if !e.busy(time.elapsed_seconds_f64()) && !e.dabs.is_empty() {
        let document = e.document.clone();
        let locked = document.try_lock();
        if locked.is_ok_and(|d| Arc::ptr_eq(s, &d.scene)) {
            e.dabs.clear();
        }
    }
    let paint = if e.tool == "fuel" {
        Color::srgb(0.2, 0.9, 0.3).with_alpha(0.25)
    } else {
        Color::srgb(1., 1., 0.).with_alpha(0.25)
    };
    for (at, radius, lowering) in &e.dabs {
        let colour = if *lowering { Color::srgb(1., 0.25, 0.2).with_alpha(0.25) } else { paint };
        drape_circle(&mut gizmos, s, *at, *radius, colour);
    }
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if let (Some(drag), Some(at)) = (e.drag, hit) {
        e.drag_at = Some(at);
        e.stroke_clock += time.delta_seconds().min(0.1);
        if e.stroke_clock >= 0.05 {
            e.stroke_clock = 0.;
            e.busy_until = time.elapsed_seconds_f64() + SETTLE_S;
            let document = e.document.clone();
            let locked = document.try_lock();
            if let Ok(mut d) = locked {
                let result = match drag {
                    Drag::Object(sel) => d.move_object(sel, at),
                    Drag::Vertex(i, v) => d.move_road_vertex(i, v, at),
                };
                if let Err(err) = result {
                    e.message = err.to_string();
                }
            }
        }
        return;
    }
    if let Some(at) = hit {
        if e.tool == "select" {
            // Handles of the selected object: grabbing one starts a drag, otherwise a click selects.
            let grab = (transform.translation().distance(crate::frame::to_bevy(s, at, 0.)) * 0.025)
                .clamp(8., 80.);
            let handle = match e.selected {
                Some(Selection::Road(i)) => s.roads.get(i).and_then(|r| {
                    r.points
                        .iter()
                        .enumerate()
                        .map(|(v, p)| (v, scenario::editor::distance(*p, at)))
                        .filter(|(_, dist)| *dist <= grab)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(v, _)| (Drag::Vertex(i, v), v))
                }),
                _ => None,
            };
            if let (Some(Selection::Road(i)), Some((_, v))) = (e.selected, handle) {
                if keys.just_pressed(KeyCode::KeyX) && !contexts.ctx_mut().wants_keyboard_input() {
                    let document = e.document.clone();
                    let locked = document.try_lock();
                    if let Ok(mut d) = locked {
                        let result = d.remove_road_vertex(i, v);
                        outcome(&mut e, result.map_err(|err| err.to_string()));
                        set_selection(&mut e, &d, Some(Selection::Road(i)));
                    }
                    return;
                }
            }
            if buttons.just_pressed(MouseButton::Left) {
                let document = e.document.clone();
                let locked = document.try_lock();
                if let Ok(mut d) = locked {
                    let mut start = handle.map(|(drag, _)| drag);
                    if start.is_none() {
                        if let (true, Some(Selection::Road(i))) = (shift, e.selected) {
                            let nearest = s.roads.get(i).and_then(|r| {
                                r.points
                                    .windows(2)
                                    .enumerate()
                                    .map(|(k, w)| (k, scenario::editor::nearest_on_segment(at, w[0], w[1])))
                                    .filter(|(_, (dist, _))| *dist <= grab)
                                    .min_by(|a, b| a.1 .0.total_cmp(&b.1 .0))
                            });
                            if let Some((k, (_, q))) = nearest {
                                // The stroke stays open: insert and drag are one undo step.
                                d.begin_stroke();
                                match d.insert_road_vertex(i, k, q) {
                                    Ok(v) => start = Some(Drag::Vertex(i, v)),
                                    Err(err) => {
                                        d.end_stroke();
                                        e.message = err.to_string();
                                    }
                                }
                            }
                        }
                    }
                    if start.is_none() {
                        start = match e.selected {
                            Some(Selection::Building(i)) => s.buildings.get(i).and_then(|b| {
                                let (sin, cos) = b.rotation_deg.to_radians().sin_cos();
                                let (dx, dy) = (at[0] - b.pos[0], at[1] - b.pos[1]);
                                let (lx, ly) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                                (lx.abs() <= b.size[0] * 0.5 + grab * 0.5
                                    && ly.abs() <= b.size[1] * 0.5 + grab * 0.5)
                                    .then_some(Drag::Object(Selection::Building(i)))
                            }),
                            Some(Selection::Place(i)) => s
                                .places
                                .get(i)
                                .filter(|p| scenario::editor::distance(p.pos, at) <= grab.max(25.))
                                .map(|_| Drag::Object(Selection::Place(i))),
                            _ => None,
                        };
                        if start.is_some() {
                            d.begin_stroke();
                        }
                    }
                    if let Some(drag) = start {
                        e.drag = Some(drag);
                        e.drag_at = Some(at);
                        e.stroke_clock = 0.;
                        e.press = None;
                        return;
                    }
                }
            }
        }
        if e.brush_tool() {
            let lowering = e.tool == "height" && e.sculpt_mode == "push" && shift;
            let colour = if lowering {
                Color::srgb(1., 0.25, 0.2)
            } else {
                Color::srgb(1., 1., 0.)
            };
            drape_circle(&mut gizmos, s, at, e.radius, colour);
            drape_circle(&mut gizmos, s, at, e.radius * 0.5, colour.with_alpha(0.4));
            if buttons.pressed(MouseButton::Left) {
                let document = e.document.clone();
                let Ok(mut d) = document.try_lock() else {
                    return;
                };
                if !e.stroking {
                    d.begin_stroke();
                    e.stroking = true;
                    e.stroke_clock = BRUSH_TICK;
                }
                e.stroke_clock += time.delta_seconds().min(0.1);
                if e.stroke_clock >= BRUSH_TICK {
                    e.busy_until = time.elapsed_seconds_f64() + SETTLE_S;
                    let elapsed = e.stroke_clock.min(0.25);
                    e.stroke_clock = 0.;
                    let result = match (e.tool.as_str(), e.sculpt_mode.as_str()) {
                        ("height", "push") => {
                            let sign = if shift { -1. } else { 1. };
                            d.sculpt(at, e.radius, sign * e.strength * elapsed)
                        }
                        ("height", _) => d.brush(at, e.radius, Some(e.elevation), None),
                        _ => {
                            // Painting is idempotent: skip until the brush has moved.
                            if e.last_paint.is_some_and(|p| {
                                scenario::editor::distance(p, at) < e.radius * 0.2
                            }) {
                                return;
                            }
                            e.last_paint = Some(at);
                            d.brush(at, e.radius, None, Some(e.fuel))
                        }
                    };
                    match result {
                        Ok(()) => {
                            let dab = (at, e.radius, lowering);
                            e.dabs.push(dab);
                            if e.dabs.len() > 300 {
                                e.dabs.remove(0);
                            }
                        }
                        Err(err) => e.message = err.to_string(),
                    }
                }
            }
            return;
        }
        if e.tool == "building" {
            drape_footprint(&mut gizmos, s, at, e.spec.size, e.spec.rotation_deg, Color::srgb(0.3, 1., 0.4));
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
                        // Inside the (rotated) footprint counts as a direct hit.
                        let (sin, cos) = b.rotation_deg.to_radians().sin_cos();
                        let (dx, dy) = (at[0] - b.pos[0], at[1] - b.pos[1]);
                        let (lx, ly) = (dx * cos + dy * sin, -dx * sin + dy * cos);
                        let inside = lx.abs() <= b.size[0] * 0.5 && ly.abs() <= b.size[1] * 0.5;
                        objects.push((
                            if inside { 0. } else { scenario::editor::distance(at, b.pos) },
                            Selection::Building(i),
                        ));
                    }
                    for (i, p) in s.places.iter().enumerate() {
                        objects.push((scenario::editor::distance(at, p.pos), Selection::Place(i)));
                    }
                    // A road is picked by its centreline, within a narrower reach than a point object.
                    for (i, r) in s.roads.iter().enumerate() {
                        let dist = r
                            .points
                            .windows(2)
                            .map(|w| scenario::editor::nearest_on_segment(at, w[0], w[1]).0)
                            .fold(f32::INFINITY, f32::min);
                        if dist < 25. {
                            objects.push((dist.max(0.) * 3., Selection::Road(i)));
                        }
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
                "building" => d
                    .add_building(at, e.spec.clone())
                    .map(|sel| set_selection(&mut e, &d, Some(sel))),
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
