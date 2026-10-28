//! Remembrance: what the blinki remembers of your days together, after
//! Lifesworn's Remembrance. Good deeds (breaks, rest, coming back, smiling)
//! go into a ledger; its temperament is worked out from that ledger.
//! Everything stays on this computer.

/// How the blinki is, slowly, from what it remembers. All 0..1; 0.5 is
/// neutral for warmth, calm and vigor. `bond` starts at 0 and only grows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Temperament {
    /// Cool and wary (0) to warm and fond (1).
    pub warmth: f32,
    /// Restless and jumpy (0) to settled (1).
    pub calm: f32,
    /// Dim and sleepy (0) to bright and lively (1).
    pub vigor: f32,
    /// How long and how well you have known each other. Never shrinks.
    pub bond: f32,
}

impl Default for Temperament {
    fn default() -> Self {
        Temperament {
            warmth: 0.5,
            calm: 0.5,
            vigor: 0.5,
            bond: 0.0,
        }
    }
}
