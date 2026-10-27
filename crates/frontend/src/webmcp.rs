//! WebMCP tools for editing scenarios: an agent in the browser (ChatGPT Atlas,
//! Chrome with WebMCP, …) works on the scenario open in the Scenario editor
//! through `navigator.modelContext`.
//!
//! The tools exist only while a scenario is open in the editor: the editor
//! calls `attach` when one opens and `detach` when it closes, which registers
//! and unregisters them. They act on the editor's draft (what the user sees,
//! even a copy not yet saved); changes are checked by the scenario crate and
//! stored only by `save_open_scenario`. Nothing touches the running exercise.
//! Same code for the server app and the standalone app (`api::request`).
//!
//! Without `navigator.modelContext` the tools are reachable while attached as
//! `window.ttxWebMcp.call(name, args)` (tests, bookmarklets, other agents).
use crate::scenario_editor::call;
use futures::{future::LocalBoxFuture, FutureExt};
use js_sys::{Array, Function, Object, Promise, Reflect};
use serde_json::{json, Value};
use std::{cell::{Cell, RefCell}, collections::HashSet, rc::Rc};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};

/// The scenario being edited, as the agent sees it.
pub struct Draft {
    pub pack: Value,
    /// Names of the territory (places, zones) to use in edits.
    pub reference: Value,
    pub saved_id: Option<String>,
    pub dirty: bool,
}

/// How the tools reach the editor's state.
pub struct Editor {
    pub snapshot: Box<dyn Fn() -> Option<Draft>>,
    /// Replaces the draft (marks it unsaved).
    pub replace: Box<dyn Fn(Value) -> Result<(), String>>,
    /// The draft was stored under this id.
    pub saved: Box<dyn Fn(String)>,
}

thread_local! {
    static EDITOR: RefCell<Option<Rc<Editor>>> = const { RefCell::new(None) };
    static REGISTERED: RefCell<HashSet<&'static str>> = RefCell::new(HashSet::new());
    /// Bumped on every attach/detach so stale registration retries stop.
    static EPOCH: Cell<u32> = const { Cell::new(0) };
}

pub const CHANGED_EVENT: &str = "ttx-scenarios-changed";

fn editor() -> Result<Rc<Editor>, String> {
    EDITOR
        .with(|e| e.borrow().clone())
        .filter(|e| (e.snapshot)().is_some())
        .ok_or_else(|| "no scenario is open in the editor: ask the user to open one".to_string())
}

