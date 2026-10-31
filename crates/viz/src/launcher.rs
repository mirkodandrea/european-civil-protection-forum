//! Opening panel of the native big screen: resume the saved session or start
//! a new exercise from a scenario of the library, like the standalone
//! launcher. The renderer only lists what the host offers and hands the
//! choice back; the host builds the exercise, and the scene follows through
//! the view source like any other scenario change.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, RichText};
use bevy_egui::EguiContexts;

/// One scenario (territory or pack) the host can start.
#[derive(Clone, Debug)]
pub struct LaunchOption {
    pub id: String,
    pub name: String,
    pub description: String,
}

/// What the launcher offers.
#[derive(Clone, Debug, Default)]
pub struct LaunchMenu {
    /// The saved session that is running, e.g. "Valverde · 12 steps".
    pub saved: Option<String>,
    pub options: Vec<LaunchOption>,
    /// Scenario of the running exercise, preselected.
    pub current: String,
}

/// The host side: lists scenarios and starts one, off the render thread.
pub trait Launcher: Send + Sync {
    fn menu(&self) -> LaunchMenu;
    /// Start a new exercise from `id`; the answer arrives on the receiver.
    fn start(&self, id: &str) -> Receiver<Result<(), String>>;
}

/// Shown at startup when the host provides a [`Launcher`].
#[derive(Resource)]
pub struct LaunchPanel {
    launcher: Arc<dyn Launcher>,
    menu: LaunchMenu,
    selected: String,
    pending: Option<Mutex<Receiver<Result<(), String>>>>,
    error: String,
}

impl LaunchPanel {
    pub fn new(launcher: Arc<dyn Launcher>) -> Self {
        let menu = launcher.menu();
        Self { selected: menu.current.clone(), launcher, menu, pending: None, error: String::new() }
    }
}

pub fn draw(mut commands: Commands, mut contexts: EguiContexts, panel: Option<ResMut<LaunchPanel>>) {
    let Some(mut panel) = panel else {
        return;
    };
    let panel = &mut *panel;
    let answer = panel
        .pending
        .as_ref()
        .map(|rx| rx.lock().map_or(Err(TryRecvError::Disconnected), |rx| rx.try_recv()));
    if let Some(answer) = answer {
        match answer {
            Ok(Ok(())) => {
                commands.remove_resource::<LaunchPanel>();
                return;
            }
            Ok(Err(e)) => {
                panel.error = e;
                panel.pending = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                panel.error = "the scenario could not be started".into();
                panel.pending = None;
            }
        }
    }
    let mut close = false;
    let mut start = None;
    let ctx = contexts.ctx_mut();
    egui::Window::new("Start the exercise")
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .show(ctx, |ui| {
            let busy = panel.pending.is_some();
            ui.add_enabled_ui(!busy, |ui| {
                if let Some(saved) = &panel.menu.saved {
                    ui.label(RichText::new("Saved session").strong().size(16.0));
                    ui.label(saved);
                    if ui.button(RichText::new("Resume session").strong()).clicked() {
                        close = true;
                    }
                    ui.separator();
                }
                ui.label(RichText::new("New exercise").strong().size(16.0));
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for o in &panel.menu.options {
                        let chosen = panel.selected == o.id;
                        let text = RichText::new(&o.name).strong();
                        if ui.radio(chosen, text).clicked() {
                            panel.selected = o.id.clone();
                        }
                        if !o.description.is_empty() {
                            ui.indent(&o.id, |ui| {
                                ui.label(RichText::new(&o.description).color(Color32::GRAY).small());
                            });
                        }
                    }
                });
                if panel.menu.saved.is_some() {
                    ui.label(RichText::new("The saved session will be discarded.").color(Color32::GRAY));
                }
                if ui.button(RichText::new("Start new exercise").strong()).clicked() {
                    start = Some(panel.selected.clone());
                }
            });
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Building the exercise…");
                });
            }
            if !panel.error.is_empty() {
                ui.colored_label(Color32::from_rgb(255, 120, 100), &panel.error);
            }
        });
    if close {
        commands.remove_resource::<LaunchPanel>();
        return;
    }
    if let Some(id) = start {
        // Nothing to rebuild: no saved session and the scenario already runs.
        if panel.menu.saved.is_none() && id == panel.menu.current {
            commands.remove_resource::<LaunchPanel>();
            return;
        }
        panel.error.clear();
        let rx = panel.launcher.start(&id);
        panel.pending = Some(Mutex::new(rx));
    }
}
