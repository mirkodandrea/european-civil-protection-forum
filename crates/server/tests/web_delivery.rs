//! Rocket serves generated browser assets, including WebAssembly, byte for byte.
use rocket::http::{ContentType, Status};
use rocket::local::asynchronous::Client;
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn pages_and_assets_are_public_but_api_and_websocket_require_keys() {
    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let fixture =
        Fixture(std::env::temp_dir().join(format!("ttx-web-{:016x}", rand::random::<u64>())));
    let web = fixture.0.clone();
    std::fs::create_dir(&web).unwrap();
    for (name, bytes) in [
        (
            "index.html",
            b"<!doctype html><title>TTX</title>".as_slice(),
        ),
        ("frontend.js", b"// generated WebAssembly loader".as_slice()),
        ("frontend_bg.wasm", b"\0asm\x01\0\0\0".as_slice()),
        ("interface.css", b"body { margin: 0 }".as_slice()),
    ] {
        std::fs::write(web.join(name), bytes).unwrap();
    }
    for page in ["master", "player", "play", "advanced", "evaluator"] {
        std::fs::copy(web.join("index.html"), web.join(format!("{page}.html"))).unwrap();
    }
    let world = sim::World::new(Arc::new(scenario::synthetic::generate(1)), 1).unwrap();
    let (app, _) = server::router(
        Arc::new(Mutex::new(exercise::Exercise::new(world))),
        llm::Llm::off(),
        Some(web.clone()),
    );
    let client = Client::untracked(app).await.unwrap();
    for entry in std::fs::read_dir(&web).unwrap() {
        let path = entry.unwrap().path();
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !["html", "js", "css", "wasm"].contains(&extension) {
            continue;
        }
        let url = format!("/{}?v=rocket", path.file_name().unwrap().to_str().unwrap());
        let response = client.get(url).dispatch().await;
        assert_eq!(response.status(), Status::Ok, "{}", path.display());
        assert_eq!(
            response.content_type(),
            ContentType::from_extension(extension)
        );
        assert_eq!(
            response.into_bytes().await.unwrap(),
            std::fs::read(path).unwrap()
        );
    }
    let response = client.get("/").dispatch().await;
    assert_eq!(response.status(), Status::Ok);
    assert_eq!(
        response.into_bytes().await.unwrap(),
        std::fs::read(web.join("index.html")).unwrap()
    );
    assert_eq!(
        client
            .get("/api/health")
            .dispatch()
            .await
            .into_json::<serde_json::Value>()
            .await
            .unwrap()["ok"],
        true
    );
    for url in [
        "/api/state",
        "/api/master?token=invalid",
        "/ws?token=invalid",
    ] {
        let response = client.get(url).dispatch().await;
        assert_eq!(response.status(), Status::Unauthorized, "{url}");
        assert_eq!(
            response.into_json::<serde_json::Value>().await.unwrap()["error"],
            "access key required"
        );
    }
}
