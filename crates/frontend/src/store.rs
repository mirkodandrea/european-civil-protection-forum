//! Browser database of the standalone app (`localStorage`): the session in
//! progress, the library of editable injects and a one-shot notice.
use serde::{Deserialize, Serialize};
use std::cell::{Cell, RefCell};

pub const SESSION_KEY: &str = "ttx.standalone.session";
pub const INJECTS_KEY: &str = "ttx.standalone.injects";
pub const API_KEY: &str = "ttx.standalone.openrouter-key";
const NOTICE_KEY: &str = "ttx.standalone.notice";
const FORMAT: u32 = 1;

/// Everything needed to play a session again from the start: the injects it
/// began with, the requests applied since, and the AI answers they received.
#[derive(Serialize, Deserialize)]
pub struct SavedSession {
    pub v: u32,
    pub msel: Vec<scenario::InjectTemplate>,
    pub journal: Vec<server::browser::JournalEntry>,
    pub tape: Vec<(String, String)>,
    /// Simulated seconds reached when saved.
    pub end_t_s: i64,
}

thread_local! {
    static PENDING: RefCell<Option<SavedSession>> = const { RefCell::new(None) };
    static RESTORING: Cell<bool> = const { Cell::new(false) };
    static LAST_SAVED: Cell<(usize, usize, i64)> = const { Cell::new((0, 0, 0)) };
    static FROZEN: Cell<bool> = const { Cell::new(false) };
    static SAVE_ERROR: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

pub fn get(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

pub fn set(key: &str, value: &str) -> Result<(), String> {
    storage()
        .ok_or("archiviazione del browser non disponibile")?
        .set_item(key, value)
        .map_err(|_| "spazio del browser esaurito o archiviazione bloccata".to_string())
}

pub fn remove(key: &str) {
    if let Some(s) = storage() {
        let _ = s.remove_item(key);
    }
}

/// Stored session, if present and readable. An unreadable one is dropped.
pub fn load_session() -> Option<SavedSession> {
    let text = get(SESSION_KEY)?;
    match serde_json::from_str::<SavedSession>(&text) {
        Ok(s) if s.v == FORMAT => Some(s),
        _ => {
            remove(SESSION_KEY);
            set_notice("La sessione salvata aveva un formato non più valido ed è stata scartata.");
            None
        }
    }
}

pub fn load_injects() -> Option<Vec<scenario::InjectTemplate>> {
    serde_json::from_str(&get(INJECTS_KEY)?).ok()
}

pub fn save_injects(injects: &[scenario::InjectTemplate]) -> Result<(), String> {
    set(
        INJECTS_KEY,
        &serde_json::to_string(injects).map_err(|e| e.to_string())?,
    )
}

/// Session found at startup and waiting to be replayed.
pub fn set_pending(session: Option<SavedSession>) {
    RESTORING.with(|r| r.set(session.is_some()));
    PENDING.with(|p| *p.borrow_mut() = session);
}

pub fn take_pending() -> Option<SavedSession> {
    PENDING.with(|p| p.borrow_mut().take())
}

pub fn restoring() -> bool {
    RESTORING.with(Cell::get)
}

/// Stop writing: the page is about to reload after deleting stored objects.
pub fn freeze() {
    FROZEN.with(|f| f.set(true));
}

pub fn finish_restore() {
    RESTORING.with(|r| r.set(false));
}

pub fn set_notice(text: &str) {
    let _ = set(NOTICE_KEY, text);
}

/// A notice left before the last reload; shown once.
pub fn take_notice() -> Option<String> {
    let text = get(NOTICE_KEY)?;
    remove(NOTICE_KEY);
    Some(text)
}

pub fn save_error() -> Option<String> {
    SAVE_ERROR.with(|e| e.borrow().clone())
}

/// Write the session when something changed since the last write (`force`
/// skips that check). `msel` is what the running exercise was built with.
pub fn autosave(state: &server::AppState, force: bool) {
    if restoring() || FROZEN.with(Cell::get) {
        return;
    }
    let journal = server::browser::journal();
    let tape = server::browser::tape_entries();
    let (msel, end_t_s) = {
        let ex = state.lock();
        (ex.world.scn.msel.clone(), ex.world.time_s())
    };
    let signature = (journal.len(), tape.len(), end_t_s);
    if !force && LAST_SAVED.with(Cell::get) == signature {
        return;
    }
    LAST_SAVED.with(|l| l.set(signature));
    if journal.is_empty() {
        remove(SESSION_KEY);
        return;
    }
    let session = SavedSession {
        v: FORMAT,
        msel,
        journal,
        tape,
        end_t_s,
    };
    let result = serde_json::to_string(&session)
        .map_err(|e| e.to_string())
        .and_then(|text| set(SESSION_KEY, &text));
    SAVE_ERROR.with(|e| *e.borrow_mut() = result.err());
}

/// Size of the stored session in kB, and the number of steps it replays.
pub fn session_summary() -> Option<(usize, usize)> {
    let text = get(SESSION_KEY)?;
    let steps = serde_json::from_str::<serde_json::Value>(&text)
        .ok()?
        .get("journal")?
        .as_array()?
        .len();
    Some((text.len().div_ceil(1024), steps))
}

/// Delete the given stored objects and reload into a clean start.
pub fn restart(keys: &[&str]) {
    freeze();
    for key in keys {
        remove(key);
    }
    let _ = crate::api::window().location().reload();
}

/// The settings' storage operations, on browser storage.
pub fn backend() -> crate::settings::Backend {
    use crate::settings::Op;
    crate::settings::Backend(std::rc::Rc::new(|op| {
        Box::pin(async move {
            match op {
                Op::SaveInjects(list) => save_injects(&list).map(|()| {
                    "Injects salvati nella libreria · valgono dalla prossima nuova sessione".into()
                }),
                Op::NewSession(list) => {
                    save_injects(&list)?;
                    restart(&[SESSION_KEY]);
                    Ok("Nuova sessione…".into())
                }
                Op::Delete(target) => {
                    let keys: &[&str] = match target {
                        "session" => &[SESSION_KEY],
                        "injects" => &[INJECTS_KEY],
                        _ => &[SESSION_KEY, INJECTS_KEY, API_KEY],
                    };
                    restart(keys);
                    Ok("Eliminato".into())
                }
            }
        })
    }))
}
