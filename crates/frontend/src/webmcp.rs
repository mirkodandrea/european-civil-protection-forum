//! WebMCP tools for editing scenarios: an agent in the browser (ChatGPT Atlas,
//! Chrome with WebMCP, …) reads the library and creates or changes scenarios
//! through `navigator.modelContext`.
//!
//! The tools go through the same EXCON API as the Scenario view
//! (`scenario_editor::call`), so they behave alike in the server app and in the
//! standalone WASM app, and every change is validated by the scenario crate
//! before it is stored. Nothing here touches the running exercise except
//! `start_scenario`, which needs an explicit `confirm`.
//!
//! Without `navigator.modelContext` the tools are still reachable as
//! `window.ttxWebMcp.call(name, args)` (tests, bookmarklets, other agents).
use crate::scenario_editor::call;
use futures::{future::LocalBoxFuture, FutureExt};
use js_sys::{Array, Function, Object, Promise, Reflect};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::HashSet};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};

thread_local! {
    /// Scenario the editor should (re)load; read by the Scenario view.
    static EDITOR_REQUEST: RefCell<Option<EditorRequest>> = const { RefCell::new(None) };
    static REGISTERED: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
}

/// What a tool asks of the Scenario view when it is open.
pub struct EditorRequest {
    pub id: String,
    /// `true` replaces whatever is open; `false` only refreshes the same,
    /// unmodified scenario.
    pub force: bool,
}

pub const CHANGED_EVENT: &str = "ttx-scenarios-changed";

pub fn take_editor_request() -> Option<EditorRequest> {
    EDITOR_REQUEST.with(|r| r.borrow_mut().take())
}

fn notify(id: &str, force: bool) {
    EDITOR_REQUEST.with(|r| *r.borrow_mut() = Some(EditorRequest { id: id.into(), force }));
    if let Ok(event) = web_sys::Event::new(CHANGED_EVENT) {
        let _ = crate::api::window().dispatch_event(&event);
    }
}

type Handler = fn(Value) -> LocalBoxFuture<'static, Result<Value, String>>;

struct Tool {
    name: &'static str,
    description: &'static str,
    schema: Value,
    read_only: bool,
    run: Handler,
}

const FORMAT_GUIDE: &str = "\
A scenario is a PACK over a read-only TERRITORY (map, buildings, roads). The pack JSON has: \
id (a-z 0-9 _ -, max 64), name, description, base (territory id), edits, run.\n\
`edits` holds what a designer controls (a field left out keeps the territory's value): \
name/start time/weather, turn_minutes (1-1440, default 30), resource types, roles (id, name, resources, allowed actions), \
reinforcement offers, places, and `injects` (the MSEL).\n\
Each inject has id, title/text, recipients, a suggested turn or an explicit `at_s` (simulated seconds from start) and `actions`: \
{id (unique in the whole scenario), title, command, offset_s, note}. `command` is an EXCON command such as \
{\"type\":\"SetWeather\",\"wind_dir_deg\":270} or {\"type\":\"Ignite\",...}. The fire starts from an `Ignite` action of an inject \
(ignitions of at least 40 m; there are no initial ignitions).\n\
Places can be named places of the territory, 8x8 grid zone codes such as F4, zone edges such as F4:N, or coordinates.\n\
Workflow: list_scenarios -> get_scenario (full example, plus the territory's names under `reference`) -> create_scenario \
(copy of an existing one, since built-in scenarios cannot be overwritten) -> edit_scenario with JSON-pointer operations \
-> validate (edit_scenario validates and refuses invalid changes, saving nothing) -> optionally open_in_editor. \
Never start a scenario unless the user asked for it: it discards the exercise in progress.";

fn id_schema(extra: Value) -> Value {
    let mut properties = json!({"id": {"type": "string", "description": "Scenario id (a-z 0-9 _ -)"}});
    if let (Some(p), Some(e)) = (properties.as_object_mut(), extra.as_object()) {
        p.extend(e.clone());
    }
    json!({"type": "object", "properties": properties, "required": ["id"]})
}

fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "describe_scenario_format",
            description: "Explains how scenarios are structured and the recommended workflow for creating or editing one. Call this first.",
            schema: json!({"type": "object", "properties": {}}),
            read_only: true,
            run: |_| async { Ok(json!({"guide": FORMAT_GUIDE})) }.boxed_local(),
        },
        Tool {
            name: "list_scenarios",
            description: "Lists the scenario library: id, name, territory (base), whether it is built in (not overwritable), and which one is running.",
            schema: json!({"type": "object", "properties": {}}),
            read_only: true,
            run: |_| list_scenarios().boxed_local(),
        },
        Tool {
            name: "get_scenario",
            description: "Returns one scenario as a full pack (every editable value filled in) with its current problems, and `reference`: the names of the territory (places, zones) to use in edits.",
            schema: id_schema(json!({})),
            read_only: true,
            run: |a| get_scenario(a).boxed_local(),
        },
        Tool {
            name: "validate_scenario",
            description: "Checks a complete pack without saving it. Returns every problem found (empty list = valid).",
            schema: json!({"type": "object", "properties": {"pack": {"type": "object", "description": "A complete scenario pack"}}, "required": ["pack"]}),
            read_only: true,
            run: |a| validate_scenario(a).boxed_local(),
        },
        Tool {
            name: "create_scenario",
            description: "Creates a new scenario in the library as a copy of an existing one (default: a scenario on `base`, or the first available). Then change it with edit_scenario.",
            schema: json!({"type": "object", "properties": {
                "id": {"type": "string", "description": "New id (a-z 0-9 _ -, max 64)"},
                "name": {"type": "string"},
                "description": {"type": "string"},
                "copy_from": {"type": "string", "description": "Id of the scenario to copy (built-ins allowed)"},
                "base": {"type": "string", "description": "Territory id, when copy_from is not given"}
            }, "required": ["id", "name"]}),
            read_only: false,
            run: |a| create_scenario(a).boxed_local(),
        },
        Tool {
            name: "edit_scenario",
            description: "Applies JSON-pointer operations to a saved, non-built-in scenario, validates the result and saves it. All-or-nothing: on any problem nothing is saved and the problems are returned. Paths start at the pack root, e.g. /name, /edits/turn_minutes, /edits/injects/- (append). Ops: add (inserts into arrays, '-' appends), replace (must exist), remove.",
            schema: id_schema(json!({
                "operations": {"type": "array", "items": {"type": "object", "properties": {
                    "op": {"type": "string", "enum": ["add", "replace", "remove"]},
                    "path": {"type": "string", "description": "JSON pointer from the pack root"},
                    "value": {"description": "New value (not for remove)"}
                }, "required": ["op", "path"]}},
                "dry_run": {"type": "boolean", "description": "Validate only, do not save"}
            })).tap(|s| s["required"] = json!(["id", "operations"])),
            read_only: false,
            run: |a| edit_scenario(a).boxed_local(),
        },
        Tool {
            name: "delete_scenario",
            description: "Permanently deletes a scenario from the library (not built-in, not running). Requires confirm=true after the user agreed.",
            schema: id_schema(json!({"confirm": {"type": "boolean"}})),
            read_only: false,
            run: |a| delete_scenario(a).boxed_local(),
        },
        Tool {
            name: "open_in_editor",
            description: "Shows a scenario in the Scenario view of the console so the user can follow or continue by hand (works when that view is open).",
            schema: id_schema(json!({})),
            read_only: false,
            run: |a| open_in_editor(a).boxed_local(),
        },
        Tool {
            name: "start_scenario",
            description: "Starts a NEW exercise from a saved scenario and DISCARDS the exercise in progress (turns, messages, decisions). Use only on the user's explicit request, with confirm=true.",
            schema: id_schema(json!({"confirm": {"type": "boolean"}})),
            read_only: false,
            run: |a| start_scenario(a).boxed_local(),
        },
    ]
}

trait Tap: Sized {
    fn tap(self, f: impl FnOnce(&mut Self)) -> Self;
}
impl Tap for Value {
    fn tap(mut self, f: impl FnOnce(&mut Self)) -> Self {
        f(&mut self);
        self
    }
}

