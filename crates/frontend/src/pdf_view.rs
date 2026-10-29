//! EXCON viewer of the PDF a player attached to a response.
//!
//! The bytes come from `GET /api/attachments/<id>` (JSON, base64) in both the
//! server and the standalone build. They are drawn with pdf.js, vendored in
//! `assets/vendor/pdfjs` and embedded in the WASM (offline and single-file
//! standalone included), turned into module/worker blob URLs on first use. The
//! PDF also stays reachable through a blob URL that the browser's own viewer
//! opens or downloads.
use crate::api::{self, text};
use base64::Engine;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use yew::prelude::*;

/// Vendored pdfjs-dist 4.10.38 (see `assets/vendor/pdfjs/VERSION`).
const PDFJS_LIB: &str = include_str!("../assets/vendor/pdfjs/pdf.min.mjs");
const PDFJS_WORKER: &str = include_str!("../assets/vendor/pdfjs/pdf.worker.min.mjs");

/// Draws `bytes` into `host`, one canvas per page (capped), with pdf.js.
const RENDER_JS: &str = r#"
return (async () => {
  window.__ttxPdfjs ??= (async () => {
    const url = code => URL.createObjectURL(new Blob([code], { type: 'text/javascript' }));
    const lib = await import(url(libCode));
    lib.GlobalWorkerOptions.workerSrc = url(workerCode);
    return lib;
  })();
  let lib;
  try { lib = await window.__ttxPdfjs; } catch (e) { delete window.__ttxPdfjs; throw e; }
  const doc = await lib.getDocument({ data: bytes }).promise;
  host.replaceChildren();
  const shown = Math.min(doc.numPages, 60);
  const width = Math.max(host.clientWidth - 8, 320);
  const ratio = window.devicePixelRatio || 1;
  for (let i = 1; i <= shown; i++) {
    const page = await doc.getPage(i);
    const fit = width / page.getViewport({ scale: 1 }).width;
    const view = page.getViewport({ scale: fit * ratio });
    const canvas = document.createElement('canvas');
    canvas.className = 'pdf-page';
    canvas.width = view.width;
    canvas.height = view.height;
    canvas.style.width = '100%';
    host.appendChild(canvas);
    await page.render({ canvasContext: canvas.getContext('2d'), viewport: view }).promise;
  }
  return doc.numPages;
})();
"#;

async fn render(host: &web_sys::Element, bytes: &[u8]) -> Result<(), String> {
    let f = js_sys::Function::new_with_args("host, bytes, libCode, workerCode", RENDER_JS);
    let array = js_sys::Uint8Array::from(bytes);
    let args = js_sys::Array::of4(host, &array, &JsValue::from_str(PDFJS_LIB), &JsValue::from_str(PDFJS_WORKER));
    let promise = f
        .apply(&JsValue::NULL, &args)
        .map_err(|e| format!("{e:?}"))?;
    JsFuture::from(js_sys::Promise::from(promise))
        .await
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

fn blob_url(bytes: &[u8]) -> Option<String> {
    let array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::of1(&array);
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/pdf");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).ok()?;
    web_sys::Url::create_object_url_with_blob(&blob).ok()
}

#[derive(Clone, PartialEq)]
enum Load {
    Loading,
    /// Blob URL of the PDF; `drawn` says whether pdf.js could show it.
    Ready { url: String, drawn: bool },
    Missing,
}

#[derive(Properties, PartialEq)]
pub struct PdfProps {
    pub token: String,
    pub id: u64,
    pub name: String,
    pub onclose: Callback<()>,
}

#[function_component(PdfViewer)]
pub fn pdf_viewer(p: &PdfProps) -> Html {
    let host = use_node_ref();
    let load = use_state(|| Load::Loading);
    {
        let (host, load, token) = (host.clone(), load.clone(), p.token.clone());
        use_effect_with(p.id, move |id| {
            let id = *id;
            let cancelled = std::rc::Rc::new(std::cell::Cell::new(false));
            let flag = cancelled.clone();
            load.set(Load::Loading);
            spawn_local(async move {
                let bytes = match api::request(&token, &format!("/api/attachments/{id}"), None).await {
                    Ok(v) => base64::engine::general_purpose::STANDARD.decode(text(&v["data"])).ok(),
                    Err(_) => None,
                };
                let Some(bytes) = bytes else {
                    if !flag.get() {
                        load.set(Load::Missing);
                    }
                    return;
                };
                let Some(url) = blob_url(&bytes) else {
                    load.set(Load::Missing);
                    return;
                };
                if flag.get() {
                    let _ = web_sys::Url::revoke_object_url(&url);
                    return;
                }
                load.set(Load::Ready { url: url.clone(), drawn: true });
                // The canvas host exists once the state above has rendered.
                gloo_timers::future::TimeoutFuture::new(0).await;
                let drawn = match host.cast::<web_sys::Element>() {
                    Some(el) => render(&el, &bytes).await.is_ok(),
                    None => false,
                };
                if !flag.get() && !drawn {
                    load.set(Load::Ready { url, drawn: false });
                }
            });
            move || cancelled.set(true)
        });
    }
    let ex = crate::i18n::ex;
    let ready = match &*load {
        Load::Ready { url, drawn } => Some((url.clone(), *drawn)),
        _ => None,
    };
    html! {<div class="pdf-viewer" data-pdf-viewer={p.id.to_string()}>
        <div class="row pdf-bar"><b class="pdf-name">{p.name.clone()}</b><div class="grow"/>
            if let Some((url,_))=&ready{
                <a class="action" href={url.clone()} target="_blank" rel="noopener noreferrer">{ex("Apri in una scheda")}</a>
                <a class="action" href={url.clone()} download={p.name.clone()}>{ex("Scarica")}</a>
            }
            <button onclick={{let c=p.onclose.clone();Callback::from(move |_|c.emit(()))}}>{ex("Chiudi PDF")}</button></div>
        if *load==Load::Loading{<p class="muted" role="status">{ex("Caricamento del PDF…")}</p>}
        if *load==Load::Missing{<p class="error" role="alert">{ex("Documento non disponibile.")}</p>}
        if matches!(ready,Some((_,false))){<p class="error" role="alert" data-pdf-fallback="">{ex("Impossibile mostrare il PDF qui (visualizzatore non raggiungibile?). Aprilo in una scheda o scaricalo.")}</p>}
        <div class="pdf-pages" ref={host}/>
    </div>}
}
