//! The on-screen HUD: turn and clock (with live resolution progress), weather, key
//! numbers, the decision feed, how to join, and map labels (facilities and
//! zone codes projected from 3D).

use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, FontId, RichText};
use bevy_egui::EguiContexts;

use crate::frame::{to_bevy, Frame};
use crate::objects::ShowGrid;
use crate::{Scn, ServerPort, Snapshot};

fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "localhost".into())
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    mut ctx: EguiContexts,
    snap: Res<Snapshot>,
    frame: Res<Frame>,
    scn: Res<Scn>,
    port: Res<ServerPort>,
    grid: Res<ShowGrid>,
    cam: Query<(&Camera, &GlobalTransform)>,
    mut ip: Local<Option<String>>,
) {
    let ip = ip.get_or_insert_with(local_ip).clone();
    let s = &scn.0;
    let ctx = ctx.ctx_mut();
    let panel = egui::Frame::none().fill(Color32::from_rgba_unmultiplied(15, 15, 18, 215)).rounding(8.0).inner_margin(12.0);

    egui::Area::new(egui::Id::new("status")).anchor(Align2::LEFT_TOP, [12.0, 12.0]).show(ctx, |ui| {
        panel.show(ui, |ui| {
            ui.label(RichText::new(&s.name).color(Color32::WHITE).size(20.0).strong());
            let clock = scenario::format_clock(s.start_clock_s + frame.display_s);
            let phase = if matches!(snap.phase.as_str(), "orders" | "resolving") { format!("Turn {}", snap.turn) } else { snap.phase.clone() };
            ui.label(RichText::new(format!("{phase}   ·   {clock}")).color(Color32::from_rgb(255, 210, 120)).size(26.0).strong());
            if let Some((turn, from_s, to_s)) = snap.resolving {
                let done = (frame.display_s - from_s) as f32 / (to_s - from_s).max(1) as f32;
                ui.label(RichText::new(format!("▶ simulating turn {turn}   → {}", scenario::format_clock(s.start_clock_s + to_s))).color(Color32::from_rgb(255, 120, 80)).size(16.0));
                ui.add(egui::ProgressBar::new(done.clamp(0.0, 1.0)).desired_width(300.0));
            }
            if let Some(w) = snap.weather {
                let arrows = ["↓", "↙", "←", "↖", "↑", "↗", "→", "↘"];
                let a = arrows[(((w.wind_dir_deg + 22.5).rem_euclid(360.0)) / 45.0) as usize % 8];
                ui.label(RichText::new(format!("wind {a} from {:.0}° {:.0} km/h · fuel moisture {:.0}%", w.wind_dir_deg, w.wind_speed_kmh, w.moisture_pct)).color(Color32::LIGHT_GRAY).size(15.0));
            }
            if let Some(e) = snap.ensemble {
                ui.label(RichText::new(format!("fire = cells reached by ≥{:.0}% of {} runs · halo = fewer", e.cutoff * 100.0, e.realizations)).color(Color32::GRAY).size(13.0));
            }
            if let Some(st) = &snap.stats {
                ui.add_space(4.0);
                for (k, v) in [
                    ("burned area", format!("{:.1} ha", st.burned_ha)),
                    ("buildings damaged / destroyed", format!("{} / {}", st.damaged, st.destroyed)),
                    ("people evacuated", st.evacuated_people.to_string()),
                    ("people at risk", st.people_at_risk.to_string()),
                    ("fire lines / drops", format!("{:.1} km / {}", st.line_km, st.drops)),
                ] {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(k).color(Color32::GRAY).size(14.0));
                        ui.label(RichText::new(v).color(Color32::WHITE).size(15.0).strong());
                    });
                }
            }
        });
    });

    egui::Area::new(egui::Id::new("feed")).anchor(Align2::RIGHT_TOP, [-12.0, 12.0]).show(ctx, |ui| {
        panel.show(ui, |ui| {
            ui.set_max_width(520.0);
            ui.label(RichText::new("Exercise feed").color(Color32::WHITE).size(16.0).strong());
            if snap.feed.is_empty() {
                ui.label(RichText::new("Waiting for the Exercise Director…").color(Color32::GRAY));
            }
            for line in &snap.feed {
                ui.label(RichText::new(line).color(Color32::from_rgb(220, 220, 225)).size(14.0));
            }
        });
    });

    egui::Area::new(egui::Id::new("join")).anchor(Align2::LEFT_BOTTOM, [12.0, -12.0]).show(ctx, |ui| {
        panel.show(ui, |ui| {
            ui.label(RichText::new(format!("Players: http://{ip}:{}/player.html     Director: /master.html", port.0)).color(Color32::WHITE).size(15.0));
            ui.label(RichText::new(format!("AI: {}   ·   drag rotate · right-drag pan · wheel zoom · A auto-orbit · F fire · G grid · R reset", if snap.llm_model.is_empty() { "see Director page" } else { &snap.llm_model })).color(Color32::GRAY).size(12.0));
        });
    });

    // Map labels.
    let Ok((camera, gt)) = cam.get_single() else { return };
    let painter = ctx.layer_painter(egui::LayerId::background());
    let label = |p: scenario::Pos, lift: f32, text: &str, size: f32, color: Color32| {
        if let Some(v) = camera.world_to_viewport(gt, to_bevy(s, p, lift)) {
            let pos = egui::pos2(v.x, v.y);
            painter.text(pos + egui::vec2(1.0, 1.0), Align2::CENTER_BOTTOM, text, FontId::proportional(size), Color32::BLACK);
            painter.text(pos, Align2::CENTER_BOTTOM, text, FontId::proportional(size), color);
        }
    };
    for b in &s.buildings {
        if let Some(n) = &b.name {
            label(b.pos, 25.0, n, 15.0, Color32::WHITE);
        }
    }
    if grid.0 {
        for name in s.zones.names() {
            if let Some(b) = s.zone_bounds(&name) {
                label([b[0] + 40.0, b[3] - 40.0], 8.0, &name, 13.0, Color32::from_rgba_unmultiplied(255, 255, 255, 190));
            }
        }
    }
}