fn arg_str(args: &Value, key: &str) -> Result<String, String> {
    args[key]
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing argument `{key}`"))
}

fn scenario_path(id: &str) -> String {
    format!("/api/master/scenarios/{}", crate::api::encode(id))
}

async fn list_scenarios() -> Result<Value, String> {
    let lib = call("/api/master/scenarios", None).await?;
    let running = lib["running"].clone();
    let packs: Vec<Value> = crate::api::list(&lib["packs"])
        .iter()
        .map(|p| {
            json!({
                "id": p["id"], "name": p["name"], "description": p["description"],
                "base": p["base"], "builtin": p["builtin"], "running": p["id"] == running,
            })
        })
        .collect();
    Ok(json!({"scenarios": packs, "territories": lib["bases"]}))
}

async fn get_scenario(args: Value) -> Result<Value, String> {
    let id = arg_str(&args, "id")?;
    let v = call(&scenario_path(&id), None).await?;
    let problems = validate(&v["pack"]).await.unwrap_or_default();
    Ok(json!({"pack": v["pack"], "builtin": v["builtin"], "problems": problems, "reference": v["reference"]}))
}

async fn validate(pack: &Value) -> Result<Vec<String>, String> {
    let v = call("/api/master/scenarios/validate", Some(json!({"pack": pack}))).await?;
    Ok(crate::api::list(&v["problems"]).iter().map(|p| crate::api::text(p).to_string()).collect())
}

async fn validate_scenario(args: Value) -> Result<Value, String> {
    if !args["pack"].is_object() {
        return Err("missing argument `pack`".into());
    }
    let problems = validate(&args["pack"]).await?;
    Ok(json!({"valid": problems.is_empty(), "problems": problems}))
}

async fn save(pack: &Value) -> Result<(), String> {
    call("/api/master/scenarios/save", Some(json!({"pack": pack}))).await.map(|_| ())
}

async fn create_scenario(args: Value) -> Result<Value, String> {
    let id = arg_str(&args, "id")?;
    let name = arg_str(&args, "name")?;
    let lib = call("/api/master/scenarios", None).await?;
    let packs = crate::api::list(&lib["packs"]);
    if packs.iter().any(|p| p["id"] == id.as_str()) {
        return Err(format!("scenario `{id}` already exists: choose another id"));
    }
    let source = match args["copy_from"].as_str().filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => {
            let base = args["base"].as_str().filter(|s| !s.is_empty());
            packs
                .iter()
                .find(|p| base.is_none_or(|b| p["base"] == b))
                .map(|p| crate::api::text(&p["id"]).to_string())
                .ok_or("no scenario available to copy: pass `copy_from`")?
        }
    };
    let mut pack = call(&scenario_path(&source), None).await?["pack"].clone();
    pack["id"] = json!(id);
    pack["name"] = json!(name);
    if let Some(d) = args["description"].as_str() {
        pack["description"] = json!(d);
    }
    let problems = validate(&pack).await?;
    if !problems.is_empty() {
        return Err(problems.join("\n"));
    }
    save(&pack).await?;
    notify(&id, false);
    Ok(json!({"created": id, "copied_from": source, "next": "use edit_scenario to change it"}))
}

/// RFC 6901 pointer split, unescaping `~1` and `~0`.
fn tokens(path: &str) -> Result<Vec<String>, String> {
    if path.is_empty() {
        return Err("the whole pack cannot be replaced: use a deeper path".into());
    }
    let rest = path.strip_prefix('/').ok_or_else(|| format!("path `{path}` must start with `/`"))?;
    Ok(rest.split('/').map(|t| t.replace("~1", "/").replace("~0", "~")).collect())
}

fn index(token: &str, len: usize, allow_end: bool) -> Result<usize, String> {
    if token == "-" && allow_end {
        return Ok(len);
    }
    let i: usize = token.parse().map_err(|_| format!("`{token}` is not an array index"))?;
    if i > len || (i == len && !allow_end) {
        return Err(format!("index {i} out of range (length {len})"));
    }
    Ok(i)
}

