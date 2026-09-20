//! Drawing a sky into a rectangle of character cells: braille, or a
//! picture for glow with the names as text showing through holes.

use crate::canvas::Canvas;
use crate::proj::{Projection, View};
use crate::{figures, star_rgb, star_teff, stars, teff_rgb};
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

/// A chart drawn in real pixels: the text to print first (only the
/// names and cardinal points; the picture has holes where they sit), the
/// picture for `Display::show_canvas`, where each star landed in pixels,
/// and the magnitude limit that was shown.
pub struct Picture {
    pub text: String,
    pub canvas: glow::Canvas,
    /// `(index into stars(), pixel x, pixel y)` for every star drawn.
    pub placed: Vec<(usize, i32, i32)>,
    pub mag_shown: f64,
}

/// Draw the sky as a picture for the rectangle at (`x`, `y`), `w`×`h`
/// cells at the terminal's cell size. Print the text, then show the canvas
/// at (`x`, `y`).
pub fn panel_pixels(view: &View, opts: &Opts, bodies: &[Body], x: u16, y: u16, w: u16, h: u16) -> Picture {
    picture(view, opts, bodies, x, y, w, h, None)
}

/// The same for a given cell size.
pub fn picture(view: &View, opts: &Opts, bodies: &[Body], x: u16, y: u16, w: u16, h: u16, cell: Option<(u16, u16)>) -> Picture {
    let (cw, ch) = (w.max(10), h.max(3));
    let mut c = glow::Canvas::sized(cw, ch, cell);
    let (cell_w, cell_h) = (c.cell_w(), c.cell_h());
    // Pixels per braille dot: the unit the braille chart's sizes are in.
    let dot = (cell_w / 2.0 + cell_h / 4.0) / 2.0;
    let (pw, ph) = (c.w as f64, c.h as f64);
    let (cx, cy) = (pw / 2.0, ph / 2.0);
    // The horizon keeps a tenth of the height clear around it, for the
    // cardinal points and the names at the edge. A hemisphere map shows
    // the sky a quarter past its equator too, out to a clean circle, so
    // its rim sits further in.
    let map = matches!(view.proj, Projection::Hemisphere { .. });
    let r = pw.min(ph) * if map { 0.35 } else { 0.45 } - 2.0 * dot;
    let (ox, oy) = (cx - view.pan.0 * r * view.zoom, cy - view.pan.1 * r * view.zoom);
    let clip = if map { 1.25 * r * view.zoom } else { f64::INFINITY };

    // How faint this much room can take, as for braille, with pixels
    // letting about twice as many stars in as dots do. A small block
    // shows the constellation stars; a full screen gets the whole sky.
    let dots = std::f64::consts::PI * r * r / (dot * dot) * 2.0;
    let fits = ((2.0 * 0.10 * dots * view.zoom * view.zoom).log10() - 0.68) / 0.5;
    let mag_shown = opts.mag.min(fits).max(1.0);

    // Star sizes follow the chart's radius: a small block gets small
    // stars, a full screen fuller ones.
    let unit = dot * (r / (60.0 * dot)).clamp(0.5, 1.2);
    let to_px = |u: (f64, f64)| (cx + u.0 * r, cy + u.1 * r);
    let in_frame = |p: (f64, f64)| {
        p.0 >= -dot && p.1 >= -dot && p.0 < pw + dot && p.1 < ph + dot
            && ((p.0 - ox).powi(2) + (p.1 - oy).powi(2)).sqrt() <= clip
    };

    if opts.rim {
        let rr = r * view.zoom;
        if rr < pw * 4.0 {
            ring(&mut c, ox, oy, rr, dot * 0.4, (70, 70, 85));
        }
    }

    if opts.figures {
        let ink = if r < 40.0 * dot { (46, 60, 88) } else { (60, 78, 110) };
        for fig in figures() {
            for pair in fig.pts.windows(2) {
                let (Some(a), Some(b)) = (view.screen(pair[0].0, pair[0].1), view.screen(pair[1].0, pair[1].1)) else { continue };
                let (p, q) = (to_px(a), to_px(b));
                if !in_frame(p) && !in_frame(q) {
                    continue;
                }
                c.line(p, q, dot * 0.35, ink, 1.0);
            }
        }
    }

    // Stars, faintest first so the bright ones paint over. A star's
    // size follows its magnitude, its colour its temperature, and the
    // faintest fade a little so the eye sorts them the way the sky does.
    let mut shown: Vec<(usize, (f64, f64))> = Vec::new();
    for (i, s) in stars().iter().enumerate() {
        if s.mag > mag_shown {
            break;
        }
        let Some(u) = view.screen(s.ra, s.dec) else { continue };
        let p = to_px(u);
        if in_frame(p) {
            shown.push((i, p));
        }
    }
    let mut placed = Vec::with_capacity(shown.len());
    let mut star_labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    for &(i, p) in shown.iter().rev() {
        let s = &stars()[i];
        let rgb = teff_rgb(star_teff(s));
        let fade = (1.0 - 0.10 * (s.mag - mag_shown + 3.0).max(0.0)).clamp(0.5, 1.0);
        let rgb = ((rgb.0 as f64 * fade) as u8, (rgb.1 as f64 * fade) as u8, (rgb.2 as f64 * fade) as u8);
        let radius = (unit * (0.20 + 0.12 * (mag_shown - s.mag))).min(unit * 1.6);
        blob(&mut c, p, radius, rgb, s.mag < 1.0);
        placed.push((i, p.0 as i32, p.1 as i32));
        let earns = s.mag < 1.8 + (view.zoom.log2() * 1.2).max(0.0);
        if opts.names && !s.name.is_empty() && earns && w >= 40 && h >= 10 {
            let (col, row) = (x + (p.0 / cell_w) as u16 + 2, y + (p.1 / cell_h) as u16);
            if col + s.name.len() as u16 + 1 < x + w {
                star_labels.push((col, row, s.name.to_string(), (150, 150, 165)));
            }
        }
    }

    let mut body_labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    for b in bodies {
        let Some(u) = view.screen(b.ra, b.dec) else { continue };
        let p = to_px(u);
        if !in_frame(p) {
            continue;
        }
        blob(&mut c, p, unit * (0.5 + 0.4 * b.size.max(0) as f64), b.rgb, b.size >= 2);
        let (col, row) = (x + (p.0 / cell_w) as u16 + 2, y + (p.1 / cell_h) as u16);
        if col + b.name.len() as u16 + 1 < x + w {
            body_labels.push((col, row, b.name.clone(), b.rgb));
        }
    }

    let mut taken: Vec<(u16, u16, u16)> = Vec::new();
    let mut text = String::new();
    if let Projection::Horizon { .. } = view.proj {
        for (label, az) in [("N", 0.0f64), ("E", 90.0), ("S", 180.0), ("W", 270.0)] {
            let a = az.to_radians();
            let p = to_px(((-a.sin() - view.pan.0) * view.zoom, (-a.cos() - view.pan.1) * view.zoom));
            if !in_frame(p) {
                continue;
            }
            let (col, row) = (x + (p.0 / cell_w) as u16, y + (p.1 / cell_h) as u16);
            if col >= x + w || row >= y + h {
                continue;
            }
            taken.push((row, col, col + 1));
            c.hole((row - y) as usize, (col - x) as usize, 1);
            text.push_str(&Cursor::at(col, row));
            text.push_str(&style::rgb(label, Some((255, 190, 90)), Some((0, 0, 0)), "b"));
        }
    }
    for (col, row, label, rgb) in body_labels.into_iter().chain(star_labels) {
        let n = label.chars().count() as u16;
        let end = col + n;
        if row >= y + h || taken.iter().any(|&(r, c0, c1)| r == row && col <= c1 && c0 <= end) {
            continue;
        }
        taken.push((row, col, end));
        c.hole((row - y) as usize, (col - x) as usize, n as usize);
        // On black, whatever the pane behind the picture is painted.
        text.push_str(&Cursor::at(col, row));
        text.push_str(&style::rgb(&label, Some(rgb), Some((0, 0, 0)), ""));
    }

    Picture { text, canvas: c, placed, mag_shown }
}

