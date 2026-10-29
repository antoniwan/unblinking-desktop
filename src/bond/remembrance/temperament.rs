//! Temperament: how the blinki is, slowly, worked out fresh from the
//! ledger every time it's asked for, and the traits it has noticed in you.

use super::ledger::{Deed, Ledger};

/// Bump this whenever `temperament_of` (or `day_brightness_of`) changes
/// meaning. The saved file notes the version it was written under, but the
/// temperament itself is never saved: it's always recomputed from the
/// ledger with the formula in this build. So an old file simply gets the
/// new formula; the number is there so a person reading the JSON (or a
/// future migration) can tell which rules shaped it.
pub const FORMULA_VERSION: u32 = 1;

/// How much lifetime history it takes for the bond to reach about 63%
/// (1 - 1/e). A "point" is roughly an hour together or a good break; a
/// couple of months of ordinary days gets there, and it keeps creeping up.
const BOND_SCALE: f64 = 250.0;

/// How many lifetime points before the blinki trusts its own memories
/// fully. Until then its moods stay near neutral (0.5), so a brand new
/// blinki isn't judged by an empty ledger.
const FAMILIAR_SCALE: f64 = 15.0;

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

/// A gentle 0..1 curve: 0 at 0, a half at `k`, approaching 1 forever.
/// `x / (x + k)` never overshoots, so no amount of deeds "maxes out".
fn sat(x: f64, k: f64) -> f64 {
    let x = x.max(0.0);
    x / (x + k)
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// The temperament formula, version `FORMULA_VERSION`. Pure: the same
/// ledger at the same `now` always gives the same answer.
///
/// - Each mood has a *target* from the recent sums, then is pulled from
///   neutral (0.5) toward it by how *familiar* the blinki is with you
///   (lifetime history). An empty ledger gives exactly the default.
/// - warmth: smiles, returns, time together.
/// - calm: breaks and rest; late nights pull it down a little.
/// - vigor: rest, breaks, returns, smiles, time together. Neglect lets it
///   sink toward sleepy, never to nothing.
/// - bond: lifetime points only, so it can never go down.
pub fn temperament_of(ledger: &Ledger, now: u64) -> Temperament {
    let r = |d| ledger.recent(d, now);
    let familiar = sat(ledger.lifetime_points(), FAMILIAR_SCALE);

    let warmth_target = 0.3
        + 0.7
            * sat(
                r(Deed::Smile)
                    + 0.5 * r(Deed::Return)
                    + 0.25 * r(Deed::TogetherHour)
                    + r(Deed::LongReturn),
                5.0,
            );
    let calm_target = 0.35 + 0.6 * sat(r(Deed::Break) + 2.0 * r(Deed::Rest), 4.0)
        - 0.25 * sat(r(Deed::LateNight), 3.0);
    let vigor_target = 0.25 + 0.75 * sat(ledger.recent_care(now), 6.0);
    let bond = 1.0 - (-ledger.lifetime_points() / BOND_SCALE).exp();

    // `as f32`: the formula works in f64 for precise decay; the struct the
    // rest of the app shares is f32, plenty for colours and speeds.
    let mood = |target: f64| lerp(0.5, target.clamp(0.0, 1.0), familiar) as f32;
    Temperament {
        warmth: mood(warmth_target),
        calm: mood(calm_target),
        vigor: mood(vigor_target),
        bond: bond.clamp(0.0, 1.0) as f32,
    }
}

/// How bright today is, 0..1: 1 when it's been well looked after (or is
/// brand new), sinking toward a floor of 0.45 when it's been left alone.
/// Never dark: a neglected blinki has dim, quiet days, nothing worse.
pub fn day_brightness_of(ledger: &Ledger, now: u64) -> f32 {
    let familiar = sat(ledger.lifetime_points(), FAMILIAR_SCALE);
    let cared = 0.45 + 0.55 * sat(ledger.recent_care(now), 3.0);
    lerp(1.0, cared, familiar) as f32
}

/// Something the blinki has noticed about you over time, from patterns in
/// the ledger. A trait can come and go as habits change, slowly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trait {
    NightOwl,
    EarlyRiser,
    TakesGoodBreaks,
    WellRested,
    Smiley,
    AlwaysComesBack,
}

impl Trait {
    /// Plain words, for a tray tooltip or a whisper.
    pub fn name(self) -> &'static str {
        match self {
            Trait::NightOwl => "a night owl",
            Trait::EarlyRiser => "an early riser",
            Trait::TakesGoodBreaks => "takes good breaks",
            Trait::WellRested => "well rested",
            Trait::Smiley => "smiles a lot",
            Trait::AlwaysComesBack => "always comes back",
        }
    }
}