/// Applies one `add` / `replace` / `remove` operation in place.
pub fn apply_op(doc: &mut Value, op: &str, path: &str, value: Option<Value>) -> Result<(), String> {
    let tokens = tokens(path)?;
    let (last, parents) = tokens.split_last().expect("non-empty");
    let mut cur = doc;
    for t in parents {
        cur = match cur {
            Value::Object(m) => m.get_mut(t).ok_or_else(|| format!("`{t}` not found in `{path}`"))?,
            Value::Array(a) => {
                let i = index(t, a.len(), false).map_err(|e| format!("{e} in `{path}`"))?;
                &mut a[i]
            }
            _ => return Err(format!("`{t}` in `{path}` is not inside an object or array")),
        };
    }
    let need = || value.clone().ok_or_else(|| format!("`{op}` needs a `value`"));
    match (op, cur) {
        ("add", Value::Object(m)) => {
            m.insert(last.clone(), need()?);
        }
        ("replace", Value::Object(m)) => {
            let slot = m.get_mut(last).ok_or_else(|| format!("`{path}` does not exist: use add"))?;
            *slot = need()?;
        }
        ("remove", Value::Object(m)) => {
            m.remove(last).ok_or_else(|| format!("`{path}` does not exist"))?;
        }
        ("add", Value::Array(a)) => {
            let i = index(last, a.len(), true)?;
            a.insert(i, need()?);
        }
        ("replace", Value::Array(a)) => {
            let i = index(last, a.len(), false)?;
            a[i] = need()?;
        }
        ("remove", Value::Array(a)) => {
            let i = index(last, a.len(), false)?;
            a.remove(i);
        }
        ("add" | "replace" | "remove", _) => return Err(format!("`{path}` is not inside an object or array")),
        _ => return Err(format!("unknown op `{op}` (use add, replace or remove)")),
    }
    Ok(())
}

async fn edit_scenario(args: Value) -> Result<Value, String> {
    let id = arg_str(&args, "id")?;
    let ops = args["operations"].as_array().ok_or("missing argument `operations`")?;
    let loaded = call(&scenario_path(&id), None).await?;
    if loaded["builtin"] == true {
        return Err("built-in scenarios cannot be overwritten: create_scenario with copy_from first".into());
    }
    let mut pack = loaded["pack"].clone();
    for (n, o) in ops.iter().enumerate() {
        let op = o["op"].as_str().unwrap_or("");
        let path = o["path"].as_str().unwrap_or("");
        apply_op(&mut pack, op, path, o.get("value").cloned())
            .map_err(|e| format!("operation {n} ({op} {path}): {e}; nothing saved"))?;
    }
    if pack["id"] != id.as_str() {
        return Err("the scenario id cannot be changed".into());
    }
    let problems = validate(&pack).await?;
    if !problems.is_empty() {
        return Ok(json!({"saved": false, "problems": problems}));
    }
    if args["dry_run"] == true {
        return Ok(json!({"saved": false, "valid": true, "dry_run": true}));
    }
    save(&pack).await?;
    notify(&id, false);
    Ok(json!({"saved": true, "id": id, "operations": ops.len()}))
}

async fn delete_scenario(args: Value) -> Result<Value, String> {
    let id = arg_str(&args, "id")?;
    if args["confirm"] != true {
        return Err("deleting is permanent: ask the user, then call again with confirm=true".into());
    }
    call("/api/master/scenarios/delete", Some(json!({"id": id}))).await?;
    notify(&id, false);
    Ok(json!({"deleted": id}))
}

async fn open_in_editor(args: Value) -> Result<Value, String> {
    let id = arg_str(&args, "id")?;
    call(&scenario_path(&id), None).await?;
    notify(&id, true);
    Ok(json!({"requested": id, "note": "shown if the Scenario view is open"}))
}

