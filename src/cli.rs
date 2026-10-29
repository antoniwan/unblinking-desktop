//! The command line: the flags listed at the top of main.rs.

use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Backend {
    Dx12,
    Vulkan,
}

/// Command line options.
pub(crate) struct Args {
    pub(crate) snap: Option<Duration>,
    pub(crate) backend: Option<Backend>,
    pub(crate) blind: bool,
    pub(crate) mood: Option<String>,
    pub(crate) senses: bool,
    pub(crate) camera: bool,
    pub(crate) voice: bool,
    pub(crate) whisper: Option<String>,
    pub(crate) demo: bool,
    pub(crate) record: Option<String>,
}

impl Args {
    pub(crate) fn parse() -> Self {
        let mut args = Args {
            snap: None,
            backend: None,
            blind: false,
            mood: None,
            senses: false,
            camera: false,
            voice: false,
            whisper: None,
            demo: false,
            record: None,
        };
        for arg in std::env::args().skip(1) {
            // `if let` runs the block only when the pattern matches.
            if let Some(ms) = arg.strip_prefix("--snap=") {
                args.snap = Some(Duration::from_millis(ms.parse().unwrap_or(6000)));
            } else if arg == "--snap" {
                args.snap = Some(Duration::from_millis(6000));
            } else if let Some(name) = arg.strip_prefix("--backend=") {
                args.backend = match name {
                    "dx12" => Some(Backend::Dx12),
                    "vulkan" => Some(Backend::Vulkan),
                    other => {
                        eprintln!("unknown backend {other:?}; trying both");
                        None
                    }
                };
            } else if let Some(name) = arg.strip_prefix("--mood=") {
                args.mood = Some(name.to_string());
            } else if let Some(text) = arg.strip_prefix("--whisper=") {
                args.whisper = Some(text.to_string());
            } else if arg == "--demo" {
                args.demo = true;
            } else if let Some(path) = arg.strip_prefix("--record=") {
                args.record = Some(path.to_string());
            } else if arg == "--voice" {
                args.voice = true;
            } else if arg == "--camera" {
                args.camera = true;
            } else if arg == "--senses" {
                args.senses = true;
            } else if arg == "--blind" {
                args.blind = true;
            } else if arg == "--bench-face" {
                // Handled before the app starts.
            } else {
                eprintln!("ignoring unknown argument {arg:?}");
            }
        }
        args
    }
}
