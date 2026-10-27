//! Dedicated standalone route: no Exercise or live simulation is initialised.
use scenario::Scenario;
use server::library::ScenarioLibrary;
struct BrowserStorage;
impl viz::editor::Storage for BrowserStorage {
    fn list(&self) -> Result<Vec<Scenario>, String> {
        let library = crate::store::BrowserLibrary;
        let mut out = vec![];
        for info in library.list() {
            if let Some(pack) = library.load(&info.id) {
                let base = crate::store::authoring_base(&pack.base)?;
                let mut s = pack.build(base).map_err(|e| e.to_string())?;
                s.id = pack.id;
                s.name = pack.name;
                out.push(s);
            }
        }
        Ok(out)
    }
    fn download(&self, json: &str, id: &str) -> Result<String, String> {
        use wasm_bindgen::JsCast;
        let parts = js_sys::Array::new();
        parts.push(&wasm_bindgen::JsValue::from_str(json));
        let options = web_sys::BlobPropertyBag::new();
        options.set_type("application/json");
        let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)
            .map_err(|e| format!("{e:?}"))?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(|e| format!("{e:?}"))?;
        let document = crate::api::window()
            .document()
            .ok_or("Documento non disponibile")?;
        let anchor = document
            .create_element("a")
            .map_err(|e| format!("{e:?}"))?
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .map_err(|_| "Link non disponibile")?;
        anchor.set_href(&url);
        anchor.set_download(&format!("{id}.json"));
        anchor.click();
        let _ = web_sys::Url::revoke_object_url(&url);
        Ok("Download JSON avviato".into())
    }
    fn save(&self, s: &Scenario) -> Result<(), String> {
        crate::store::save_territory(s)
    }
}
pub fn start() {
    let window = crate::api::window();
    let document = window.document().unwrap();
    use wasm_bindgen::JsCast;
    let listener = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::ClipboardEvent)>::new(
        |event: web_sys::ClipboardEvent| {
            if let Some(data) = event.clipboard_data() {
                if let Ok(text) = data.get_data("text/plain") {
                    viz::editor::paste(text);
                    event.prevent_default();
                }
            }
        },
    );
    window
        .add_event_listener_with_callback("paste", listener.as_ref().unchecked_ref())
        .unwrap();
    listener.forget();
    document.body().unwrap().set_inner_html("<a href='?' style='position:fixed;right:12px;top:55px;z-index:9;background:#16212a;color:white;padding:8px'>Torna al TTX</a><div style='position:fixed;inset:0'><canvas id='standalone-3d' aria-label='Editor scenario 3D' style='width:100%;height:100%'></canvas></div>");
    let result = scenario::editor::blank("nuovo-scenario", 64, 20.)
        .map_err(|e| e.to_string())
        .and_then(|s| viz::editor::run(s, Box::new(BrowserStorage)));
    if let Err(e) = result {
        document
            .body()
            .unwrap()
            .set_text_content(Some(&format!("Impossibile aprire l’editor: {e}")));
    }
}