/// Brighten a pixel to at least `rgb` scaled by `k`: things that overlap
/// add up to the brighter of them, never to darker.
fn lift(c: &mut glow::Canvas, x: i64, y: i64, rgb: (u8, u8, u8), k: f64) {
    if x < 0 || y < 0 || x as usize >= c.w || y as usize >= c.h {
        return;
    }
    let o = (y as usize * c.w + x as usize) * 4;
    let want = [rgb.0, rgb.1, rgb.2].map(|v| (v as f64 * k).round().clamp(0.0, 255.0) as u8);
    for (i, w) in want.iter().enumerate() {
        if c.rgba[o + i] < *w {
            c.rgba[o + i] = *w;
        }
    }
    c.rgba[o + 3] = 255;
}

/// A soft-edged disc, with a faint glow around it when `bright`.
fn blob(c: &mut glow::Canvas, p: (f64, f64), r: f64, rgb: (u8, u8, u8), bright: bool) {
    let reach = if bright { r * 2.4 } else { r + 1.0 };
    let (x0, x1) = ((p.0 - reach).floor() as i64, (p.0 + reach).ceil() as i64);
    let (y0, y1) = ((p.1 - reach).floor() as i64, (p.1 + reach).ceil() as i64);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = ((x as f64 + 0.5 - p.0).powi(2) + (y as f64 + 0.5 - p.1).powi(2)).sqrt();
            let k = if d <= r + 0.5 {
                (r + 0.5 - d).clamp(0.0, 1.0)
            } else if bright {
                (0.22 * (1.0 - (d - r) / (1.4 * r))).max(0.0)
            } else {
                0.0
            };
            if k > 0.0 {
                lift(c, x, y, rgb, k);
            }
        }
    }
}