async fn start_scenario(args: Value) -> Result<Value, String> {
    let id = arg_str(&args, "id")?;
    if args["confirm"] != true {
        return Err("this discards the exercise in progress: ask the user, then call again with confirm=true".into());
    }
    let message = crate::scenario_editor::start_scenario(&id).await?;
    Ok(json!({"started": id, "message": message}))
}

// ------------------------------------------------------------ browser glue

fn run_tool(tools: &'static [Tool], name: &str, args: JsValue) -> Promise {
    let name = name.to_string();
    wasm_bindgen_futures::future_to_promise(async move {
        let args: Value = js_sys::JSON::stringify(&args)
            .ok()
            .and_then(|s| s.as_string())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let args = if args.is_null() { json!({}) } else { args };
        let result = match tools.iter().find(|t| t.name == name) {
            Some(t) => (t.run)(args).await,
            None => Err(format!("unknown tool `{name}`")),
        };
        let (text, is_error) = match result {
            Ok(v) => (crate::api::pretty(&v), false),
            Err(e) => (e, true),
        };
        let part = Object::new();
        let _ = Reflect::set(&part, &"type".into(), &"text".into());
        let _ = Reflect::set(&part, &"text".into(), &text.into());
        let out = Object::new();
        let _ = Reflect::set(&out, &"content".into(), &Array::of1(&part));
        if is_error {
            let _ = Reflect::set(&out, &"isError".into(), &true.into());
        }
        Ok(out.into())
    })
}

fn tool_object(tools: &'static [Tool], tool: &'static Tool) -> Result<Object, JsValue> {
    let o = Object::new();
    Reflect::set(&o, &"name".into(), &tool.name.into())?;
    Reflect::set(&o, &"description".into(), &tool.description.into())?;
    Reflect::set(&o, &"inputSchema".into(), &js_sys::JSON::parse(&tool.schema.to_string())?)?;
    let annotations = Object::new();
    Reflect::set(&annotations, &"readOnlyHint".into(), &tool.read_only.into())?;
    Reflect::set(&o, &"annotations".into(), &annotations)?;
    let name = tool.name;
    let execute = Closure::<dyn FnMut(JsValue, JsValue) -> Promise>::new(move |args, _client| run_tool(tools, name, args));
    Reflect::set(&o, &"execute".into(), execute.as_ref())?;
    execute.forget();
    Ok(o)
}

