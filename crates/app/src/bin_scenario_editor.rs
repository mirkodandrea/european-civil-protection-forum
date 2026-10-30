//! Native authoring app; no HTTP, LLM or exercise starts here.
use scenario::Scenario;
use std::path::PathBuf;
struct Files(PathBuf);
impl viz::editor::Storage for Files {
    fn list(&self) -> Result<Vec<Scenario>, String> {
        let entries = std::fs::read_dir(&self.0).map_err(|e| e.to_string())?;
        let mut scenes = vec![];
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|x| x == "json")
                && !path.to_string_lossy().ends_with(".pack.json")
            {
                if let Ok(s) = Scenario::load(&path) {
                    scenes.push(s);
                }
            } else if path.to_string_lossy().ends_with(".pack.json") {
                if let Ok(text) = std::fs::read_to_string(&path) {
                    if let Ok(pack) = serde_json::from_str::<scenario::ScenarioPack>(&text) {
                        if let Ok(base) = Scenario::load(self.0.join(format!("{}.json", pack.base)))
                        {
                            if let Ok(mut s) = pack.build(base) {
                                s.id = pack.id;
                                s.name = pack.name;
                                scenes.push(s);
                            }
                        }
                    }
                }
            }
        }
        scenes.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(scenes)
    }
    fn save(&self, s: &Scenario) -> Result<(), String> {
        scenario::editor::validate(s).map_err(|e| e.to_string())?;
        // Native writes are atomic. Authored copies have their own territory id.
        let path = self.0.join(format!("{}.json", s.id));
        if self.0.join(format!("{}.pack.json", s.id)).exists() {
            return Err("A pack with this ID already exists: clone it with a new identifier".into());
        }
        let temp = self.0.join(format!("{}.json.tmp", s.id));
        s.save(&temp).map_err(|e| e.to_string())?;
        std::fs::rename(temp, path).map_err(|e| e.to_string())
    }
}
fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    let mut dir = PathBuf::from("data/scenarios");
    let mut input = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenarios-dir" => {
                dir = PathBuf::from(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("missing directory"))?,
                )
            }
            "--scenario" => {
                input = Some(
                    args.next()
                        .ok_or_else(|| anyhow::anyhow!("missing scenario id"))?,
                )
            }
            "--help" | "-h" => {
                println!("scenario-editor [--scenarios-dir DIR] [--scenario ID]");
                return Ok(());
            }
            _ => anyhow::bail!("unknown argument: {arg}"),
        }
    }
    std::fs::create_dir_all(&dir)?;
    let files = Files(dir);
    use viz::editor::Storage;
    let scene = if let Some(id) = input {
        files
            .list()
            .map_err(anyhow::Error::msg)?
            .into_iter()
            .find(|s| s.id == id)
            .ok_or_else(|| anyhow::anyhow!("scenario not found: {id}"))?
    } else {
        scenario::editor::blank("nuovo-scenario", 64, 20.)?
    };
    viz::editor::run(scene, Box::new(files)).map_err(anyhow::Error::msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use viz::editor::Storage;
    #[test]
    fn library_save_reopens_complete_document_and_rejects_pack_collision() {
        let dir = std::env::temp_dir().join(format!("ttx-editor-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let files = Files(dir.clone());
        let mut d =
            scenario::editor::Document::new(scenario::editor::blank("authored", 16, 20.).unwrap())
                .unwrap();
        d.add("building", [100., 100.], None).unwrap();
        files.save(&d.scene).unwrap();
        let reopened = Scenario::load(dir.join("authored.json")).unwrap();
        assert_eq!(
            serde_json::to_value(&reopened).unwrap(),
            serde_json::to_value(&*d.scene).unwrap()
        );
        assert_eq!(files.list().unwrap().len(), 1);
        std::fs::write(dir.join("authored.pack.json"), "{}").unwrap();
        assert!(files.save(&d.scene).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
