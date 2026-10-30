//! EXCON header timers: real time since the exercise started (and in the open
//! turn), the simulated time of the open turn against its length, and the
//! scenario's planned span as a progress bar. The numbers come from the
//! exercise state (`progress`, see `exercise::progress`); between polls the
//! real-time counters advance with the browser clock from the last received
//! value, so a reload resumes from what the exercise recorded.
use gloo_timers::callback::Interval;
use serde_json::Value;
use yew::prelude::*;

/// `m:ss`, or `h:mm:ss` from one hour (negative values count as zero).
pub fn hms(total_s: i64) -> String {
    let s = total_s.max(0);
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// Real elapsed milliseconds now: the value the exercise computed at
/// `now_wall_ms`, plus the time since it was received when still counting.
pub fn live_ms(base_ms: u64, counting: bool, since_received_ms: f64) -> u64 {
    if counting {
        base_ms + since_received_ms.max(0.0) as u64
    } else {
        base_ms
    }
}

/// "Day 1 13:00" → "13:00" when `same_day`; the full label otherwise.
pub fn short_clock(clock: &str, same_day: bool) -> &str {
    match clock.rsplit_once(' ') {
        Some((_, hhmm)) if same_day => hhmm,
        _ => clock,
    }
}

/// Day part of a clock label ("Day 1").
fn day(clock: &str) -> &str {
    clock.rsplit_once(' ').map_or(clock, |(d, _)| d)
}

/// Share of the planned scenario span already simulated, 0–1.
pub fn scenario_fraction(scenario_s: i64, span_s: i64) -> f64 {
    if span_s <= 0 {
        return 0.0;
    }
    (scenario_s as f64 / span_s as f64).clamp(0.0, 1.0)
}

/// "Turno 2/5" (or "Turno 2" without a planned total).
pub fn turn_label(turn: u64, planned: Option<u64>) -> String {
    match planned {
        Some(n) => crate::exfmt!("Turno {}/{}", "Turn {}/{}", turn, n),
        None => crate::exfmt!("Turno {}", "Turn {}", turn),
    }
}

/// "Turno 2/5 · 12:40 / 30:00 simulati".
pub fn turn_line(p: &Value) -> String {
    let turn = p["turn"].as_u64().unwrap_or(0);
    let planned = p["planned_turns"].as_u64();
    let label = turn_label(turn, planned);
    let elapsed = hms(p["turn_s"].as_i64().unwrap_or(0));
    let length = hms(p["turn_len_s"].as_i64().unwrap_or(0));
    crate::exfmt!(
        "{} · {} / {} simulati",
        "{} · {} / {} simulated",
        label,
        elapsed,
        length
    )
}

#[derive(Properties, PartialEq)]
pub struct Props {
    /// `progress` of the exercise state.
    pub progress: Value,
}

#[function_component(ExerciseTimers)]
pub fn exercise_timers(props: &Props) -> Html {
    let p = &props.progress;
    // Browser instant the current value arrived (a new poll resets it).
    let received = use_memo(p.clone(), |_| js_sys::Date::now());
    let tick = use_state(|| 0u32);
    {
        let tick = tick.clone();
        use_effect_with((), move |_| {
            let interval = Interval::new(1000, move || tick.set(tick.wrapping_add(1)));
            move || drop(interval)
        });
    }
    if p["turn"].as_u64().unwrap_or(0) == 0 {
        return Html::default();
    }
    let counting = p["counting"] == true;
    let since = js_sys::Date::now() - *received;
    let elapsed = live_ms(p["elapsed_ms"].as_u64().unwrap_or(0), counting, since) / 1000;
    let in_turn = live_ms(p["turn_elapsed_ms"].as_u64().unwrap_or(0), counting, since) / 1000;
    let start = p["start_clock"].as_str().unwrap_or("");
    let scenario = match (p["scenario_span_s"].as_i64(), p["end_clock"].as_str()) {
        (Some(span), Some(end)) => {
            let frac = scenario_fraction(p["scenario_s"].as_i64().unwrap_or(0), span);
            let same_day = day(start) == day(end);
            let pct = (frac * 100.0).round();
            let title = crate::exfmt!(
                "Scenario simulato: {} → {} ({}%)",
                "Simulated scenario: {} → {} ({}%)",
                start,
                end,
                pct
            );
            html! {<span class="timer-scenario" id="timer-scenario" title={title.clone()} aria-label={title}>
                <span>{short_clock(start, same_day)}</span>
                <span class="timer-bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={pct.to_string()}><span style={format!("width:{:.1}%", frac * 100.0)}/></span>
                <span>{short_clock(end, same_day)}</span>
            </span>}
        }
        _ => Html::default(),
    };
    let elapsed_title = crate::i18n::ex("Tempo reale dall’avvio dell’esercitazione (le pause contano); tra parentesi, il turno aperto");
    html! {<div class="exercise-timers" id="exercise-timers">
        <span class="timer-real" id="timer-elapsed" title={elapsed_title} data-elapsed-s={elapsed.to_string()}>
            {crate::i18n::ex("Tempo reale")}{" "}<strong>{hms(elapsed as i64)}</strong>
            if counting {<span class="muted">{crate::exfmt!(" (turno {})", " (turn {})", hms(in_turn as i64))}</span>}
            else {<span class="muted">{crate::exfmt!(" · conclusa", " · ended")}</span>}
        </span>
        <span class="timer-turn" id="timer-turn">{turn_line(p)}</span>
        {scenario}
    </div>}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn durations_read_as_minutes_or_hours() {
        assert_eq!(hms(0), "0:00");
        assert_eq!(hms(-5), "0:00");
        assert_eq!(hms(760), "12:40");
        assert_eq!(hms(1800), "30:00");
        assert_eq!(hms(3735), "1:02:15");
    }

    #[test]
    fn live_counter_advances_only_while_counting() {
        assert_eq!(live_ms(60_000, true, 2_500.0), 62_500);
        assert_eq!(live_ms(60_000, false, 2_500.0), 60_000);
        assert_eq!(live_ms(60_000, true, -10.0), 60_000);
    }

    #[test]
    fn clocks_and_fraction() {
        assert_eq!(short_clock("Day 1 13:00", true), "13:00");
        assert_eq!(short_clock("Day 1 13:00", false), "Day 1 13:00");
        assert_eq!(day("Day 2 01:30"), "Day 2");
        assert_eq!(scenario_fraction(900, 3600), 0.25);
        assert_eq!(scenario_fraction(9000, 3600), 1.0);
        assert_eq!(scenario_fraction(10, 0), 0.0);
    }

    #[test]
    fn turn_line_in_both_languages() {
        let p = json!({"turn": 2, "planned_turns": 5, "turn_s": 760, "turn_len_s": 1800});
        crate::i18n::with_ui_lang("it", || {
            assert_eq!(turn_line(&p), "Turno 2/5 · 12:40 / 30:00 simulati");
        });
        crate::i18n::with_ui_lang("en", || {
            assert_eq!(turn_line(&p), "Turn 2/5 · 12:40 / 30:00 simulated");
            assert_eq!(crate::i18n::ex("Tempo reale"), "Real time");
        });
        let open = json!({"turn": 3, "planned_turns": null, "turn_s": 60, "turn_len_s": 900});
        crate::i18n::with_ui_lang("de", || {
            assert_eq!(turn_line(&open), "Turn 3 · 1:00 / 15:00 simulated");
        });
    }
}
