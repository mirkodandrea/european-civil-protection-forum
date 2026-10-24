use futures::future::AbortHandle;
#[cfg(not(feature = "standalone"))]
use futures::StreamExt;
#[cfg(not(feature = "standalone"))]
use gloo_net::{http::Request, websocket::futures::WebSocket};
use gloo_timers::future::TimeoutFuture;
use serde_json::Value;
use wasm_bindgen::JsValue;

pub fn window() -> web_sys::Window {
    web_sys::window().expect("browser window")
}
pub fn text(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}
pub fn list(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}
pub fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
pub fn encode(v: &str) -> String {
    js_sys::encode_uri_component(v).into()
}
pub fn input(e: web_sys::InputEvent) -> String {
    use yew::TargetCast;
    if let Some(el) = e.target_dyn_into::<web_sys::HtmlTextAreaElement>() {
        el.value()
    } else {
        e.target_unchecked_into::<web_sys::HtmlInputElement>()
            .value()
    }
}
pub fn select(e: web_sys::Event) -> String {
    use yew::TargetCast;
    e.target_unchecked_into::<web_sys::HtmlSelectElement>()
        .value()
}
#[cfg(not(feature = "standalone"))]
pub fn token() -> String {
    let w = window();
    let url = web_sys::Url::new(&w.location().href().unwrap_or_default()).unwrap();
    let params = url.search_params();
    let context = params
        .get("role")
        .or_else(|| params.get("evaluator"))
        .unwrap_or("master".into());
    // No secret: the server takes the identity itself as the bearer value.
    context
}
#[cfg(not(feature = "standalone"))]
pub async fn request(token: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let req = if let Some(body) = body {
        Request::post(path)
            .header("Authorization", &format!("Bearer {token}"))
            .json(&body)
            .map_err(|e| e.to_string())?
            .send()
            .await
    } else {
        Request::get(path)
            .header("Authorization", &format!("Bearer {token}"))
            .send()
            .await
    };
    let response = req.map_err(|e| format!("Connessione non disponibile: {e}"))?;
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|_| format!("Risposta non valida ({status})"))?;
    if response.ok() {
        Ok(value)
    } else {
        Err(value["error"]
            .as_str()
            .unwrap_or("Richiesta rifiutata")
            .into())
    }
}
#[cfg(not(feature = "standalone"))]
pub fn live(token: String, callback: yew::Callback<()>) -> AbortHandle {
    let (handle, registration) = AbortHandle::new_pair();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = futures::future::Abortable::new(
            async move {
                let loc = window().location();
                let protocol = if loc.protocol().unwrap_or_default() == "https:" {
                    "wss"
                } else {
                    "ws"
                };
                let url = format!(
                    "{protocol}://{}/ws?token={}",
                    loc.host().unwrap_or_default(),
                    encode(&token)
                );
                loop {
                    if let Ok(mut socket) = WebSocket::open(&url) {
                        while let Some(Ok(_)) = socket.next().await {
                            callback.emit(());
                        }
                    }
                    TimeoutFuture::new(1500).await;
                }
            },
            registration,
        )
        .await;
    });
    handle
}
pub fn download(value: &Value) -> Result<(), String> {
    download_as(value, "ttx-debrief.json")
}
pub fn download_as(value: &Value, filename: &str) -> Result<(), String> {
    use wasm_bindgen::JsCast;
    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(&pretty(value)));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/json");
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &options)
        .map_err(|_| "Export non disponibile")?;
    let url =
        web_sys::Url::create_object_url_with_blob(&blob).map_err(|_| "Export non disponibile")?;
    let anchor = window()
        .document()
        .unwrap()
        .create_element("a")
        .unwrap()
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .unwrap();
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();
    let _ = web_sys::Url::revoke_object_url(&url);
    Ok(())
}
/// EXCON console languages: a subset of the player languages.
pub const EXCON_LANGUAGES: &[(&str, &str)] = &[("it", "Italiano"), ("en", "English")];
pub const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("it", "Italiano"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("hy", "Հայերեն"),
    ("ka", "ქართული"),
    ("uk", "Українська"),
    ("ro", "Română"),
    ("az", "Azərbaycan"),
    ("el", "Ελληνικά"),
    ("pt", "Português"),
];
pub fn status(s: &str) -> &str {
    crate::i18n::status("it", s)
}

#[cfg(feature = "standalone")]
pub async fn request(role: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    let state = crate::standalone::runtime()?;
    let changes = body.is_some();
    let result = server::browser::request(state.clone(), role, path, body).await;
    if changes && result.is_ok() {
        crate::store::autosave(&state, false);
    }
    result
}
#[cfg(feature = "standalone")]
pub fn live(_token: String, callback: yew::Callback<()>) -> AbortHandle {
    let (handle, registration) = AbortHandle::new_pair();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = futures::future::Abortable::new(
            async move {
                let mut last = u64::MAX;
                loop {
                    if let Ok(state) = crate::standalone::runtime() {
                        let generation = state.lock().generation;
                        if generation != last {
                            last = generation;
                            callback.emit(());
                        }
                    }
                    TimeoutFuture::new(250).await;
                }
            },
            registration,
        )
        .await;
    });
    handle
}
