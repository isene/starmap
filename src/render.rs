//! Drawing a sky into a rectangle of character cells.

use crate::canvas::Canvas;
use crate::proj::{Projection, View};
use crate::{figures, star_rgb, stars};
use crust::style;
use crust::Cursor;

/// Something to draw that is not a catalogue star: a planet, the moon,
/// a comet, a telescope's current target.
pub struct Body {
    pub name: String,
    pub ra: f64,
    pub dec: f64,
    pub rgb: (u8, u8, u8),
    /// Dot radius. 0 for a single dot, 2 for the sun and moon.
    pub size: i32,
}

/// What the chart shows.
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// Constellation stick figures.
    pub figures: bool,
    /// Proper names beside the bright stars.
    pub names: bool,
    /// Faintest magnitude the caller wants, before the chart scales it
    /// down to what the room can actually show.
    pub mag: f64,
    /// The horizon (or the celestial equator) as a circle.
    pub rim: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self { figures: true, names: true, mag: 6.5, rim: true }
    }
}

impl Opts {
    /// Start at the faintest star the sky you are under actually shows.
    /// Bortle 1 is a desert sky at about magnitude 7.8, Bortle 9 an inner
    /// city where four is a good night. The catalogue stops at 6.5, which
    /// is roughly the naked-eye limit anyway.
    pub fn for_bortle(bortle: f64) -> Self {
        let mag = (7.8 - 0.475 * (bortle.clamp(1.0, 9.0) - 1.0)).clamp(3.5, 6.5);
        Self { mag, ..Self::default() }
    }
}

/// A drawn chart: the printable frame, plus where each star landed so a
/// caller can hit-test against it.
pub(crate) struct Plot {
    pub frame: String,
    /// `(index into stars(), dot x, dot y)` for every star drawn.
    pub placed: Vec<(usize, i32, i32)>,
    /// The magnitude limit this size and zoom could actually take.
    pub mag_shown: f64,
}

/// Draw the sky into the rectangle at (`x`, `y`), `w`×`h` cells.
pub fn panel(
    view: &View,
    opts: &Opts,
    bodies: &[Body],
    x: u16,
    y: u16,
    w: u16,
    h: u16,
) -> String {
    plot(view, opts, bodies, x, y, w, h).frame
}

