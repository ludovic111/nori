//! nori: the desktop window (GPUI) around the Rust core.
//!
//! The window is one client of the command registry in `nori-control`, like the built-in agent,
//! `nori-cli` and `nori-mcp`; it starts the session, the loopback bridge those tools use, and
//! writes the lsuite discovery file.

// No console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod app;
mod assets;
mod store;
mod theme;
mod ui;
mod views;

#[cfg(test)]
mod tests;

use gpui::{App, AppContext as _, Bounds, TitlebarOptions, WindowBackgroundAppearance, WindowBounds, WindowOptions, point, px, size};
use nori_control::{Session, SessionOptions};

fn main() {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("nori {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let data_dir = nori_control::session::default_data_dir();
    let config_dir = nori_control::session::default_config_dir();
    let level = nori_control::Settings::load(&config_dir).diagnostics.log_level;
    let _started = nori_control::diagnostics::init(&data_dir, "nori", &level, true);

    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().thread_name("nori-worker").build().expect("tokio runtime");
    let session = {
        let _guard = runtime.enter();
        Session::new(SessionOptions { secrets: Some(nori_control::secrets::default_store()), headless: false, ..Default::default() })
            .unwrap_or_else(|e| fatal(&format!("nori couldn't create its folders in {} or {}: {e}", data_dir.display(), config_dir.display())))
    };

    // The bridge for nori-cli and nori-mcp, and the lsuite discovery entry.
    let bridge = runtime.block_on(nori_control::bridge::Server::start(session.clone()));
    let running = match &bridge {
        Ok(server) => Some(nori_control::discovery::Running { pid: std::process::id(), control_file: Some(server.path().to_path_buf()), port: Some(server.port()), since: chrono::Utc::now() }),
        Err(e) => {
            tracing::warn!("the control bridge couldn't start: {e}");
            None
        }
    };
    if let Err(e) = nori_control::discovery::write(&nori_control::discovery::entry(&session.data_dir, running)) {
        tracing::warn!("couldn't write ~/.lsuite/apps/nori.json: {e}");
    }

    #[cfg(unix)]
    {
        let data_dir = session.data_dir.clone();
        runtime.spawn(async move {
            use tokio::signal::unix::{SignalKind, signal};
            let (Ok(mut term), Ok(mut hup), Ok(mut int)) = (signal(SignalKind::terminate()), signal(SignalKind::hangup()), signal(SignalKind::interrupt())) else {
                return;
            };
            tokio::select! {
                _ = term.recv() => {}
                _ = hup.recv() => {}
                _ = int.recv() => {}
            }
            tracing::info!("asked to stop by the system");
            let _ = nori_control::discovery::write(&nori_control::discovery::entry(&data_dir, None));
            nori_control::diagnostics::clean_exit();
            std::process::exit(0);
        });
    }

    // Files given on the command line (or by the OS) open at start.
    let open_at_start: Option<std::path::PathBuf> = std::env::args().skip(1).find(|a| !a.starts_with('-')).map(std::path::PathBuf::from);

    let handle = runtime.handle().clone();
    let bridge = parking_lot::Mutex::new(bridge.ok());
    gpui_platform::application().with_assets(assets::Assets).run(move |cx: &mut App| {
        gpui_tokio::init_from_handle(cx, handle);
        assets::load_fonts(cx);
        app::init(session.clone(), cx);
        open_main_window(cx);
        cx.activate(true);
        app::welcome(&session, open_at_start.clone(), cx);

        let s = session.clone();
        cx.on_app_quit(move |_| {
            let _ = nori_control::discovery::write(&nori_control::discovery::entry(&s.data_dir, None));
            drop(bridge.lock().take());
            nori_control::diagnostics::clean_exit();
            async {}
        })
        .detach();
    });
    nori_control::diagnostics::clean_exit();
    drop(runtime);
}

fn fatal(message: &str) -> ! {
    tracing::error!("{message}");
    eprintln!("{message}");
    std::process::exit(1)
}

pub fn open_main_window(cx: &mut App) {
    // `NORI_WINDOW_SIZE=2000x1250` opens the window at that size (screenshots, tests).
    let (w, h) = std::env::var("NORI_WINDOW_SIZE")
        .ok()
        .and_then(|v| v.split_once('x').and_then(|(w, h)| Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))))
        .unwrap_or_else(|| {
            let screen = cx.primary_display().map(|d| d.visible_bounds().size);
            let fit = |want: f32, room: Option<f32>, min: f32| room.map_or(want, |r| want.min(r * 0.92)).max(min);
            (fit(1520., screen.map(|s| f32::from(s.width)), 960.), fit(940., screen.map(|s| f32::from(s.height)), 620.))
        });
    let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
    let transparent = cx.global::<theme::Theme>().transparent;
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions { title: Some("nori".into()), appears_transparent: true, traffic_light_position: Some(point(px(16.), px(17.))) }),
        focus: true,
        show: true,
        window_min_size: Some(size(px(960.), px(620.))),
        window_background: if transparent { WindowBackgroundAppearance::Blurred } else { WindowBackgroundAppearance::Opaque },
        app_id: Some("nori".into()),
        icon: image::load_from_memory(include_bytes!("../resources/nori.png")).ok().map(|i| std::sync::Arc::new(i.to_rgba8())),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| cx.new(|cx| app::Workspace::new(window, cx))).expect("couldn't open the nori window");
}