fn notify() {
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
id (a-z 0-9 _ -, max 64; changeable only until the first save), name, description, base (territory id), edits, run.\n\
`edits` holds what a designer controls (a field left out keeps the territory's value): \
name/start time/weather, turn_minutes (1-1440, default 30), resource types, roles (id, name, resources, allowed actions), \
reinforcement offers, places, and `injects` (the MSEL).\n\
Each inject has id, title/text, recipients, a suggested turn or an explicit `at_s` (simulated seconds from start) and `actions`: \
{id (unique in the whole scenario), title, command, offset_s, note}. `command` is an EXCON command such as \
{\"type\":\"SetWeather\",\"wind_dir_deg\":270} or {\"type\":\"Ignite\",...}. The fire starts from an `Ignite` action of an inject \
(ignitions of at least 40 m; there are no initial ignitions).\n\
Places can be named places of the territory, 8x8 grid zone codes such as F4, zone edges such as F4:N, or coordinates.\n\
Workflow: get_open_scenario (the draft and the territory's names under `reference`) -> edit_open_scenario with JSON-pointer \
operations (the user sees the draft change; invalid changes are refused) -> validate_open_scenario -> save_open_scenario when the \
user agrees. list_scenarios / get_scenario show other scenarios as examples.";

fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "describe_scenario_format",
            description: "Explains how scenarios are structured and the recommended workflow. Call this first.",
            schema: json!({"type": "object", "properties": {}}),
            read_only: true,
            run: |_| async { Ok(json!({"guide": FORMAT_GUIDE})) }.boxed_local(),
        },
        Tool {
            name: "get_open_scenario",
            description: "Returns the scenario open in the editor: the full pack (every editable value filled in, including unsaved changes), whether it is saved, its current problems, and `reference`: the names of the territory (places, zones) to use in edits.",
            schema: json!({"type": "object", "properties": {}}),
            read_only: true,
            run: |_| get_open_scenario().boxed_local(),
        },
        Tool {
            name: "edit_open_scenario",
            description: "Applies JSON-pointer operations to the open scenario's draft. All-or-nothing: if the result has problems, nothing changes and the problems are returned. The draft is NOT saved (use save_open_scenario). Paths start at the pack root, e.g. /name, /edits/turn_minutes, /edits/injects/- (append). Ops: add (inserts into arrays, '-' appends), replace (must exist), remove.",
            schema: json!({"type": "object", "properties": {
                "operations": {"type": "array", "items": {"type": "object", "properties": {
                    "op": {"type": "string", "enum": ["add", "replace", "remove"]},
                    "path": {"type": "string", "description": "JSON pointer from the pack root"},
                    "value": {"description": "New value (not for remove)"}
                }, "required": ["op", "path"]}},
                "dry_run": {"type": "boolean", "description": "Check only, leave the draft as it is"}
            }, "required": ["operations"]}),
            read_only: false,
            run: |a| edit_open_scenario(a).boxed_local(),
        },
        Tool {
            name: "validate_open_scenario",
            description: "Checks the open scenario's draft. Returns every problem found (empty list = valid).",
            schema: json!({"type": "object", "properties": {}}),
            read_only: true,
            run: |_| validate_open_scenario().boxed_local(),
        },
        Tool {
            name: "save_open_scenario",
            description: "Saves the open scenario to the library (validated; built-in scenarios are never overwritten). Does not start it and does not touch the exercise in progress. Ask the user first if they did not request saving.",
            schema: json!({"type": "object", "properties": {}}),
            read_only: false,
            run: |_| save_open_scenario().boxed_local(),
        },
        Tool {
            name: "list_scenarios",
            description: "Lists the scenario library: id, name, territory (base), built-in or not, and which one is running. Read-only; useful to find examples.",
            schema: json!({"type": "object", "properties": {}}),
            read_only: true,
            run: |_| list_scenarios().boxed_local(),
        },
        Tool {
            name: "get_scenario",
            description: "Returns another scenario of the library as a full pack. Read-only; useful as an example of injects, roles or actions.",
            schema: json!({"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]}),
            read_only: true,
            run: |a| get_scenario(a).boxed_local(),
        },
    ]
}

fn arg_str(args: &Value, key: &str) -> Result<String, String> {
    args[key]
        .as_str()
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("missing argument `{key}`"))
}

async fn validate(pack: &Value) -> Result<Vec<String>, String> {
    let v = call("/api/master/scenarios/validate", Some(json!({"pack": pack}))).await?;
    Ok(crate::api::list(&v["problems"]).iter().map(|p| crate::api::text(p).to_string()).collect())
}

async fn list_scenarios() -> Result<Value, String> {
    let lib = call("/api/master/scenarios", None).await?;
    let running = lib["running"].clone();
    let packs: Vec<Value> = crate::api::list(&lib["packs"])
        .iter()
        .map(|p| json!({"id": p["id"], "name": p["name"], "description": p["description"], "base": p["base"], "builtin": p["builtin"], "running": p["id"] == running}))
        .collect();
    Ok(json!({"scenarios": packs, "territories": lib["bases"]}))
}

async fn get_scenario(args: Value) -> Result<Value, String> {
    let id = arg_str(&args, "id")?;
    let v = call(&format!("/api/master/scenarios/{}", crate::api::encode(&id)), None).await?;
    Ok(json!({"pack": v["pack"], "builtin": v["builtin"]}))
}

async fn get_open_scenario() -> Result<Value, String> {
    let draft = (editor()?.snapshot)().ok_or("no scenario is open")?;
    let problems = validate(&draft.pack).await.unwrap_or_default();
    Ok(json!({
        "pack": draft.pack, "saved_as": draft.saved_id, "unsaved_changes": draft.dirty,
        "problems": problems, "reference": draft.reference,
    }))
}

async fn validate_open_scenario() -> Result<Value, String> {
    let draft = (editor()?.snapshot)().ok_or("no scenario is open")?;
    let problems = validate(&draft.pack).await?;
    Ok(json!({"valid": problems.is_empty(), "problems": problems}))
}