/// The traits that show in a ledger. Lifetime counts with ratios, so a
/// habit has to be real (several times over) and has to outweigh its
/// opposite: start sleeping early and "night owl" fades as early mornings
/// pile up.
pub fn traits_of(ledger: &Ledger) -> Vec<Trait> {
    let c = |d| ledger.count(d);
    let mut traits = Vec::new();
    if c(Deed::LateNight) >= 5.0 && c(Deed::LateNight) >= 2.0 * c(Deed::EarlyMorning) {
        traits.push(Trait::NightOwl);
    }
    if c(Deed::EarlyMorning) >= 5.0 && c(Deed::EarlyMorning) >= 2.0 * c(Deed::LateNight) {
        traits.push(Trait::EarlyRiser);
    }
    // About one break for every three hours together, or better.
    if c(Deed::Break) >= 10.0 && c(Deed::Break) * 3.0 >= c(Deed::TogetherHour) {
        traits.push(Trait::TakesGoodBreaks);
    }
    if c(Deed::Rest) >= 7.0 && c(Deed::LateNight) * 4.0 <= c(Deed::Rest) {
        traits.push(Trait::WellRested);
    }
    if c(Deed::Smile) >= 20.0 {
        traits.push(Trait::Smiley);
    }
    if c(Deed::Return) >= 30.0 || c(Deed::LongReturn) >= 3.0 {
        traits.push(Trait::AlwaysComesBack);
    }
    traits
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 3600;
    /// Some Tuesday in 2026, so times look like real dates.
    const T0: u64 = 1_790_000_000;

    #[test]
    fn an_empty_ledger_is_the_default_temperament() {
        assert_eq!(
            temperament_of(&Ledger::default(), T0),
            Temperament::default()
        );
        assert_eq!(day_brightness_of(&Ledger::default(), T0), 1.0);
    }

    #[test]
    fn bond_never_shrinks() {
        let mut ledger = Ledger::default();
        let mut now = T0;
        let mut last = 0.0;
        fastrand::seed(7);
        for _ in 0..500 {
            // Random deeds at random gaps, some of them weeks long.
            now += fastrand::u64(0..(10 * DAY));
            ledger.add(Deed::ALL[fastrand::usize(0..Deed::COUNT)], now);
            let bond = temperament_of(&ledger, now).bond;
            assert!(bond >= last, "bond fell from {last} to {bond}");
            last = bond;
        }
        assert!(last > 0.5 && last < 1.0);
    }

    #[test]
    fn care_warms_and_neglect_dims_but_never_darkens() {
        let mut ledger = Ledger::default();
        // Two good weeks: rest each night, breaks, smiles, hours together.
        for day in 0..14 {
            let t = T0 + day * DAY;
            ledger.add(Deed::Rest, t);
            for h in 0..6 {
                ledger.add(Deed::TogetherHour, t + h * 3600);
            }
            ledger.add(Deed::Break, t + 3 * 3600);
            ledger.add(Deed::Smile, t + 4 * 3600);
        }
        let good = T0 + 14 * DAY;
        let cared = temperament_of(&ledger, good);
        assert!(
            cared.warmth > 0.6 && cared.calm > 0.6 && cared.vigor > 0.6,
            "{cared:?}"
        );
        assert!(day_brightness_of(&ledger, good) > 0.85);

        // Then a month alone.
        let later = good + 30 * DAY;
        let alone = temperament_of(&ledger, later);
        assert!(alone.vigor < cared.vigor && alone.warmth < cared.warmth);
        assert_eq!(alone.bond, cared.bond, "time apart doesn't cost the bond");
        let dim = day_brightness_of(&ledger, later);
        assert!((0.45..0.6).contains(&dim), "dim, not dark: {dim}");
    }

    #[test]
    fn late_nights_cost_a_little_calm() {
        let mut a = Ledger::default();
        let mut b = Ledger::default();
        for day in 0..5 {
            let t = T0 + day * DAY;
            a.add(Deed::Break, t);
            b.add(Deed::Break, t);
            b.add(Deed::LateNight, t + 60);
        }
        let now = T0 + 5 * DAY;
        let (ca, cb) = (temperament_of(&a, now).calm, temperament_of(&b, now).calm);
        assert!(cb < ca, "late nights lower calm: {cb} vs {ca}");
        assert!(cb > 0.4, "gently: {cb}");
    }

    #[test]
    fn traits_follow_patterns() {
        let mut ledger = Ledger::default();
        assert!(traits_of(&ledger).is_empty());
        for day in 0..6 {
            ledger.add(Deed::LateNight, T0 + day * DAY);
        }
        assert_eq!(traits_of(&ledger), vec![Trait::NightOwl]);
        // Habits change: enough early mornings and the owl is gone.
        for day in 6..20 {
            ledger.add(Deed::EarlyMorning, T0 + day * DAY);
        }
        assert_eq!(traits_of(&ledger), vec![Trait::EarlyRiser]);
    }
}
