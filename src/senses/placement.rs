//! Where the webcam sits: on which monitor, along which edge, and how far
//! along it. The eye needs this to aim at you, since it sees you from the
//! camera's spot but looks at you from its own.

use serde::{Deserialize, Serialize};

use crate::platform::desktop::{Point, Rect};

/// Which side of a monitor the camera is clipped to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// The saved spot. `offset` runs 0..1 along the edge: left to right on the
/// top and bottom, top to bottom on the sides.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CameraSpot {
    pub monitor: String,
    pub edge: Edge,
    pub offset: f64,
}

/// One monitor, in physical desktop pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Monitor {
    pub name: String,
    pub rect: Rect,
    pub primary: bool,
    /// Pixels per metre, from the size the monitor reports (or a 54 cm guess).
    pub px_per_m: f64,
}

/// Within this much of an edge's middle, the camera snaps to it.
const MAGNET: f64 = 0.04;

impl CameraSpot {
    /// The spot most webcams are in: the top middle of the main screen.
    pub fn default_for(monitors: &[Monitor]) -> Option<CameraSpot> {
        let main = monitors.iter().find(|m| m.primary).or(monitors.first())?;
        Some(CameraSpot {
            monitor: main.name.clone(),
            edge: Edge::Top,
            offset: 0.5,
        })
    }

    /// The point on the desktop, in physical pixels.
    pub fn point_on(&self, r: &Rect) -> Point {
        let t = self.offset.clamp(0.0, 1.0);
        match self.edge {
            Edge::Top => Point {
                x: r.x + r.width * t,
                y: r.y,
            },
            Edge::Bottom => Point {
                x: r.x + r.width * t,
                y: r.y + r.height,
            },
            Edge::Left => Point {
                x: r.x,
                y: r.y + r.height * t,
            },
            Edge::Right => Point {
                x: r.x + r.width,
                y: r.y + r.height * t,
            },
        }
    }

    /// Words for it: "top of DISPLAY1, middle".
    pub fn describe(&self) -> String {
        let edge = match self.edge {
            Edge::Top => "top",
            Edge::Bottom => "bottom",
            Edge::Left => "left side",
            Edge::Right => "right side",
        };
        let along = if (self.offset - 0.5).abs() < 1e-6 {
            "middle".to_string()
        } else {
            format!("{:.0}% along", self.offset * 100.0)
        };
        format!("{edge} of {}, {along}", short_name(&self.monitor))
    }
}

