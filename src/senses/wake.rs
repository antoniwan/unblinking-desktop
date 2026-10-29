//! The wake word: "Hey, OG Blinki" (or "Hey Blinki") calls the eye.
//!
//! Opt-in and fully on the computer. It uses the speech recognizer built
//! into Windows (`Windows.Media.SpeechRecognition`) with a short list of
//! phrases to listen for. A list like that is matched by the installed
//! speech language pack, locally: no cloud, no account, no API key, and
//! nothing is recorded. The recognizer only ever says "I heard one of your
//! phrases", never what else was said.
//!
//! Like the camera, it runs on its own thread and talks to the app over a
//! channel. Dropping the [`Wake`] stops the recognizer and frees the mic.

use std::sync::mpsc::{self, Receiver, Sender};

/// What the listening thread tells the app.
#[derive(Clone, Debug, PartialEq)]
pub enum WakeEvent {
    /// Someone said a wake phrase. Carries the words, for the log.
    /// Only the Windows recognizer sends it.
    #[cfg_attr(not(windows), allow(dead_code))]
    Heard(String),
    /// The recognizer could not start or stopped for good, and why.
    /// The app turns the feature off and says so once.
    Failed(String),
}

/// The open "ears for names". Dropping it stops listening.
pub struct Wake {
    events: Receiver<WakeEvent>,
    /// The extra name it answers to, if any.
    name: Option<String>,
    // Kept so `Drop` can tell the thread to stop.
    control: Sender<Control>,
}

/// Messages to the listening thread.
// Only the Windows listener reads these; elsewhere it's never built.
#[cfg_attr(not(windows), allow(dead_code))]
enum Control {
    /// The session ended by itself (a long silence, a hiccup): start again.
    Restart(String),
    /// The app is done with us: stop and free the mic.
    Stop,
}

impl Wake {
    /// Starts listening on a new thread. `name` is an extra name to answer
    /// to ("Hey <name>"), if the user gave the eye one.
    pub fn start(name: Option<String>) -> Wake {
        let (events_tx, events) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let restart = stop_tx.clone();
        let words = phrases(name.as_deref());
        let spawned = std::thread::Builder::new()
            .name("wake word".into())
            .spawn(move || listen(&words, &events_tx, &restart, &stop_rx));
        if let Err(err) = spawned {
            // No thread, no listening; the channel is closed so `poll`
            // would never say why, so say it here.
            eprintln!("wake word: no thread: {err}");
        }
        Wake {
            events,
            name,
            control: stop_tx,
        }
    }

    /// Everything heard since the last call. Never waits. Each heard
    /// phrase goes through [`is_wake`] once more, so only a real call counts
    /// whatever the recognizer hands back.
    pub fn poll(&self) -> Vec<WakeEvent> {
        self.events
            .try_iter()
            .filter(|e| match e {
                WakeEvent::Heard(text) => is_wake(text, self.name.as_deref()),
                WakeEvent::Failed(_) => true,
            })
            .collect()
    }
}

/// `Drop` runs when a value goes away, like a destructor. The thread also
/// holds a Sender (for restarts), so the channel never closes on its own:
/// we say "stop" out loud. It doesn't wait for the thread, so blinding
/// never stalls the eye; the mic is freed a moment later.
impl Drop for Wake {
    fn drop(&mut self) {
        let _ = self.control.send(Control::Stop);
    }
}

/// The phrases the recognizer listens for. "Blinki" is not a word it
/// knows, so the spellings it might guess a pronunciation from best are
/// all in: "blinki" and "blinky", "OG" said as a word or as two letters.
pub fn phrases(name: Option<&str>) -> Vec<String> {
    let mut list: Vec<String> = [
        "hey og blinki",
        "hey o g blinki",
        "hey og blinky",
        "hey o g blinky",
        "hey blinki",
        "hey blinky",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if let Some(name) = name.map(normalise).filter(|n| !n.is_empty()) {
        list.push(format!("hey {name}"));
    }
    list
}

/// Lower case, letters and digits only, single spaces: "Hey, OG Blinki!"
/// becomes "hey og blinki".
pub fn normalise(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() {
                c.to_lowercase().next().unwrap_or(c)
            } else {
                ' '
            }
        })
        .collect();
    // `split_whitespace` skips runs of spaces, so joining with one space
    // collapses them.
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Is `heard` a call for the eye? Accepts "hey og blinki", "hey o g
/// blinki", "hey blinki" (or "blinky") and "hey <name>", ignoring case and
/// punctuation. Anything before or after the call is fine ("oh, hey
/// blinki, there you are"), but the words must come in order and together.
pub fn is_wake(heard: &str, name: Option<&str>) -> bool {
    let heard = normalise(heard);
    // Treat "blinky" as "blinki", and "o g" as "og", so there's one spelling.
    let words: Vec<&str> = heard
        .split(' ')
        .map(|w| if w == "blinky" { "blinki" } else { w })
        .collect();
    // Padded with spaces so " hey blinki " can't match inside "they blinkin".
    let padded = format!(" {} ", words.join(" ")).replace(" o g ", " og ");
    if padded.contains(" hey og blinki ") || padded.contains(" hey blinki ") {
        return true;
    }
    match name.map(normalise).filter(|n| !n.is_empty()) {
        Some(name) => padded.contains(&format!(" hey {name} ")),
        None => false,
    }
}