async fn edit_open_scenario(args: Value) -> Result<Value, String> {
    let ed = editor()?;
    let draft = (ed.snapshot)().ok_or("no scenario is open")?;
    let ops = args["operations"].as_array().ok_or("missing argument `operations`")?;
    let mut pack = draft.pack.clone();
    for (n, o) in ops.iter().enumerate() {
        let (op, path) = (o["op"].as_str().unwrap_or(""), o["path"].as_str().unwrap_or(""));
        apply_op(&mut pack, op, path, o.get("value").cloned())
            .map_err(|e| format!("operation {n} ({op} {path}): {e}; nothing changed"))?;
    }
    if draft.saved_id.is_some() && pack["id"] != draft.pack["id"] {
        return Err("the id of a saved scenario cannot be changed".into());
    }
    let problems = validate(&pack).await?;
    if !problems.is_empty() {
        return Ok(json!({"applied": false, "problems": problems}));
    }
    if args["dry_run"] == true {
        return Ok(json!({"applied": false, "valid": true, "dry_run": true}));
    }
    (ed.replace)(pack)?;
    Ok(json!({"applied": true, "operations": ops.len(), "saved": false}))
}

async fn save_open_scenario() -> Result<Value, String> {
    let ed = editor()?;
    let draft = (ed.snapshot)().ok_or("no scenario is open")?;
    let id = crate::api::text(&draft.pack["id"]).to_string();
    // The API refuses problems and built-in ids with a readable message.
    call("/api/master/scenarios/save", Some(json!({"pack": draft.pack}))).await?;
    (ed.saved)(id.clone());
    notify();
    Ok(json!({"saved": id}))
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

fn all_tools() -> &'static [Tool] {
    thread_local! { static TOOLS: &'static [Tool] = Box::leak(tools().into_boxed_slice()); }
    TOOLS.with(|t| *t)
}

fn model_context() -> Option<JsValue> {
    let context = Reflect::get(&crate::api::window().navigator(), &"modelContext".into()).ok()?;
    (!context.is_undefined() && !context.is_null()).then_some(context)
}

/// A scenario was opened in the editor: expose the tools. Called again with
/// the same editor it only refreshes the handle.
pub fn attach(editor: Editor) {
    let first = EDITOR.with(|e| e.borrow_mut().replace(Rc::new(editor)).is_none());
    if !first {
        return;
    }
    let epoch = EPOCH.with(|e| {
        e.set(e.get() + 1);
        e.get()
    });
    let tools = all_tools();
    expose_fallback(tools);
    wasm_bindgen_futures::spawn_local(async move {
        // The context may be injected after load (extensions, polyfills).
        for wait in [0u32, 250, 500, 1000, 2000, 4000, 8000, 15000, 30000] {
            gloo_timers::future::TimeoutFuture::new(wait).await;
            if EPOCH.with(Cell::get) != epoch || try_register(tools) {
                return;
            }
        }
    });
}

/// The editor closed: withdraw the tools.
pub fn detach() {
    if EDITOR.with(|e| e.borrow_mut().take()).is_none() {
        return;
    }
    EPOCH.with(|e| e.set(e.get() + 1));
    let _ = Reflect::delete_property(&crate::api::window(), &"ttxWebMcp".into());
    let names: Vec<&'static str> = REGISTERED.with(|r| r.borrow_mut().drain().collect());
    let Some(context) = model_context() else { return };
    let unregister = Reflect::get(&context, &"unregisterTool".into()).ok().and_then(|f| f.dyn_into::<Function>().ok());
    if let Some(unregister) = unregister {
        for name in names {
            let _ = unregister.call1(&context, &name.into());
        }
    } else if let Some(provide) = Reflect::get(&context, &"provideContext".into()).ok().and_then(|f| f.dyn_into::<Function>().ok()) {
        // Earlier drafts of the API: an empty list replaces ours.
        let arg = Object::new();
        let _ = Reflect::set(&arg, &"tools".into(), &Array::new());
        let _ = provide.call1(&context, &arg);
    }
}

fn try_register(tools: &'static [Tool]) -> bool {
    let Some(context) = model_context() else { return false };
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