/// "\\.\DISPLAY1" reads better as "DISPLAY1".
pub fn short_name(name: &str) -> &str {
    name.trim_start_matches(r"\\.\")
}

/// Where the camera is on the desktop and that monitor's pixels per metre.
/// A saved spot on a monitor that's gone falls back to the default.
pub fn locate(spot: Option<&CameraSpot>, monitors: &[Monitor]) -> Option<(Point, f64)> {
    let find = |s: &CameraSpot| {
        monitors
            .iter()
            .find(|m| m.name == s.monitor)
            .map(|m| (s.point_on(&m.rect), m.px_per_m))
    };
    spot.and_then(find)
        .or_else(|| CameraSpot::default_for(monitors).as_ref().and_then(find))
}

/// The edge spot nearest a desktop point, for dragging the camera around.
/// Near an edge's middle it snaps there.
pub fn nearest_spot(p: Point, monitors: &[Monitor]) -> Option<CameraSpot> {
    let mut best: Option<(f64, CameraSpot)> = None;
    for m in monitors {
        let r = m.rect;
        for edge in [Edge::Top, Edge::Bottom, Edge::Left, Edge::Right] {
            let (along, across) = match edge {
                Edge::Top => ((p.x - r.x) / r.width, (p.y - r.y).abs()),
                Edge::Bottom => ((p.x - r.x) / r.width, (p.y - (r.y + r.height)).abs()),
                Edge::Left => ((p.y - r.y) / r.height, (p.x - r.x).abs()),
                Edge::Right => ((p.y - r.y) / r.height, (p.x - (r.x + r.width)).abs()),
            };
            let offset = along.clamp(0.0, 1.0);
            // Distance to the edge segment: across it, plus any overshoot past its ends.
            let len = if matches!(edge, Edge::Top | Edge::Bottom) {
                r.width
            } else {
                r.height
            };
            let past = (along - offset).abs() * len;
            let dist = across.hypot(past);
            let offset = if (offset - 0.5).abs() < MAGNET {
                0.5
            } else {
                offset
            };
            if best.as_ref().is_none_or(|(d, _)| dist < *d) {
                best = Some((
                    dist,
                    CameraSpot {
                        monitor: m.name.clone(),
                        edge,
                        offset,
                    },
                ));
            }
        }
    }
    best.map(|(_, spot)| spot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitors() -> Vec<Monitor> {
        vec![
            Monitor {
                name: r"\\.\DISPLAY1".into(),
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 1920.0,
                    height: 1080.0,
                },
                primary: true,
                px_per_m: 3529.0,
            },
            Monitor {
                name: r"\\.\DISPLAY2".into(),
                rect: Rect {
                    x: 1920.0,
                    y: -200.0,
                    width: 2560.0,
                    height: 1440.0,
                },
                primary: false,
                px_per_m: 4000.0,
            },
        ]
    }

    #[test]
    fn the_default_is_the_top_middle_of_the_main_screen() {
        let (p, ppm) = locate(None, &monitors()).unwrap();
        assert_eq!(p, Point { x: 960.0, y: 0.0 });
        assert_eq!(ppm, 3529.0);
    }

    #[test]
    fn each_edge_lands_where_it_should() {
        let r = Rect {
            x: 100.0,
            y: 50.0,
            width: 1000.0,
            height: 500.0,
        };
        let at = |edge, offset| {
            CameraSpot {
                monitor: String::new(),
                edge,
                offset,
            }
            .point_on(&r)
        };
        assert_eq!(at(Edge::Top, 0.25), Point { x: 350.0, y: 50.0 });
        assert_eq!(
            at(Edge::Bottom, 1.0),
            Point {
                x: 1100.0,
                y: 550.0
            }
        );
        assert_eq!(at(Edge::Left, 0.5), Point { x: 100.0, y: 300.0 });
        assert_eq!(at(Edge::Right, 0.0), Point { x: 1100.0, y: 50.0 });
    }

    #[test]
    fn dragging_finds_the_nearest_edge_and_snaps_to_its_middle() {
        let m = monitors();
        // Just above the second screen's top, near its middle: snaps to the middle.
        let s = nearest_spot(
            Point {
                x: 1920.0 + 1280.0 + 60.0,
                y: -230.0,
            },
            &m,
        )
        .unwrap();
        assert_eq!(
            (s.monitor.as_str(), s.edge, s.offset),
            (r"\\.\DISPLAY2", Edge::Top, 0.5)
        );
        // Near the main screen's bottom left corner, along the bottom.
        let s = nearest_spot(
            Point {
                x: 300.0,
                y: 1100.0,
            },
            &m,
        )
        .unwrap();
        assert_eq!((s.edge, (s.offset * 1000.0).round()), (Edge::Bottom, 156.0));
    }

    #[test]
    fn a_vanished_monitor_falls_back_to_the_default() {
        let gone = CameraSpot {
            monitor: r"\\.\DISPLAY9".into(),
            edge: Edge::Left,
            offset: 0.2,
        };
        assert_eq!(
            locate(Some(&gone), &monitors()).unwrap().0,
            Point { x: 960.0, y: 0.0 }
        );
    }

    #[test]
    fn spots_round_trip_through_json() {
        let s = CameraSpot {
            monitor: r"\\.\DISPLAY2".into(),
            edge: Edge::Right,
            offset: 0.3,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"right\""), "{json}");
        assert_eq!(serde_json::from_str::<CameraSpot>(&json).unwrap(), s);
    }

    #[test]
    fn no_monitors_means_no_spot() {
        assert_eq!(locate(None, &[]), None);
        assert_eq!(nearest_spot(Point { x: 0.0, y: 0.0 }, &[]), None);
    }

    #[test]
    fn an_offset_past_the_ends_stays_on_the_screen() {
        let r = monitors()[0].rect;
        let spot = |offset| CameraSpot {
            monitor: String::new(),
            edge: Edge::Top,
            offset,
        };
        assert_eq!(spot(1.7).point_on(&r), Point { x: 1920.0, y: 0.0 });
        assert_eq!(spot(-0.5).point_on(&r), Point { x: 0.0, y: 0.0 });
    }

    #[test]
    fn describes_itself_in_words() {
        let mut s = CameraSpot {
            monitor: r"\\.\DISPLAY1".into(),
            edge: Edge::Top,
            offset: 0.5,
        };
        assert_eq!(s.describe(), "top of DISPLAY1, middle");
        s.edge = Edge::Right;
        s.offset = 0.25;
        assert_eq!(s.describe(), "right side of DISPLAY1, 25% along");
    }
}
