//! Drawing a sky into a rectangle of character cells: braille, or a
//! picture for glow with the names as text showing through holes.

use crate::canvas::Canvas;
use crate::proj::{Projection, View};
use crate::{dsos, figures, star_rgb, star_teff, stars, teff_rgb, Dso, DsoKind};
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
    /// The Messier and Caldwell objects, drawn at their size.
    pub dso: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Self { figures: true, names: true, mag: 6.5, rim: true, dso: false }
    }
}

/// A deep-sky mark's colour, by kind.
fn dso_rgb(k: DsoKind) -> (u8, u8, u8) {
    match k {
        DsoKind::Galaxy => (225, 150, 230),
        DsoKind::OpenCluster => (235, 215, 120),
        DsoKind::Globular => (245, 170, 95),
        DsoKind::Planetary => (110, 225, 200),
        DsoKind::Nebula | DsoKind::ClusterNebula => (120, 205, 140),
        DsoKind::Supernova => (235, 110, 110),
        DsoKind::Dark => (125, 125, 140),
        DsoKind::StarGroup => (190, 190, 205),
    }
}

/// An object's outline in unit-disc screen coordinates, `steps` points
/// round it, with its centre; `None` when the centre is not in this sky.
/// The ellipse is laid out on the sky first, its long axis turned to the
/// object's tilt, so the chart's own bending applies to it as to the stars.
fn dso_outline(view: &View, d: &Dso, steps: usize) -> Option<((f64, f64), Vec<Option<(f64, f64)>>)> {
    let centre = view.screen(d.ra, d.dec)?;
    Some((centre, sky_ellipse(view, d.ra, d.dec, d.major / 120.0, d.minor / 120.0, d.pa, steps)))
}

/// An ellipse on the sky about (`ra`, `dec`), semi-axes `a` and `b` in
/// degrees, long axis tilted `pa` degrees from north through east, as
/// `steps` + 1 unit-disc screen points; `None` for any point out of view.
fn sky_ellipse(view: &View, ra: f64, dec: f64, a: f64, b: f64, pa: f64, steps: usize) -> Vec<Option<(f64, f64)>> {
    let pa = pa.to_radians();
    let cos_dec = dec.to_radians().cos().max(0.05);
    (0..=steps)
        .map(|i| {
            let t = i as f64 * std::f64::consts::TAU / steps as f64;
            let (u, v) = (a * t.cos(), b * t.sin());
            let east = u * pa.sin() + v * pa.cos();
            let north = u * pa.cos() - v * pa.sin();
            view.screen(ra + east / cos_dec, dec + north)
        })
        .collect()
}

/// Something drawn over the chart at a place on the sky: a circle of
/// `radius_deg` (an eyepiece's field, say), or with radius 0 a crosshair.
#[derive(Clone, Debug)]
pub struct Mark {
    pub ra: f64,
    pub dec: f64,
    pub radius_deg: f64,
    pub rgb: (u8, u8, u8),
}

/// What to write beside an object, if it earns a label at this zoom: the
/// brightest few on the whole sky, more as you close in, and the common
/// name once you are close.
fn dso_label(d: &Dso, zoom: f64) -> Option<String> {
    let mag = d.mag.unwrap_or(9.0);
    if mag > 4.5 + zoom.log2().max(0.0) * 2.5 {
        return None;
    }
    Some(if zoom >= 4.0 && !d.name.is_empty() { d.label() } else { d.id.to_string() })
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
    /// `(index into dsos(), dot x, dot y)` for every object drawn.
    #[allow(dead_code)] // the picker hit-tests against it next
    pub dso_placed: Vec<(usize, i32, i32)>,
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
    plot(view, opts, bodies, &[], x, y, w, h).frame
}

