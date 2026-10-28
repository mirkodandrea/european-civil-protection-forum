//! The on-screen HUD: turn and clock (with live resolution progress), weather, key
//! numbers, the decision feed, how to join, and map labels (facilities and
//! zone codes projected from 3D).

use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, FontId, RichText};
use bevy_egui::EguiContexts;

use crate::frame::{to_bevy, Frame};
use crate::objects::ShowGrid;
use crate::{Info, Scn, Snapshot};

fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "localhost".into())
}

/// Pick a graphics level: applied live, remembered for the next launch.
fn set_level(quality: &mut crate::quality::Quality, level: &str) {
    *quality = crate::quality::Quality::preset(level);
    crate::quality::save(level);
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut ctx: EguiContexts,
    editor: Option<Res<crate::editor::Editor>>,
    snap: Res<Snapshot>,
    frame: Res<Frame>,
    scn: Res<Scn>,
    info: Res<Info>,
    grid: Res<ShowGrid>,
    mut quality: ResMut<crate::quality::Quality>,
    fire_style: Res<crate::fire_field::FirePresentation>,
    keys: Res<ButtonInput<KeyCode>>,
    cam: Query<(&Camera, &GlobalTransform)>,
    mut ip: Local<Option<String>>,
    mut show_labels: Local<Option<bool>>,
) {
    if editor.is_some() {
        return;
    }
    // The embedded viewer shares its small viewport with the browser controls.
    // Operational information is already rendered by the surrounding role UI.
    if cfg!(target_arch = "wasm32") {
        return;
    }
    if std::env::var("TTX_SNAPSHOT_UI").as_deref() == Ok("0") {
        return;
    }
    let ip = ip.get_or_insert_with(local_ip).clone();
    let s = &scn.0;
    let show_labels =
        show_labels.get_or_insert_with(|| std::env::var("TTX_MAP_LABELS").as_deref() == Ok("1"));
    if keys.just_pressed(KeyCode::KeyL) {
        *show_labels = !*show_labels;
    }
    if keys.just_pressed(KeyCode::KeyQ) {
        let next = quality.next_level();
        set_level(&mut quality, next);
    }
    let ctx = ctx.ctx_mut();
    let fire_caption = format!(
        "Fire display: {} · model {:.0} m · V toggles",
        if fire_style.continuous {
            "interpolated"
        } else {
            "cells"
        },
        s.grid.cellsize
    );
    let panel = egui::Frame::none()
        .fill(Color32::from_rgba_unmultiplied(12, 20, 28, 245))
        .rounding(8.0)
        .inner_margin(12.0);

    egui::Area::new(egui::Id::new("status"))
        .anchor(Align2::LEFT_TOP, [12.0, 12.0])
        .show(ctx, |ui| {
            panel.show(ui, |ui| {
                ui.label(
                    RichText::new(&s.name)
                        .color(Color32::WHITE)
                        .size(20.0)
                        .strong(),
                );
                let clock = scenario::format_clock(s.start_clock_s + frame.display_s);
                let phase = if matches!(snap.phase.as_str(), "orders" | "resolving") {
                    format!("Turn {}", snap.turn)
                } else {
                    snap.phase.clone()
                };
                ui.label(
                    RichText::new(format!("{phase}   ·   {clock}"))
                        .color(Color32::from_rgb(255, 210, 120))
                        .size(26.0)
                        .strong(),
                );
                // Always say whether time is moving or the game is held in the turn.
                let (badge, color) = if snap.resolving.is_some() {
                    ("▶ SIMULATION RUNNING", Color32::from_rgb(255, 120, 80))
                } else if snap.clock_running {
                    ("▶ CLOCK RUNNING", Color32::from_rgb(255, 120, 80))
                } else if snap.phase == "orders" {
                    ("⏸ PAUSED · ORDERS OPEN", Color32::from_rgb(120, 220, 150))
                } else {
                    ("⏸ PAUSED", Color32::from_rgb(170, 180, 190))
                };
                ui.label(RichText::new(badge).color(color).size(16.0).strong());
                if let Some(view::TurnWindow { turn, from_s, to_s }) = snap.resolving {
                    let done = (frame.display_s - from_s) as f32 / (to_s - from_s).max(1) as f32;
                    ui.label(
                        RichText::new(format!(
                            "▶ simulating turn {turn}   → {}",
                            scenario::format_clock(s.start_clock_s + to_s)
                        ))
                        .color(Color32::from_rgb(255, 120, 80))
                        .size(16.0),
                    );
                    ui.add(egui::ProgressBar::new(done.clamp(0.0, 1.0)).desired_width(300.0));
                }
                if let Some(w) = snap.weather {
                    let arrows = ["↓", "↙", "←", "↖", "↑", "↗", "→", "↘"];
                    let a =
                        arrows[(((w.wind_dir_deg + 22.5).rem_euclid(360.0)) / 45.0) as usize % 8];
                    ui.label(
                        RichText::new(format!(
                            "wind {a} from {:.0}° {:.0} km/h · fuel moisture {:.0}%",
                            w.wind_dir_deg, w.wind_speed_kmh, w.moisture_pct
                        ))
                        .color(Color32::LIGHT_GRAY)
                        .size(15.0),
                    );
                }
                if let Some(e) = snap.ensemble {
                    ui.label(
                        RichText::new(format!(
                            "fire = cells reached by ≥{:.0}% of {} runs · halo = fewer",
                            e.cutoff * 100.0,
                            e.realizations
                        ))
                        .color(Color32::from_rgb(195, 205, 215))
                        .size(13.0),
                    );
                }
                ui.label(
                    RichText::new(&fire_caption)
                        .color(Color32::from_rgb(195, 205, 215))
                        .size(12.0),
                );
                if let Some(st) = &snap.stats {
                    ui.add_space(4.0);
                    for (k, v) in [
                        ("burned area", format!("{:.1} ha", st.burned_ha)),
                        (
                            "buildings damaged / destroyed",
                            format!("{} / {}", st.damaged, st.destroyed),
                        ),
                        ("people evacuated", st.evacuated_people.to_string()),
                        ("people at risk", st.people_at_risk.to_string()),
                        (
                            "fire lines / drops",
                            format!("{:.1} km / {}", st.line_km, st.drops),
                        ),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(k)
                                    .color(Color32::from_rgb(195, 205, 215))
                                    .size(14.0),
                            );
                            ui.label(RichText::new(v).color(Color32::WHITE).size(15.0).strong());
                        });
                    }
                }
            });
        });

    egui::Area::new(egui::Id::new("feed"))
        .anchor(Align2::RIGHT_TOP, [-12.0, 12.0])
        .show(ctx, |ui| {
            panel.show(ui, |ui| {
                ui.set_max_width(520.0);
                ui.label(
                    RichText::new("Exercise feed")
                        .color(Color32::WHITE)
                        .size(16.0)
                        .strong(),
                );
                if snap.feed.is_empty() {
                    ui.label(
                        RichText::new("Waiting for the EXCON…")
                            .color(Color32::from_rgb(195, 205, 215)),
                    );
                }
                for line in &snap.feed {
                    ui.label(
                        RichText::new(line)
                            .color(Color32::from_rgb(220, 220, 225))
                            .size(14.0),
                    );
                }
            });
        });

    egui::Area::new(egui::Id::new("join")).anchor(Align2::LEFT_BOTTOM, [12.0, -12.0]).show(ctx, |ui| {
        panel.show(ui, |ui| {
            ui.label(RichText::new(if std::env::var("TTX_SNAPSHOT").is_ok() { "Reproducible visual fixture · HTTP and LLM off".into() } else if cfg!(target_arch = "wasm32") { "Standalone browser simulation · camera: drag pan · right/Shift-drag rotate · wheel zoom".into() } else { match (&info.remote, info.port) { (Some(url), _) => format!("Remote view of {url}"), (None, Some(port)) => format!("Players: http://{ip}:{port}/player.html     EXCON: /master.html"), (None, None) => "Exercise view".to_string() } }).color(Color32::WHITE).size(15.0));
            ui.label(RichText::new(format!("AI: {}   ·   drag pan · right/Shift-drag rotate · wheel zoom · A auto-orbit · F fire · G grid · R/Home reset", if info.llm_model.is_empty() { "see EXCON page" } else { &info.llm_model })).color(Color32::from_rgb(195, 205, 215)).size(12.0));
        });
    });

    egui::Area::new(egui::Id::new("deployments")).anchor(Align2::RIGHT_BOTTOM, [-12.0,-12.0]).show(ctx,|ui|{
      panel.show(ui,|ui|{
       ui.label(RichText::new("Approved deployments").color(Color32::WHITE).strong());
       for task in snap.deployments.iter().filter(|t|t.qty>0&&t.status!=view::TaskStatus::Cancelled).take(12) {
        let remaining=(task.arrival_s-snap.time_s).max(0);
        ui.label(RichText::new(format!("#{} {} ×{} · {:?}{}",task.id,task.resource,task.qty,task.status,if task.status==view::TaskStatus::EnRoute {format!(" · ETA {remaining}s")}else{String::new()})).color(Color32::WHITE));
       }
       ui.label(RichText::new("orange travelling/refilling/withdrawing · cyan working · green complete · red failed").color(Color32::from_rgb(195, 205, 215)).size(11.0));
       ui.horizontal(|ui| {
        ui.label(RichText::new("Graphics (Q)").color(Color32::from_rgb(195, 205, 215)).size(11.0));
        for level in crate::quality::LEVELS {
         if ui.selectable_label(quality.name == level, level).clicked() && quality.name != level {
          set_level(&mut quality, level);
         }
        }
       });
      });
    });
    // Map labels are optional because they can obscure the scene at overview scale.
    if !*show_labels {
        return;
    }
    let Ok((camera, gt)) = cam.get_single() else {
        return;
    };
    let painter = ctx.layer_painter(egui::LayerId::background());
    let mut occupied: Vec<egui::Rect> = Vec::new();
    let mut label = |p: scenario::Pos, lift: f32, text: &str, size: f32, color: Color32| {
        if let Some(v) = camera.world_to_viewport(gt, to_bevy(s, p, lift)) {
            let anchor = egui::pos2(v.x, v.y);
            let galley =
                painter.layout_no_wrap(text.to_string(), FontId::proportional(size), color);
            let extent = galley.size() + egui::vec2(10.0, 6.0);
            let mut rect =
                egui::Rect::from_center_size(anchor - egui::vec2(0.0, extent.y * 0.5), extent);
            // Deterministic screen-space decluttering; keep a leader to the true point.
            for _ in 0..12 {
                if occupied.iter().all(|r| !r.intersects(rect)) {
                    break;
                }
                rect = rect.translate(egui::vec2(0.0, -extent.y - 3.0));
            }
            occupied.push(rect);
            painter.line_segment(
                [anchor, rect.center_bottom()],
                egui::Stroke::new(1.0, Color32::WHITE),
            );
            painter.rect_filled(rect, 3.0, Color32::from_rgba_unmultiplied(15, 24, 31, 235));
            painter.galley(rect.min + egui::vec2(5.0, 3.0), galley, color);
        }
    };
    for task in snap
        .deployments
        .iter()
        .filter(|t| t.qty > 0 && t.status != view::TaskStatus::Cancelled)
    {
        label(
            task.position(snap.time_s),
            if task.resource == "drones" {
                120.0
            } else if task.resource == "air_tankers" {
                155.0
            } else {
                50.0
            },
            &format!(
                "#{} {} ×{} · {:?}",
                task.id, task.resource, task.qty, task.status
            ),
            13.0,
            Color32::from_rgb(255, 230, 130),
        );
    }
    let mut road_labels = std::collections::HashSet::new();
    for view::RoadSegment { road, from: a, to: b, closed, cut_by_fire: cut } in &snap.road_segments {
        if (*closed || *cut) && road_labels.insert((*road, *cut)) {
            label(
                [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5],
                45.0,
                if *cut {
                    "× FIRE CUT"
                } else {
                    "— POLICE CLOSED"
                },
                12.0,
                if *cut {
                    Color32::from_rgb(255, 150, 130)
                } else {
                    Color32::from_rgb(255, 215, 130)
                },
            );
        }
    }
    for b in &s.buildings {
        if let Some(n) = &b.name {
            label(b.pos, 45.0, n, 15.0, Color32::WHITE);
        }
    }
    if grid.0 {
        for name in s.zones.names() {
            if let Some(b) = s.zone_bounds(&name) {
                label(
                    [b[0] + 40.0, b[3] - 40.0],
                    8.0,
                    &name,
                    13.0,
                    Color32::from_rgba_unmultiplied(255, 255, 255, 190),
                );
            }
        }
    }
}
