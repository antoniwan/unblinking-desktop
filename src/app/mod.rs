//! The whole app: one `App` that winit calls into. Split by job:
//! `frame` (one frame of the eye), `actions` (menu and windows),
//! `presence` (opening and closing), `events` (winit's callbacks),
//! `capture` (frames read back for --snap and --record).

mod actions;
mod capture;
mod events;
mod frame;
mod pacing;
mod presence;

use presence::{AfterClose, Presence};

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::{Duration, Instant};

use global_hotkey::GlobalHotKeyManager;
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use winit::window::Window;

use crate::audio::sound;
use crate::audio::voice::Voice;
use crate::bond::remembrance::{self, Remembrance};
use crate::bond::wellbeing::Watch;
use crate::cli::Args;
use crate::mind::feeling::Feeling;
use crate::mind::moods::Moods;
use crate::mind::motion;
use crate::mind::{self, Line, Mind, Mode, Vec2, World};
use crate::platform::desktop::{self, Point, Rect};
use crate::platform::fullscreen;
use crate::platform::session;
use crate::platform::tray::Tray;
use crate::prefs::Prefs;
use crate::render::gpu::Gpu;
use crate::senses::camera::Eyes;
use crate::senses::expression::Expression;
use crate::senses::mic::Ears;
use crate::senses::placement::{self, Monitor};
use crate::senses::wake::Wake;
use crate::tools::{demo, record};
use crate::ui::camera_picker::CameraPicker;
use crate::ui::senses_panel::{SensesPanel, Snapshot};

/// The eye sits a little above center so whispers fit below it.
const CENTER: [f32; 2] = [0.0, 0.06];
/// Wait this long after the last move before saving where the eye was dropped.
const SAVE_MOVE_AFTER: Duration = Duration::from_millis(400);

/// The whole app. winit calls into it through `ApplicationHandler`.
/// Hours after midnight before this count as the small hours.
const LATE_UNTIL_HOUR: u32 = 5;

pub(crate) struct App {
    args: Args,
    gpu: Option<Gpu>,
    started: Instant,
    last_frame: Instant,
    /// When the next frame is due (see pacing.rs), and frames drawn since
    /// the last heartbeat line, to print the frame rate.
    next_frame: Instant,
    frames: u32,
    mind: Mind,
    /// The body that carries out the mind's wishes: weight, breath, flinch.
    motion: motion::Motion,
    mode: Option<Mode>,
    /// The camera thread, while the eye is allowed to see.
    eyes: Option<Eyes>,
    /// The microphone, while the eye is allowed to hear.
    ears: Option<Ears>,
    /// Listening for "Hey, OG Blinki", while that's on and it isn't blind.
    wake: Option<Wake>,
    prefs: Prefs,
    tray: Option<Tray>,
    presence: Presence,
    /// When to save the window's spot, a moment after the last move.
    save_move_at: Option<Instant>,
    last_beat: Instant,
    moods: Moods,
    /// Picks the mood from the moment, your face and its temperament.
    feeling: Feeling,
    /// What it remembers of your days, while "Remember our days" is on.
    book: Option<Remembrance>,
    /// Notices breaks, returns and smiles, for the book.
    watch: Watch,
    /// The latest reading of your face, while "Read my face" is on.
    expression: Option<Expression>,
    /// The flame drift's looping phase, 0..1, advanced at the mood's speed.
    flow: f32,
    /// The latest whisper and when, shared with the mind's `say` closure.
    /// `Rc<Cell<..>>`: both own it (Rc), and a Copy value inside a Cell can
    /// be swapped even through a shared handle.
    last_said: Rc<Cell<Option<(Line, Instant)>>>,
    /// When it last saw a face, and when it last heard anything louder than quiet.
    last_face: Option<Instant>,
    last_noise: Instant,
    /// The screen's pixels per metre, to place the eye in the room.
    px_per_m: f64,
    /// Blinki's Senses, while it's open, and what it shows.
    panel: Option<SensesPanel>,
    /// "Where's my camera?", while it's open.
    picker: Option<CameraPicker>,
    /// The monitors as they're laid out, for placing the camera.
    monitors: Vec<Monitor>,
    snapshot: Option<Snapshot>,
    levels: VecDeque<f32>,
    /// One flag per entry in `levels`: did it flinch on that frame?
    flinch_marks: VecDeque<bool>,
    last_startle: f64,
    /// Every whisper so far, for the Mind lens.
    #[allow(clippy::type_complexity)]
    whisper_log: Rc<RefCell<Vec<(Instant, Line, &'static str)>>>,
    /// The drone and spoken whispers, and how many logged whispers it has seen.
    voice: Voice,
    spoken: usize,
    /// A whisper to put on screen once the frame's GPU work is at hand.
    pending_whisper: Option<String>,
    /// The startle the flinch effect last played for.
    last_flinch_sound: f64,
    /// --demo's script, while it plays.
    demo: Option<demo::Demo>,
    /// --record's frames, written out when the app ends.
    recording: Option<record::Recording>,
    /// Seconds you've been up and here past midnight tonight, and when you
    /// were last here (face or keyboard/mouse), for the Worried mood.
    up_late: f32,
    last_here: Instant,
    /// When it last told you to rest, so it doesn't nag.
    last_worry: Option<Instant>,
    /// Proof this is the only copy running; another launch pokes it awake.
    instance: Option<session::SingleInstance>,
    /// Owns the Ctrl+Alt+B shortcut; dropping it would unregister it.
    _hotkeys: Option<GlobalHotKeyManager>,
    /// Hides the eye during full-screen games, and the pout after.
    fullscreen: fullscreen::Watcher,
}

impl App {
    /// Builds the app: loads the prefs (with this run's flags on top), the
    /// moods, what it remembers, and the mind with its whisper log.
    pub(crate) fn new(args: Args, instance: Option<session::SingleInstance>) -> App {
        // For now the mind's whispers go to the terminal; the app also notes the
        // latest one, because some moods follow what was just said.
        let last_said = Rc::new(Cell::new(None));
        let heard = Rc::clone(&last_said);
        let whisper_log = Rc::new(RefCell::new(Vec::new()));
        let log = Rc::clone(&whisper_log);
        let say: mind::Say = Box::new(move |text, kind| {
            eprintln!("whisper ({kind:?}): {text}");
            heard.set(Some((kind, Instant::now())));
            log.borrow_mut().push((Instant::now(), kind, text));
        });
        // Ctrl+Alt+B opens Blinki's Senses from anywhere. If another app has
        // the shortcut, the tray item still works.
        let hotkeys = GlobalHotKeyManager::new().ok().filter(|m| {
            let key = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::KeyB);
            m.register(key)
                .map_err(|err| eprintln!("no Ctrl+Alt+B: {err}"))
                .is_ok()
        });
        let now = Instant::now();
        let mut prefs = Prefs::load();
        // --blind blinds this run only; the saved pref is untouched unless the
        // menu changes something.
        prefs.blind |= args.blind;
        if args.voice {
            // --voice: everything audible, this run only.
            prefs.sound = sound::Sound {
                master: true,
                music: true,
                voice: true,
                fx: true,
                ..prefs.sound
            };
        }
        let moods = Moods::of_theme(prefs.theme.as_deref());
        let book = prefs
            .remember
            .then(|| Remembrance::load(remembrance::unix_now()));
        let mut app = App {
            args,
            gpu: None,
            started: now,
            last_frame: now,
            next_frame: now,
            frames: 0,
            mind: Mind::new(say),
            motion: motion::Motion::new(),
            mode: None,
            eyes: None,
            ears: None,
            wake: None,
            prefs,
            tray: None,
            presence: Presence::Hidden,
            save_move_at: None,
            last_beat: now,
            moods,
            feeling: Feeling::new(),
            book,
            watch: Watch::new(),
            expression: None,
            flow: 0.0,
            last_said,
            last_face: None,
            last_noise: now,
            px_per_m: {
                let ppm = desktop::pixels_per_metre();
                eprintln!("screen: {ppm:.0} pixels per metre");
                ppm
            },
            panel: None,
            picker: None,
            monitors: Vec::new(),
            snapshot: None,
            levels: VecDeque::new(),
            flinch_marks: VecDeque::new(),
            last_startle: f64::NEG_INFINITY,
            whisper_log,
            voice: Voice::new(),
            spoken: 0,
            pending_whisper: None,
            last_flinch_sound: f64::NEG_INFINITY,
            demo: None,
            recording: None,
            up_late: 0.0,
            last_here: now,
            last_worry: None,
            _hotkeys: hotkeys,
            instance,
            fullscreen: fullscreen::Watcher::new(),
        };
        app.apply_sound();
        app.recording = app.args.record.clone().map(record::Recording::new);
        if app.recording.is_some() {
            // A GIF has no sound, so a recording plays none (and saves nothing).
            app.prefs.sound.master = false;
            app.apply_sound();
        }
        app
    }