pub(crate) fn plot(
    view: &View,
    opts: &Opts,
    bodies: &[Body],
    x: u16,
    y: u16,
    w: u16,
    h: u16,
) -> Plot {
    let (cw, ch) = (w.max(10) as usize, h.max(3) as usize);
    let mut canvas = Canvas::new(cw, ch);
    let (dw, dh) = canvas.dots();
    let (cx, cy) = (dw / 2.0, dh / 2.0);
    // Unit radius 1 is the rim of the sky at zoom 1.
    let r = (dw.min(dh) / 2.0) - 2.0;

    // How faint this much room can take. Star counts run about half a
    // magnitude per factor of ten (15 stars brighter than 1st, 4,800
    // brighter than 6th), so invert that for a tenth of the dots filled.
    // Zooming shows less sky, so the same dots go further down the list.
    let dots = std::f64::consts::PI * r * r;
    let fits = ((2.0 * 0.10 * dots * view.zoom * view.zoom).log10() - 0.68) / 0.5;
    let mag_shown = opts.mag.min(fits).max(1.0);

    let to_dot = |u: (f64, f64)| ((cx + u.0 * r) as i32, (cy + u.1 * r) as i32);
    let in_frame = |p: (i32, i32)| {
        p.0 >= 0 && p.1 >= 0 && (p.0 as f64) < dw && (p.1 as f64) < dh
    };

    // The rim: the horizon you stand under, or the celestial equator.
    if opts.rim {
        let rim = (70, 70, 85);
        let rr = r * view.zoom;
        let (ox, oy) = (cx - view.pan.0 * r * view.zoom, cy - view.pan.1 * r * view.zoom);
        if rr < dw * 4.0 {
            canvas.circle(ox, oy, rr, rim, -1.0);
        }
    }

    // Stick figures under everything else.
    if opts.figures {
        let ink = if r < 40.0 { (46, 60, 88) } else { (60, 78, 110) };
        for fig in figures() {
            for pair in fig.pts.windows(2) {
                let (Some(a), Some(b)) = (
                    view.screen(pair[0].0, pair[0].1),
                    view.screen(pair[1].0, pair[1].1),
                ) else {
                    continue;
                };
                let (p, q) = (to_dot(a), to_dot(b));
                // A stroke with both ends far outside would waste a long
                // Bresenham run; one end in view is enough to bother.
                if !in_frame(p) && !in_frame(q) {
                    continue;
                }
                canvas.line(p.0, p.1, q.0, q.1, ink, -0.5);
            }
        }
    }

    // Stars. The catalogue is brightest first, so the loop stops as soon
    // as it passes the limit.
    let mut placed = Vec::new();
    let mut star_labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    for (i, s) in stars().iter().enumerate() {
        if s.mag > mag_shown {
            break;
        }
        let Some(u) = view.screen(s.ra, s.dec) else { continue };
        let p = to_dot(u);
        if !in_frame(p) {
            continue;
        }
        let rgb = star_rgb(s);
        if s.mag < 1.0 {
            canvas.disc(p.0, p.1, 1, rgb, 10.0 - s.mag);
        } else {
            canvas.set(p.0, p.1, rgb, 10.0 - s.mag);
        }
        placed.push((i, p.0, p.1));
        // Names need room, and the fainter the star the more zoomed in
        // you have to be before it earns a label.
        let earns = s.mag < 1.8 + (view.zoom.log2() * 1.2).max(0.0);
        if opts.names && !s.name.is_empty() && earns && w >= 40 && h >= 10 {
            let (col, row) = (x + p.0 as u16 / 2 + 2, y + p.1 as u16 / 4);
            if col + s.name.len() as u16 + 1 < x + w {
                star_labels.push((col, row, s.name.to_string(), (150, 150, 165)));
            }
        }
    }

    // Bodies on top of the stars, each labelled.
    let mut body_labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    for b in bodies {
        let Some(u) = view.screen(b.ra, b.dec) else { continue };
        let p = to_dot(u);
        if !in_frame(p) {
            continue;
        }
        canvas.disc(p.0, p.1, b.size.max(1), b.rgb, 100.0);
        let (col, row) = (x + p.0 as u16 / 2 + 2, y + p.1 as u16 / 4);
        if col + b.name.len() as u16 + 1 < x + w {
            body_labels.push((col, row, b.name.clone(), b.rgb));
        }
    }

    // Place labels, bodies first: two names on one patch of sky print
    // over each other, and "jupiterollux" is worse than no Pollux.
    let mut taken: Vec<(u16, u16, u16)> = Vec::new();
    let mut labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    for (col, row, text, rgb) in body_labels.into_iter().chain(star_labels) {
        let end = col + text.chars().count() as u16;
        if taken.iter().any(|&(r, c0, c1)| r == row && col <= c1 && c0 <= end) {
            continue;
        }
        taken.push((row, col, end));
        labels.push((col, row, text, rgb));
    }

    let mut frame = canvas.frame(x, y);

    // Cardinal points, on the horizon where they mean something.
    if let Projection::Horizon { .. } = view.proj {
        for (label, az) in [("N", 0.0f64), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
            let a = az.to_radians();
            let u = (
                (-a.sin() - view.pan.0) * view.zoom,
                (-a.cos() - view.pan.1) * view.zoom,
            );
            let p = to_dot(u);
            if !in_frame(p) {
                continue;
            }
            frame.push_str(&Cursor::at(x + p.0 as u16 / 2, y + p.1 as u16 / 4));
            frame.push_str(&style::rgb(label, Some((255, 190, 90)), None, "b"));
        }
    }

    for (col, row, text, rgb) in labels {
        if row >= y + h {
            continue;
        }
        frame.push_str(&Cursor::at(col, row));
        frame.push_str(&style::rgb(&text, Some(rgb), None, ""));
    }

    Plot { frame, placed, mag_shown }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proj::Projection;

    #[test]
    fn draws_something_and_places_stars() {
        let v = View::new(Projection::Hemisphere { north: true });
        let p = plot(&v, &Opts::default(), &[], 1, 1, 100, 30);
        assert!(!p.placed.is_empty(), "no stars placed");
        assert!(p.frame.contains('\u{2800}') || p.frame.chars().any(|c| c >= '\u{2801}' && c <= '\u{28ff}'));
        // Polaris is on the northern map, Canopus is not.
        let names: Vec<&str> = p.placed.iter().map(|&(i, _, _)| stars()[i].name).collect();
        assert!(names.contains(&"Polaris"), "no Polaris on the north map");
        assert!(!names.contains(&"Canopus"), "Canopus should be off the north map");
    }

    #[test]
    fn zoom_shows_fainter_stars() {
        let mut v = View::new(Projection::Hemisphere { north: true });
        let wide = plot(&v, &Opts::default(), &[], 1, 1, 100, 30).mag_shown;
        v.zoom = 8.0;
        let close = plot(&v, &Opts::default(), &[], 1, 1, 100, 30).mag_shown;
        assert!(close > wide, "zoomed in should reach fainter: {wide} -> {close}");
    }

    #[test]
    fn one_frame_is_quick() {
        let v = View::new(Projection::Hemisphere { north: true });
        let o = Opts::default();
        let t = std::time::Instant::now();
        for _ in 0..20 {
            let _ = plot(&v, &o, &[], 1, 1, 150, 42);
        }
        let per = t.elapsed() / 20;
        assert!(per.as_millis() < 20, "one frame took {per:?}");
    }
}
