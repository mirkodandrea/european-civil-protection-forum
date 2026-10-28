use crate::{
    api::{list, text},
    i18n, App,
};
use yew::prelude::*;

fn number_or_text(value: &serde_json::Value) -> String {
    value
        .as_u64()
        .map(|n| n.to_string())
        .or_else(|| value.as_i64().map(|n| n.to_string()))
        .or_else(|| value.as_f64().map(|n| n.to_string()))
        .unwrap_or_default()
}

/// `**bold**` spans inside one line of plain text.
fn inline(line: &str) -> Html {
    html! {<>{for line.split("**").enumerate().map(|(i, part)| if i % 2 == 1 {html!{<strong>{part.to_string()}</strong>}} else {html!{{part.to_string()}}})}</>}
}

/// Plain LLM text as paragraphs and bullet lists (`-`/`*`/`•` start a bullet; consecutive bullets share one list).
fn summary_body(text: &str) -> Html {
    let bullet = |l: &str| l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).or_else(|| l.strip_prefix("• ")).map(str::to_string);
    let mut blocks: Vec<Html> = Vec::new();
    let mut items: Vec<Html> = Vec::new();
    let flush = |items: &mut Vec<Html>, blocks: &mut Vec<Html>| {
        if !items.is_empty() {
            blocks.push(html! {<ul>{for std::mem::take(items)}</ul>});
        }
    };
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        match bullet(line) {
            Some(b) => items.push(html! {<li>{inline(&b)}</li>}),
            None => {
                flush(&mut items, &mut blocks);
                blocks.push(html! {<p>{inline(line)}</p>});
            }
        }
    }
    flush(&mut items, &mut blocks);
    html! {<div class="summary-body">{for blocks}</div>}
}

impl App {
    pub fn view_function(&self) -> Html {
        let lang = self.lang();
        let role = &self.state["role"];
        let allowed = list(&role["allowed_actions"]);
        let accounting = &self.state["resource_accounting"];
        let resources = accounting
            .as_object()
            .into_iter()
            .flat_map(|o| o.iter())
            .collect::<Vec<_>>();
        let deployments = list(&self.state["deployments"]);
        let mut summaries = list(&self.state["turn_summaries"]).to_vec();
        summaries.sort_by(|a, b| {
            (b["turn"].as_u64().unwrap_or(0), text(&b["clock"]))
                .cmp(&(a["turn"].as_u64().unwrap_or(0), text(&a["clock"])))
        });
        let summary_data_missing = !self.state["turn_summaries"].is_array();

        html! {
            <main id="function-view" class="page function-view" hidden={self.active!="function"}>
                <section class="card function-role">
                    <div class="eyebrow">{i18n::t(&lang,"function_role")}</div>
                    <h2>{text(&role["name"])}</h2>
                    if text(&role["description"]).is_empty() {<p class="muted">{i18n::t(&lang,"function_no_description")}</p>}
                    else {<p>{text(&role["description"])}</p>}
                    <h3>{i18n::t(&lang,"function_authority")}</h3>
                    if allowed.is_empty() {<p class="muted">{i18n::t(&lang,"function_no_actions")}</p>}
                    else {<ul>{for allowed.iter().map(|a| html!{<li>{i18n::action(&lang,text(a))}</li>})}</ul>}
                </section>

                <section class="card">
                    <h2>{i18n::t(&lang,"function_resources")}</h2>
                    if resources.is_empty() {<p class="muted">{i18n::t(&lang,"function_no_resources")}</p>}
                    else {<div class="function-resource-list">{for resources.iter().map(|(id,entry)| {
                        let metadata = list(&self.stat["resource_types"]).iter().find(|r| text(&r["id"]) == id.as_str());
                        let name = metadata.map(|r| text(&r["name"])).filter(|s| !s.is_empty()).unwrap_or(id.as_str());
                        let description = metadata.map(|r| text(&r["description"])).unwrap_or("");
                        html! {<article class="function-resource" key={id.to_string()}>
                            <div><h3>{name}</h3>{if !description.is_empty(){html!{<p class="muted">{description}</p>}}else{Html::default()}}</div>
                            <dl><div><dt>{i18n::t(&lang,"function_available")}</dt><dd>{number_or_text(&entry["available"])}</dd></div>
                            <div><dt>{i18n::t(&lang,"function_committed")}</dt><dd>{number_or_text(&entry["committed"])}</dd></div>
                            <div><dt>{i18n::t(&lang,"function_deployed")}</dt><dd>{number_or_text(&entry["deployed"])}</dd></div></dl>
                        </article>}
                    })}</div>}
                </section>

                <section class="card">
                    <h2>{i18n::t(&lang,"function_missions")}</h2>
                    if deployments.is_empty() {<p class="muted">{i18n::t(&lang,"function_no_missions")}</p>}
                    else {<ul class="function-missions">{for deployments.iter().map(|deployment| {
                        let task = &deployment["task"];
                        let resource_id = text(&task["resource"]);
                        let resource = list(&self.stat["resource_types"]).iter().find(|r| text(&r["id"]) == resource_id);
                        let resource_name = resource.map(|r| text(&r["name"]).to_string()).unwrap_or_else(|| resource_id.to_string());
                        let drone = resource.is_some_and(|r| r["drone"].is_object() || resource_id == "drones");
                        let status = match (drone, text(&task["status"])) {
                            (true, "en_route") => i18n::t(&lang,"mission_drone_en_route"),
                            (true, "working") => i18n::t(&lang,"mission_drone_on_station"),
                            (true, "refilling") => i18n::t(&lang,"mission_drone_recharging"),
                            (_, other) => i18n::mission_status(&lang,other),
                        };
                        html! {<li key={task["id"].to_string()}>
                            <div class="function-mission-heading"><strong>{resource_name}</strong><span class="pill">{status}</span></div>
                            if task["qty"].is_number() {<p>{i18n::tf(&lang,"function_mission_quantity",&[&number_or_text(&task["qty"])])}</p>}
                            if !text(&task["detail"]).is_empty() {<p>{text(&task["detail"])}</p>}
                        </li>}
                    })}</ul>}
                </section>

                <section class="card">
                    <h2>{i18n::t(&lang,"function_summaries")}</h2>
                    if summary_data_missing || summaries.is_empty() {<p class="muted">{i18n::t(&lang,"function_no_summaries")}</p>}
                    else {<ol class="function-summaries">{for summaries.iter().enumerate().map(|(idx, summary)| {
                        let turn = number_or_text(&summary["turn"]);
                        let raw_status = text(&summary["status"]);
                        let status = if raw_status.is_empty() {i18n::t(&lang,"function_status_unknown")} else {i18n::summary_status(&lang,raw_status)};
                        let body = if text(&summary["text"]).is_empty() {html!{<p class="muted">{i18n::t(&lang,"function_summary_empty")}</p>}} else {summary_body(text(&summary["text"]))};
                        html! {<li key={format!("{}:{}",turn,text(&summary["clock"]))} class={classes!("summary-card", format!("summary-{raw_status}"))}>
                            <details open={idx == 0}>
                                <summary><span class="summary-turn">{turn.clone()}</span><strong>{i18n::tf(&lang,"function_turn_clock",&[&turn,text(&summary["clock"])])}</strong><span class="pill">{status}</span></summary>
                                {body}
                            </details>
                        </li>}
                    })}</ol>}
                </section>
            </main>
        }
    }
}