/// `panel` with marks drawn over the chart.
#[allow(clippy::too_many_arguments)]
pub fn panel_marked(view: &View, opts: &Opts, bodies: &[Body], marks: &[Mark], x: u16, y: u16, w: u16, h: u16) -> String {
    plot(view, opts, bodies, marks, x, y, w, h).frame
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn plot(
    view: &View,
    opts: &Opts,
    bodies: &[Body],
    marks: &[Mark],
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

    // Deep-sky objects under the stars, each at its own size, a small
    // ring where it is too small to see. Open clusters dashed, globulars
    // crossed, so the kinds tell apart without colour.
    let mut dso_placed = Vec::new();
    let mut dso_labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    if opts.dso {
        for (i, d) in dsos().iter().enumerate() {
            let Some((cu, pts)) = dso_outline(view, d, 32) else { continue };
            let c = to_dot(cu);
            if !in_frame(c) {
                continue;
            }
            let rgb = dso_rgb(d.kind);
            let dots: Vec<Option<(i32, i32)>> = pts.iter().map(|u| u.map(to_dot)).collect();
            let reach = dots.iter().flatten().map(|p| (p.0 - c.0).abs().max((p.1 - c.1).abs())).max().unwrap_or(0);
            if reach < 3 {
                canvas.circle(c.0 as f64, c.1 as f64, 2.0, rgb, 0.5);
            } else {
                for (k, pair) in dots.windows(2).enumerate() {
                    if d.kind == DsoKind::OpenCluster && k % 2 == 1 {
                        continue;
                    }
                    if let (Some(p), Some(q)) = (pair[0], pair[1]) {
                        canvas.line(p.0, p.1, q.0, q.1, rgb, 0.5);
                    }
                }
            }
            if d.kind == DsoKind::Globular {
                let k = reach.clamp(2, 4);
                canvas.line(c.0 - k, c.1, c.0 + k, c.1, rgb, 0.5);
                canvas.line(c.0, c.1 - k, c.0, c.1 + k, rgb, 0.5);
            }
            dso_placed.push((i, c.0, c.1));
            if let (true, Some(label)) = (opts.names && w >= 40 && h >= 10, dso_label(d, view.zoom)) {
                let (col, row) = (x + (c.0 + reach) as u16 / 2 + 1, y + c.1 as u16 / 4);
                if col + label.chars().count() as u16 + 1 < x + w {
                    dso_labels.push((col, row, label, rgb));
                }
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

    // Marks on top of everything.
    for m in marks {
        let Some(cu) = view.screen(m.ra, m.dec) else { continue };
        let c = to_dot(cu);
        if m.radius_deg <= 0.0 {
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                canvas.line(c.0 + 2 * dx, c.1 + 2 * dy, c.0 + 5 * dx, c.1 + 5 * dy, m.rgb, 100.0);
            }
            continue;
        }
        let dots: Vec<Option<(i32, i32)>> =
            sky_ellipse(view, m.ra, m.dec, m.radius_deg, m.radius_deg, 0.0, 64).iter().map(|u| u.map(to_dot)).collect();
        for pair in dots.windows(2) {
            if let (Some(p), Some(q)) = (pair[0], pair[1]) {
                canvas.line(p.0, p.1, q.0, q.1, m.rgb, 100.0);
            }
        }
    }

    // Place labels, bodies first: two names on one patch of sky print
    // over each other, and "jupiterollux" is worse than no Pollux.
    let mut taken: Vec<(u16, u16, u16)> = Vec::new();
    let mut labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    for (col, row, text, rgb) in body_labels.into_iter().chain(dso_labels).chain(star_labels) {
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

    Plot { frame, placed, mag_shown, dso_placed }
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
    /// `(index into dsos(), pixel x, pixel y)` for every object drawn.
    pub dso_placed: Vec<(usize, i32, i32)>,
}

/// Draw the sky as a picture for the rectangle at (`x`, `y`), `w`×`h`
/// cells at the terminal's cell size. Print the text, then show the canvas
/// at (`x`, `y`).
pub fn panel_pixels(view: &View, opts: &Opts, bodies: &[Body], x: u16, y: u16, w: u16, h: u16) -> Picture {
    picture(view, opts, bodies, &[], x, y, w, h, None)
}

/// `panel_pixels` with marks drawn over the chart.
#[allow(clippy::too_many_arguments)]
pub fn panel_pixels_marked(view: &View, opts: &Opts, bodies: &[Body], marks: &[Mark], x: u16, y: u16, w: u16, h: u16) -> Picture {
    picture(view, opts, bodies, marks, x, y, w, h, None)
}

/// The same for a given cell size.
#[allow(clippy::too_many_arguments)]
pub fn picture(view: &View, opts: &Opts, bodies: &[Body], marks: &[Mark], x: u16, y: u16, w: u16, h: u16, cell: Option<(u16, u16)>) -> Picture {
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

    // Deep-sky objects, under the stars; the same marks as in braille.
    let mut dso_placed = Vec::new();
    let mut dso_labels: Vec<(u16, u16, String, (u8, u8, u8))> = Vec::new();
    if opts.dso {
        let thick = dot * 0.35;
        for (i, d) in dsos().iter().enumerate() {
            let Some((cu, pts)) = dso_outline(view, d, 48) else { continue };
            let cp = to_px(cu);
            if !in_frame(cp) {
                continue;
            }
            let rgb = dso_rgb(d.kind);
            let px: Vec<Option<(f64, f64)>> = pts.iter().map(|u| u.map(to_px)).collect();
            let reach = px.iter().flatten().map(|p| (p.0 - cp.0).hypot(p.1 - cp.1)).fold(0.0, f64::max);
            let least = unit * 1.3;
            if reach < least {
                ring(&mut c, cp.0, cp.1, least, thick, rgb);
            } else {
                for (k, pair) in px.windows(2).enumerate() {
                    if d.kind == DsoKind::OpenCluster && k % 2 == 1 {
                        continue;
                    }
                    if let (Some(p), Some(q)) = (pair[0], pair[1]) {
                        c.line(p, q, thick, rgb, 1.0);
                    }
                }
            }
            if d.kind == DsoKind::Globular {
                let k = reach.max(least).min(unit * 3.0);
                c.line((cp.0 - k, cp.1), (cp.0 + k, cp.1), thick, rgb, 1.0);
                c.line((cp.0, cp.1 - k), (cp.0, cp.1 + k), thick, rgb, 1.0);
            }
            dso_placed.push((i, cp.0 as i32, cp.1 as i32));
            if let (true, Some(label)) = (opts.names && w >= 40 && h >= 10, dso_label(d, view.zoom)) {
                let (col, row) = (x + ((cp.0 + reach.max(least)) / cell_w) as u16 + 1, y + (cp.1 / cell_h) as u16);
                if col + label.chars().count() as u16 + 1 < x + w {
                    dso_labels.push((col, row, label, rgb));
                }
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

    for m in marks {
        let Some(cu) = view.screen(m.ra, m.dec) else { continue };
        let cp = to_px(cu);
        if m.radius_deg <= 0.0 {
            let (gap, len) = (unit * 1.4, unit * 4.0);
            for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                c.line((cp.0 + gap * dx, cp.1 + gap * dy), (cp.0 + len * dx, cp.1 + len * dy), dot * 0.4, m.rgb, 1.0);
            }
            continue;
        }
        let px: Vec<Option<(f64, f64)>> =
            sky_ellipse(view, m.ra, m.dec, m.radius_deg, m.radius_deg, 0.0, 96).iter().map(|u| u.map(to_px)).collect();
        for pair in px.windows(2) {
            if let (Some(p), Some(q)) = (pair[0], pair[1]) {
                c.line(p, q, dot * 0.45, m.rgb, 1.0);
            }
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
    for (col, row, label, rgb) in body_labels.into_iter().chain(dso_labels).chain(star_labels) {
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

    Picture { text, canvas: c, placed, mag_shown, dso_placed }
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
        let p = plot(&v, &Opts::default(), &[], &[], 1, 1, 100, 30);
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
        let wide = plot(&v, &Opts::default(), &[], &[], 1, 1, 100, 30).mag_shown;
        v.zoom = 8.0;
        let close = plot(&v, &Opts::default(), &[], &[], 1, 1, 100, 30).mag_shown;
        assert!(close > wide, "zoomed in should reach fainter: {wide} -> {close}");
    }

    #[test]
    fn the_picture_places_stars_and_cuts_holes_for_their_names() {
        let v = View::new(Projection::Hemisphere { north: true });
        let p = picture(&v, &Opts::default(), &[], &[], 1, 2, 100, 30, Some((10, 20)));
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
            let _ = picture(&v, &o, &[], &[], 1, 2, 190, 50, Some((10, 20)));
        }
        let per = t.elapsed() / 3;
        assert!(per.as_millis() < 400, "one picture took {per:?}");
        eprintln!("one picture: {per:?}");
        // STARMAP_DUMP=/some/file.png writes the picture out for a look.
        if let Ok(path) = std::env::var("STARMAP_DUMP") {
            let v = View::new(Projection::Horizon { lst_deg: 40.0, lat_deg: 59.9 });
            let _ = std::fs::write(path, picture(&v, &o, &[], &[], 1, 2, 190, 50, Some((10, 20))).canvas.png());
        }
    }

    #[test]
    fn deep_sky_objects_are_drawn_only_when_asked() {
        assert_eq!(dsos().len(), 219);
        let m31 = dsos().iter().find(|d| d.id == "M31").unwrap();
        assert_eq!((m31.alt, m31.kind, m31.constellation), ("NGC 224", DsoKind::Galaxy, "And"));
        let v = View::new(Projection::Hemisphere { north: true });
        let off = plot(&v, &Opts::default(), &[], &[], 1, 1, 150, 42);
        assert!(off.dso_placed.is_empty());
        let on = plot(&v, &Opts { dso: true, ..Opts::default() }, &[], &[], 1, 1, 150, 42);
        let ids: Vec<&str> = on.dso_placed.iter().map(|&(i, _, _)| dsos()[i].id).collect();
        assert!(ids.contains(&"M31") && ids.contains(&"M13"), "northern objects missing");
        assert!(!ids.contains(&"C99"), "the Coalsack is a southern object");
        let pic = picture(&v, &Opts { dso: true, ..Opts::default() }, &[], &[], 1, 2, 150, 42, Some((10, 20)));
        assert!(pic.dso_placed.len() > 100, "{} objects in the picture", pic.dso_placed.len());
        assert!(pic.text.contains("M31"), "the brightest objects are labelled");
    }

    /// STARMAP_DSO_DUMP=/some/dir writes three pictures with the objects on:
    /// the northern map, and close-ups of Orion's sword and Andromeda.
    #[test]
    fn dump_deep_sky_pictures() {
        let Ok(dir) = std::env::var("STARMAP_DSO_DUMP") else { return };
        let o = Opts { dso: true, ..Opts::default() };
        let mut v = View::new(Projection::Hemisphere { north: true });
        let _ = std::fs::write(format!("{dir}/north.png"), picture(&v, &o, &[], &[], 1, 2, 190, 50, Some((10, 20))).canvas.png());
        for (name, ra, dec, zoom) in [("orion", 83.8, -5.4, 18.0), ("andromeda", 10.7, 41.3, 14.0)] {
            v = View::new(Projection::Horizon { lst_deg: ra, lat_deg: 30.0 });
            v.pan = v.place(ra, dec).unwrap();
            v.zoom = zoom;
            let p = picture(&v, &o, &[], &[], 1, 2, 190, 50, Some((10, 20)));
            let _ = std::fs::write(format!("{dir}/{name}.png"), p.canvas.png());
            let _ = std::fs::write(format!("{dir}/{name}.txt"), crust::strip_ansi(&p.text));
        }
    }

    #[test]
    fn one_frame_is_quick() {
        let v = View::new(Projection::Hemisphere { north: true });
        let o = Opts::default();
        let t = std::time::Instant::now();
        for _ in 0..20 {
            let _ = plot(&v, &o, &[], &[], 1, 1, 150, 42);
        }
        let per = t.elapsed() / 20;
        assert!(per.as_millis() < 20, "one frame took {per:?}");
    }
}