/// A circle of radius `r` about (`cx`, `cy`), `thick` pixels wide.
fn ring(c: &mut glow::Canvas, cx: f64, cy: f64, r: f64, thick: f64, rgb: (u8, u8, u8)) {
    let steps = ((r * 3.0) as usize).clamp(360, 12000);
    let rr = (thick / 2.0).max(0.5);
    for i in 0..steps {
        let a = i as f64 * std::f64::consts::TAU / steps as f64;
        let p = (cx + r * a.sin(), cy + r * a.cos());
        if p.0 < -rr || p.1 < -rr || p.0 > c.w as f64 + rr || p.1 > c.h as f64 + rr {
            continue;
        }
        if rr <= 0.6 { lift(c, p.0 as i64, p.1 as i64, rgb, 1.0) } else { blob(c, p, rr, rgb, false) }
    }
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
    fn the_picture_places_stars_and_cuts_holes_for_their_names() {
        let v = View::new(Projection::Hemisphere { north: true });
        let p = picture(&v, &Opts::default(), &[], 1, 2, 100, 30, Some((10, 20)));
        assert_eq!((p.canvas.w, p.canvas.h), (1000, 600));
        assert!(p.placed.len() > 500, "only {} stars", p.placed.len());
        assert!(p.text.contains("Vega") && p.text.contains("Arcturus"), "the bright stars are named: {}", p.text.replace('\x1b', "^"));
        let holes = p.canvas.rgba.chunks(4).filter(|px| px[3] == 0).count();
        assert!(holes > 0 && holes % 200 == 0, "{holes} transparent pixels, whole cells of 200");
        let lit = p.canvas.rgba.chunks(4).filter(|px| px[3] == 255 && px[0].max(px[1]).max(px[2]) > 40).count();
        assert!(lit > 2000, "only {lit} lit pixels");
        // Polaris sits near the middle, and its pixel is white-ish and bright.
        let polaris = p.placed.iter().find(|&&(i, _, _)| stars()[i].name == "Polaris").map(|&(_, x, y)| (x, y)).unwrap();
        let o = (polaris.1 as usize * p.canvas.w + polaris.0 as usize) * 4;
        assert!(p.canvas.rgba[o] > 150 && p.canvas.rgba[o + 2] > 150, "Polaris pixel {:?}", &p.canvas.rgba[o..o + 3]);
    }

    #[test]
    fn one_picture_is_quick_enough() {
        let v = View::new(Projection::Hemisphere { north: true });
        let o = Opts::default();
        let t = std::time::Instant::now();
        for _ in 0..3 {
            let _ = picture(&v, &o, &[], 1, 2, 190, 50, Some((10, 20)));
        }
        let per = t.elapsed() / 3;
        assert!(per.as_millis() < 400, "one picture took {per:?}");
        eprintln!("one picture: {per:?}");
        // STARMAP_DUMP=/some/file.png writes the picture out for a look.
        if let Ok(path) = std::env::var("STARMAP_DUMP") {
            let v = View::new(Projection::Horizon { lst_deg: 40.0, lat_deg: 59.9 });
            let _ = std::fs::write(path, picture(&v, &o, &[], 1, 2, 190, 50, Some((10, 20))).canvas.png());
        }
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