/// Registers the tools with `navigator.modelContext` as soon as it exists.
/// Safe to call more than once.
pub fn register() {
    thread_local! { static TOOLS: &'static [Tool] = Box::leak(tools().into_boxed_slice()); }
    let tools: &'static [Tool] = TOOLS.with(|t| *t);
    expose_fallback(tools);
    wasm_bindgen_futures::spawn_local(async move {
        // The context may be injected after load (extensions, polyfills).
        for wait in [0u32, 250, 500, 1000, 2000, 4000, 8000, 15000, 30000] {
            gloo_timers::future::TimeoutFuture::new(wait).await;
            if try_register(tools) {
                return;
            }
        }
    });
}

fn try_register(tools: &'static [Tool]) -> bool {
    let Ok(context) = Reflect::get(&crate::api::window().navigator(), &"modelContext".into()) else {
        return false;
    };
    if context.is_undefined() || context.is_null() {
        return false;
    }
    let register_tool = Reflect::get(&context, &"registerTool".into()).ok().and_then(|f| f.dyn_into::<Function>().ok());
    if let Some(register_tool) = register_tool {
        for tool in tools {
            if REGISTERED.with(|r| r.borrow().contains(tool.name)) {
                continue;
            }
            match tool_object(tools, tool).and_then(|o| register_tool.call1(&context, &o)) {
                Ok(_) => {
                    REGISTERED.with(|r| r.borrow_mut().insert(tool.name));
                }
                Err(e) => web_sys::console::warn_2(&format!("WebMCP: tool {} not registered:", tool.name).into(), &e),
            }
        }
        return REGISTERED.with(|r| r.borrow().len() == tools.len());
    }
    // Earlier drafts of the API: one call with the whole list.
    let provide = Reflect::get(&context, &"provideContext".into()).ok().and_then(|f| f.dyn_into::<Function>().ok());
    if let Some(provide) = provide {
        let list = Array::new();
        for tool in tools {
            if let Ok(o) = tool_object(tools, tool) {
                list.push(&o);
            }
        }
        let arg = Object::new();
        let _ = Reflect::set(&arg, &"tools".into(), &list);
        return provide.call1(&context, &arg).is_ok();
    }
    false
}

/// `window.ttxWebMcp = { tools: [{name, description, inputSchema}], call(name, args) }`
fn expose_fallback(tools: &'static [Tool]) {
    let api = Object::new();
    let list = Array::new();
    for t in tools {
        let o = Object::new();
        let _ = Reflect::set(&o, &"name".into(), &t.name.into());
        let _ = Reflect::set(&o, &"description".into(), &t.description.into());
        if let Ok(s) = js_sys::JSON::parse(&t.schema.to_string()) {
            let _ = Reflect::set(&o, &"inputSchema".into(), &s);
        }
        list.push(&o);
    }
    let _ = Reflect::set(&api, &"tools".into(), &list);
    let call = Closure::<dyn FnMut(JsValue, JsValue) -> Promise>::new(move |name: JsValue, args| {
        run_tool(tools, &name.as_string().unwrap_or_default(), args)
    });
    let _ = Reflect::set(&api, &"call".into(), call.as_ref());
    call.forget();
    let _ = Reflect::set(&crate::api::window(), &"ttxWebMcp".into(), &api);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> Value {
        json!({"id": "x", "name": "N", "edits": {"injects": [{"id": "I-01"}, {"id": "I-02"}], "weather": {"wind": 3}}})
    }

    #[test]
    fn replaces_adds_and_removes() {
        let mut p = pack();
        apply_op(&mut p, "replace", "/name", Some(json!("Nuovo"))).unwrap();
        apply_op(&mut p, "add", "/edits/turn_minutes", Some(json!(20))).unwrap();
        apply_op(&mut p, "remove", "/edits/weather/wind", None).unwrap();
        assert_eq!(p["name"], "Nuovo");
        assert_eq!(p["edits"]["turn_minutes"], 20);
        assert!(p["edits"]["weather"].get("wind").is_none());
    }

    #[test]
    fn array_insert_append_and_remove() {
        let mut p = pack();
        apply_op(&mut p, "add", "/edits/injects/-", Some(json!({"id": "I-03"}))).unwrap();
        apply_op(&mut p, "add", "/edits/injects/0", Some(json!({"id": "I-00"}))).unwrap();
        apply_op(&mut p, "remove", "/edits/injects/1", None).unwrap();
        let ids: Vec<_> = p["edits"]["injects"].as_array().unwrap().iter().map(|i| i["id"].clone()).collect();
        assert_eq!(ids, [json!("I-00"), json!("I-02"), json!("I-03")]);
        apply_op(&mut p, "replace", "/edits/injects/2/id", Some(json!("Z"))).unwrap();
        assert_eq!(p["edits"]["injects"][2]["id"], "Z");
    }

    #[test]
    fn rejects_bad_paths() {
        let mut p = pack();
        assert!(apply_op(&mut p, "replace", "/nope", Some(json!(1))).is_err());
        assert!(apply_op(&mut p, "remove", "/edits/injects/9", None).is_err());
        assert!(apply_op(&mut p, "add", "/edits/injects/9", Some(json!(1))).is_err());
        assert!(apply_op(&mut p, "add", "/edits/zzz/a", Some(json!(1))).is_err());
        assert!(apply_op(&mut p, "add", "", Some(json!(1))).is_err());
        assert!(apply_op(&mut p, "add", "/name", None).is_err());
        assert!(apply_op(&mut p, "move", "/name", None).is_err());
        assert_eq!(p, pack());
    }

    #[test]
    fn pointer_escapes() {
        let mut p = json!({"a/b": {"c~d": 1}});
        apply_op(&mut p, "replace", "/a~1b/c~0d", Some(json!(2))).unwrap();
        assert_eq!(p["a/b"]["c~d"], 2);
    }
}
