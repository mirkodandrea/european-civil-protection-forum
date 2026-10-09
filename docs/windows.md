# Windows installation and launch

Windows 11 with **Rust `x86_64-pc-windows-msvc`** is the supported Windows setup.
Install Visual Studio 2022 Build Tools, selecting **Desktop development with C++**
(the MSVC C++ compiler and Windows 10/11 SDK), then install Rust through rustup.
In a new PowerShell terminal:

```powershell
rustup toolchain install stable-x86_64-pc-windows-msvc
rustup default stable-x86_64-pc-windows-msvc
```

Clone the repository and optionally put `OPENROUTER_API_KEY=...` in its `.env`.
Without a key, manual exercise control works. From the repository:

```powershell
# Web dashboards, without linking Bevy/WGPU:
powershell -ExecutionPolicy Bypass -File .\scripts\start-windows.ps1
# The 3D window and dashboards share one exercise process:
powershell -ExecutionPolicy Bypass -File .\scripts\start-windows.ps1 -Visualization
```

The launcher builds current source incrementally with Cargo's locked dependency
versions, uses the repository as the process working directory (including `.env`,
web pages, scenario and assets), checks `/api/state` and all dashboards, then
opens `play.html`. Keep the terminal open; Ctrl+C stops the exercise. The first
3D build takes substantially longer. Use `-Port 8081` for a different port;
a port already in use is rejected so an old process cannot masquerade as the
new simulation. `-NoBrowser` skips opening a tab. `-SmokeTest` checks startup
and stops the process, for CI or installation diagnosis.

Equivalent manual commands (run from the repository root):

```powershell
cargo build --locked --release -p app --no-default-features --target x86_64-pc-windows-msvc
.\target\x86_64-pc-windows-msvc\release\ttx.exe --headless --port 8080
# Visualization enabled:
cargo build --locked --release -p app --target x86_64-pc-windows-msvc
.\target\x86_64-pc-windows-msvc\release\ttx.exe --port 8080
# Full workspace tests:
cargo test --locked --release --workspace --target x86_64-pc-windows-msvc
```

## GNU/MinGW compatibility

`x86_64-pc-windows-gnu` is **unsupported** for this application. Issue #1 records
that Rust 1.98.1 / GCC-MinGW 16.2.0 builds the default binary, but `winit 0.30.13`
segfaults in `INIT_MAIN_THREAD_ID::initer` during CRT initialization, before
Rust enters `main` (`0xc0000005`). `--headless` cannot avoid a pre-main failure
in a visualization-linked binary. A no-default-features GNU build worked in
that report, but is not a supported installation path. The same revision's
MSVC visualization build was confirmed to open the window and serve the API.
Use MSVC; this repository does not claim to fix the upstream GNU initializer.
Reproduction and debugger evidence remain in GitHub issue #1.
