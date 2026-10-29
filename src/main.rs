//! Unblinking, in Rust: the eye in a small transparent window that stays on
//! top of everything. This file owns the window and the frame loop.
//!
//! Flags:
//!   --snap=MS        after MS milliseconds, save the frame to snap.png and quit
//!   --backend=NAME   force `dx12` or `vulkan` instead of trying both
//!   --camera         open "Where's my camera?" at start (also in the tray)
//!   --senses         open Blinki's Senses at start (also Ctrl+Alt+B, or the tray)
//!   --mood=NAME      hold one of the studio's moods (for looking at it, or captures)
//!   --demo           a scripted 14 s performance: moods, whispers, a flinch (no camera or mic)
//!   --record=F.gif   save the frames it draws as an animated GIF, then quit when it closes
//!   --whisper=TEXT   show TEXT as a whisper once the eye opens (for captures)
//!   --voice          turn all sound (music, voice, effects) on for this run, without saving it
//!   --blind          start blinded this run (camera and mic off), without saving it
//!
//! Left-drag moves the eye; right-click it (or the tray icon) for the menu.

// In release builds, don't open a console window next to the eye.
// `cfg_attr(condition, attribute)` applies the attribute only when the
// condition holds; `debug_assertions` is on in debug builds only.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Each `mod` line pulls in a file (`cli.rs`) or a folder (`app/mod.rs`)
// of the same name from src/.
mod app;
mod audio;
mod bond;
mod cli;
mod mind;
mod platform;
mod prefs;
mod render;
mod senses;
mod tools;
mod ui;

use std::error::Error;
use std::time::Instant;

use winit::event_loop::EventLoop;

use app::App;
use cli::Args;
use platform::session;
use senses::Detector;
use senses::face::YuNet;

fn main() -> Result<(), Box<dyn Error>> {
    // --bench-face: time the face model on a blank 1280x720 frame, then quit.
    if std::env::args().any(|a| a == "--bench-face") {
        let mut net = YuNet::new().map_err(|e| e.to_string())?;
        let frame = image::RgbImage::new(1280, 720);
        net.detect(&frame); // warm up
        let start = Instant::now();
        for _ in 0..50 {
            net.detect(&frame);
        }
        eprintln!(
            "face model: {:.1} ms per frame",
            start.elapsed().as_secs_f64() * 1000.0 / 50.0
        );
        return Ok(());
    }
    let args = Args::parse();
    // One eye at a time. A second launch wakes the first and leaves. Runs
    // with --snap, --record or --demo are tools, so they don't take part.
    let instance = if args.snap.is_some() || args.record.is_some() || args.demo {
        None
    } else {
        match session::SingleInstance::claim() {
            Some(instance) => Some(instance),
            None => {
                eprintln!("unblinking is already running; showing it");
                return Ok(());
            }
        }
    };
    let event_loop = EventLoop::new()?;
    let mut app = App::new(args, instance);
    event_loop.run_app(&mut app)?;
    app.finish_recording()
}