    /// Where the cursor, the eye and its monitor are right now.
    fn world(&self, window: &Window) -> Option<World> {
        let cursor = desktop::cursor()?;
        let pos = window.outer_position().ok()?;
        let size = window.inner_size();
        let monitor = window.current_monitor()?;
        let (mp, ms) = (monitor.position(), monitor.size());
        // Where the webcam is: the spot picked in "Where's my camera?", or
        // the top middle of the main screen.
        let (camera, px_per_m) = placement::locate(self.prefs.camera.as_ref(), &self.monitors)
            .unwrap_or((
                Point {
                    x: mp.x as f64 + ms.width as f64 / 2.0,
                    y: mp.y as f64,
                },
                self.px_per_m,
            ));
        Some(World {
            cursor,
            eye: Point {
                x: pos.x as f64 + size.width as f64 / 2.0,
                y: pos.y as f64 + size.height as f64 * 0.42,
            },
            display: Rect {
                x: mp.x as f64,
                y: mp.y as f64,
                width: ms.width as f64,
                height: ms.height as f64,
            },
            scale: window.scale_factor(),
            camera,
            px_per_m,
            look_offset: self
                .prefs
                .look_offset
                .map_or_else(Vec2::default, |p| Vec2::new(p.x, p.y)),
        })
    }

    /// Reads the monitors' layout and sizes, for placing the camera.
    fn refresh_monitors(&mut self, window: &Window) {
        let primary = window.primary_monitor();
        self.monitors = window
            .available_monitors()
            .map(|m| {
                let (p, s) = (m.position(), m.size());
                let name = m.name().unwrap_or_default();
                Monitor {
                    px_per_m: desktop::monitor_px_per_m(&name, s.width as f64),
                    primary: primary.as_ref() == Some(&m),
                    rect: Rect {
                        x: p.x as f64,
                        y: p.y as f64,
                        width: s.width as f64,
                        height: s.height as f64,
                    },
                    name,
                }
            })
            .collect();
    }
}
