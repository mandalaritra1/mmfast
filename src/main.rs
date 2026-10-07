//! mmfast: a small native Mattermost client.

mod api;
mod app;
mod backend;
mod config;
mod highlight;
mod icon;
mod images;
#[cfg(target_os = "macos")]
mod macos;
mod markup;
mod math;
mod model;
mod script;
mod store;
mod theme;
mod ui;
mod ws;

use clap::Parser;
use std::sync::OnceLock;

/// Process start, for the startup timings in the log.
pub static STARTED: OnceLock<std::time::Instant> = OnceLock::new();

pub fn since_start_ms() -> u128 {
    STARTED.get().map(|s| s.elapsed().as_millis()).unwrap_or(0)
}

#[derive(Parser, Debug, Clone)]
#[command(version, about = "A fast, native Mattermost client")]
pub struct Args {
    /// Use the seeded local test server (dev/seed-credentials.json).
    #[arg(long)]
    dev: bool,
    /// Scripted steps for checks and screenshots (see src/script.rs).
    #[arg(long)]
    script: Option<String>,
    /// light, dark or system.
    #[arg(long)]
    theme: Option<String>,
    /// Write the app icon's PNGs into DIR (for the macOS bundle) and exit.
    #[arg(long, hide = true)]
    export_icon: Option<std::path::PathBuf>,
    /// Initial window size, WIDTHxHEIGHT.
    #[arg(long, default_value = "1320x840")]
    size: String,
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,mmfast=info")).init();
    let _ = rustls::crypto::ring::default_provider().install_default();
    let started = *STARTED.get_or_init(std::time::Instant::now);
    let args = Args::parse();
    #[cfg(target_os = "macos")]
    macos::links::install();
    let (w, h) = args
        .size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?)))
        .unwrap_or((1320.0, 840.0));
    if let Some(dir) = &args.export_icon {
        icon::export(dir).expect("could not write the icon");
        return Ok(());
    }
    let waker = fastframe_shell::Waker::default();
    let app = app::App::new(args, &waker);
    fastframe_shell::Shell::new(app, &waker).idle(fastframe_tray::idle).run(|lease| {
        let mut options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("mmfast")
                .with_app_id("mmfast")
                .with_inner_size([w, h])
                .with_min_inner_size([760.0, 480.0])
                .with_icon(egui::IconData { rgba: icon::app_rgba(256), width: 256, height: 256 }),
            ..Default::default()
        };
        // A hidden or locked session may never present a frame; without
        // vsync the swap cannot block there (testing over SSH, say).
        options.glow_options.vsync = std::env::var_os("MMFAST_NO_VSYNC").is_none();
        eframe::run_native(
            "mmfast",
            options,
            Box::new(move |cc| {
                log::info!("window ready after {} ms", started.elapsed().as_millis());
                let mut app = lease.take(&cc.egui_ctx);
                app.attach(&cc.egui_ctx);
                Ok(Box::new(app::Window { app }))
            }),
        )
    })
}