/// The thread's body: start the recognizer, report what it hears, and
/// stop when the app drops its [`Wake`].
#[cfg(windows)]
fn listen(
    phrases: &[String],
    events: &Sender<WakeEvent>,
    restart: &Sender<Control>,
    control: &Receiver<Control>,
) {
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

    // WinRT objects need the thread to join the "multithreaded apartment"
    // first, the COM way of saying "objects here may be used from any
    // thread". It's per thread, which is one reason this has its own.
    // Safe: called once, at the top of a thread we own.
    if let Err(err) = unsafe { RoInitialize(RO_INIT_MULTITHREADED) } {
        let _ = events.send(WakeEvent::Failed(format!("WinRT: {err}")));
        return;
    }
    // A session that ends by itself is restarted, a few times at most, so
    // a mic that keeps failing doesn't spin forever.
    let mut restarts = 0;
    loop {
        let session = match winrt::Session::start(phrases, events, restart) {
            Ok(session) => session,
            Err(why) => {
                let _ = events.send(WakeEvent::Failed(why));
                return;
            }
        };
        eprintln!("wake word: listening");
        // `recv` waits for the next message.
        match control.recv() {
            Ok(Control::Restart(why)) => {
                drop(session);
                restarts += 1;
                if restarts > 5 {
                    let _ = events.send(WakeEvent::Failed(format!("kept stopping: {why}")));
                    return;
                }
                eprintln!("wake word: restarting ({why})");
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            // `Err` would mean every Sender is gone; stop then too.
            Ok(Control::Stop) | Err(_) => {
                drop(session);
                eprintln!("wake word: stopped listening");
                return;
            }
        }
    }
}

/// Not Windows: there is no built-in recognizer to use (yet).
#[cfg(not(windows))]
fn listen(
    _phrases: &[String],
    events: &Sender<WakeEvent>,
    _restart: &Sender<Control>,
    _control: &Receiver<Control>,
) {
    let _ = events.send(WakeEvent::Failed("only on Windows for now".into()));
}

/// The WinRT parts, kept together. Everything returns `Result<_, String>`
/// so the reason a step failed reaches the log in words.
#[cfg(windows)]
mod winrt {
    use std::sync::mpsc::Sender;

    use windows::Foundation::{TimeSpan, TypedEventHandler};
    use windows::Media::SpeechRecognition::{
        ISpeechRecognitionConstraint, SpeechContinuousRecognitionCompletedEventArgs,
        SpeechContinuousRecognitionResultGeneratedEventArgs, SpeechContinuousRecognitionSession,
        SpeechRecognitionConfidence, SpeechRecognitionListConstraint,
        SpeechRecognitionResultStatus, SpeechRecognizer,
    };
    use windows::core::{HSTRING, Interface};
    use windows_collections::IIterable;

    use super::{Control, WakeEvent};

    /// A running recognition session. Dropping it stops the session and
    /// releases the recognizer, which lets go of the microphone.
    pub struct Session {
        recognizer: SpeechRecognizer,
        session: SpeechContinuousRecognitionSession,
    }

    impl Session {
        pub fn start(
            phrases: &[String],
            events: &Sender<WakeEvent>,
            restart: &Sender<Control>,
        ) -> Result<Session, String> {
            // `new()` uses the Windows display language's speech pack. It
            // fails when no speech language is installed.
            let recognizer =
                SpeechRecognizer::new().map_err(|e| format!("no speech recognizer: {e}"))?;

            // The phrases as WinRT strings, in a WinRT list. `From<Vec>`
            // wraps a plain Vec in the COM interface the API wants.
            let words: Vec<HSTRING> = phrases.iter().map(HSTRING::from).collect();
            let list = SpeechRecognitionListConstraint::Create(&IIterable::<HSTRING>::from(words))
                .map_err(|e| format!("phrase list: {e}"))?;
            // `cast` asks a COM object for another of its interfaces; the
            // Constraints list holds the general constraint interface.
            let constraint: ISpeechRecognitionConstraint =
                list.cast().map_err(|e| format!("phrase list: {e}"))?;
            recognizer
                .Constraints()
                .and_then(|c| c.Append(&constraint))
                .map_err(|e| format!("phrase list: {e}"))?;

            // `join()` waits for the async operation on this thread. Fine
            // here: this thread does nothing else.
            let compiled = recognizer
                .CompileConstraintsAsync()
                .and_then(|op| op.join())
                .and_then(|r| r.Status())
                .map_err(|e| format!("compile: {e}"))?;
            if compiled != SpeechRecognitionResultStatus::Success {
                return Err(format!(
                    "the phrases did not compile ({}); is a speech language installed?",
                    status_name(compiled)
                ));
            }

            let session = recognizer
                .ContinuousRecognitionSession()
                .map_err(|e| format!("session: {e}"))?;
            // By default a session gives up after a few seconds of quiet.
            // We want it listening all day: one day, in 100 ns ticks.
            let _ = session.SetAutoStopSilenceTimeout(TimeSpan {
                Duration: 24 * 60 * 60 * 10_000_000,
            });

            // The handler runs on a WinRT worker thread whenever a phrase
            // is recognized. `move` gives the closure its own Sender; it
            // must be `Send` because it runs on another thread.
            let tx = events.clone();
            let heard = TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionResultGeneratedEventArgs,
            >::new(move |_, args| {
                // `args` may be null in theory; `.ok()?` turns that into an error.
                let result = args.ok()?.Result()?;
                let confidence = result.Confidence()?;
                if confidence == SpeechRecognitionConfidence::High
                    || confidence == SpeechRecognitionConfidence::Medium
                {
                    let text = result.Text()?.to_string_lossy();
                    let _ = tx.send(WakeEvent::Heard(text));
                }
                Ok(())
            });
            session
                .ResultGenerated(&heard)
                .map_err(|e| format!("listen: {e}"))?;

            // When the session ends by itself, ask the thread to restart it.
            // A stop we asked for ends with UserCanceled, which is ignored.
            let again = restart.clone();
            let ended = TypedEventHandler::<
                SpeechContinuousRecognitionSession,
                SpeechContinuousRecognitionCompletedEventArgs,
            >::new(move |_, args| {
                let status = args.ok()?.Status()?;
                if status != SpeechRecognitionResultStatus::UserCanceled {
                    let _ = again.send(Control::Restart(status_name(status).into()));
                }
                Ok(())
            });
            session
                .Completed(&ended)
                .map_err(|e| format!("listen: {e}"))?;

            // This is where a missing mic, or the microphone privacy
            // setting being off, shows up.
            session
                .StartAsync()
                .and_then(|op| op.join())
                .map_err(|e| format!("could not start listening: {e}"))?;
            Ok(Session {
                recognizer,
                session,
            })
        }
    }

    impl Drop for Session {
        fn drop(&mut self) {
            // Stop the session, then close the recognizer. Errors here only
            // mean it had already stopped.
            let _ = self.session.CancelAsync().and_then(|op| op.join());
            let _ = self.recognizer.Close();
        }
    }

    /// A readable name for a recognizer status, for the log.
    fn status_name(s: SpeechRecognitionResultStatus) -> &'static str {
        match s {
            SpeechRecognitionResultStatus::Success => "success",
            SpeechRecognitionResultStatus::TopicLanguageNotSupported => "language not supported",
            SpeechRecognitionResultStatus::GrammarLanguageMismatch => "language mismatch",
            SpeechRecognitionResultStatus::GrammarCompilationFailure => "compilation failure",
            SpeechRecognitionResultStatus::AudioQualityFailure => "poor audio",
            SpeechRecognitionResultStatus::UserCanceled => "canceled",
            SpeechRecognitionResultStatus::TimeoutExceeded => "timeout",
            SpeechRecognitionResultStatus::PauseLimitExceeded => "pause limit",
            SpeechRecognitionResultStatus::NetworkFailure => "network failure",
            SpeechRecognitionResultStatus::MicrophoneUnavailable => "microphone unavailable",
            _ => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalise_strips_case_and_punctuation() {
        assert_eq!(normalise("  Hey, OG   Blinki!  "), "hey og blinki");
        assert_eq!(normalise(""), "");
    }

    #[test]
    fn hears_every_way_of_calling() {
        for heard in [
            "Hey, OG Blinki",
            "hey og blinki",
            "Hey O.G. Blinki!",
            "hey o g blinky",
            "Hey Blinki",
            "hey blinky",
            "oh, hey blinki, there you are",
        ] {
            assert!(is_wake(heard, None), "{heard}");
        }
    }

    #[test]
    fn ignores_near_misses() {
        for heard in [
            "hey",
            "blinki",
            "hey there blinki",
            "they blinki",
            "hey og",
            "",
        ] {
            assert!(!is_wake(heard, None), "{heard}");
        }
    }

    #[test]
    fn answers_to_a_given_name() {
        assert!(is_wake("Hey, Ember!", Some("Ember")));
        assert!(!is_wake("Hey, Ember!", None));
        assert!(!is_wake("hey", Some("  ")), "a blank name is no name");
        assert!(
            is_wake("hey blinki", Some("Ember")),
            "the usual call still works"
        );
    }

    #[test]
    fn every_listed_phrase_is_a_wake() {
        for p in phrases(Some("Ember")) {
            assert!(is_wake(&p, Some("Ember")), "{p}");
        }
        assert_eq!(phrases(Some("Mr. Ember")).last().unwrap(), "hey mr ember");
        assert_eq!(phrases(Some("")).len(), phrases(None).len());
    }
}
