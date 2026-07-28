//! The interactive sky: walk a crosshair over it and choose a star.

use crate::proj::{Projection, View};
use crate::render::{plot, Body, Opts};
use crate::{stars, Star};
use crust::style;
use crust::{Crust, Cursor, Input};
use std::io::Write;

/// What came back from the picker.
pub struct Picked {
    pub star: &'static Star,
    /// Its index in [`crate::stars`].
    pub index: usize,
}

/// Run the picker full screen and return the star the user chose, or
/// `None` if they backed out.
///
/// Arrows walk the crosshair, `+` / `-` zoom about it, `f` flips between
/// the northern and southern half of the sky, `c` and `n` toggle the
/// figures and the names, Enter takes the star under the crosshair.
///
/// The caller owns the screen afterwards: this leaves the terminal as it
/// found it but does not redraw whatever was there before.
pub fn pick(start: View, mut opts: Opts, title: &str) -> Option<Picked> {
    let mut view = start;
    // Dot coordinates of the crosshair, from the top left of the chart.
    let mut cross: Option<(i32, i32)> = None;

    loop {
        let (cols, rows) = Crust::terminal_size();
        let (w, h) = (cols, rows.saturating_sub(2));
        let (dw, dh) = (w as i32 * 2, h as i32 * 4);
        let cur = *cross.get_or_insert((dw / 2, dh / 2));

        let p = plot(&view, &opts, &[] as &[Body], 1, 2, w, h);
        // The star nearest the crosshair, within a few dots.
        let target = p
            .placed
            .iter()
            .map(|&(i, x, y)| {
                let (dx, dy) = ((x - cur.0) as f64, (y - cur.1) as f64);
                (i, dx * dx + dy * dy)
            })
            .filter(|&(_, d2)| d2 <= 64.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i);

        Crust::clear_screen();
        print!("{}", p.frame);

        // The crosshair, or the star it has hold of. Marking the star's
        // own cell rather than bracketing it keeps the mark off its
        // neighbours' name labels, which are only two columns away.
        let (mark, at) = match target.and_then(|i| p.placed.iter().find(|&&(j, _, _)| j == i)) {
            Some(&(_, x, y)) => ("*", (1 + x as u16 / 2, 2 + y as u16 / 4)),
            None => ("+", (1 + cur.0 as u16 / 2, 2 + cur.1 as u16 / 4)),
        };
        print!(
            "{}{}",
            Cursor::at(at.0, at.1),
            style::rgb(mark, Some((255, 220, 120)), None, "b")
        );

        // Title row and the star under the crosshair.
        let where_ = match view.proj {
            Projection::Hemisphere { north } => {
                if north { "northern sky" } else { "southern sky" }
            }
            Projection::Horizon { .. } => "the sky over you",
        };
        let head = format!(
            " {}  ·  {}  ·  ×{:.0} zoom · stars to mag {:.1} ",
            title, where_, view.zoom, p.mag_shown
        );
        print!(
            "{}{}",
            Cursor::at(1, 1),
            style::rgb(&crust::truncate_ansi(&head, cols as usize), Some((255, 200, 120)), None, "b")
        );

        let foot = match target {
            Some(i) => {
                let s = &stars()[i];
                let dist = match s.dist_pc {
                    Some(d) => format!("{:.0} ly", d * 3.2616),
                    None => "distance unknown".into(),
                };
                format!(
                    " {}  mag {:.2}  {}  {}   ⏎ take it · ←↓↑→ move · +/- zoom · f flip · c figures · n names · q back",
                    s.label(), s.mag, s.spectral, dist
                )
            }
            None => " ←↓↑→ move · +/- zoom · f flip · c figures · n names · ⏎ take the star under the cross · q back".into(),
        };
        print!(
            "{}{}",
            Cursor::at(1, rows),
            style::dim(&crust::truncate_ansi(&foot, cols as usize))
        );
        print!("{}", Cursor::hide_seq());
        std::io::stdout().flush().ok();

        let Some(key) = Input::getchr(None) else { continue };
        // Moving: the crosshair walks until it reaches the edge, then the
        // sky slides under it instead.
        let step = 4;
        let walk = |dx: i32, dy: i32, cross: &mut Option<(i32, i32)>, view: &mut View| {
            let (mut x, mut y) = cross.unwrap();
            x += dx * step;
            y += dy * step;
            let r = (dw.min(dh) as f64 / 2.0) - 2.0;
            if x < 0 || x >= dw {
                x -= dx * step;
                view.pan.0 += dx as f64 * step as f64 / r / view.zoom;
            }
            if y < 0 || y >= dh {
                y -= dy * step;
                view.pan.1 += dy as f64 * step as f64 / r / view.zoom;
            }
            *cross = Some((x, y));
        };

        match key.as_str() {
            "q" | "Q" | "ESC" => return None,
            "ENTER" | " " => {
                if let Some(i) = target {
                    return Some(Picked { star: &stars()[i], index: i });
                }
            }
            "RIGHT" | "l" => walk(1, 0, &mut cross, &mut view),
            "LEFT" | "h" => walk(-1, 0, &mut cross, &mut view),
            "DOWN" | "j" => walk(0, 1, &mut cross, &mut view),
            "UP" | "k" => walk(0, -1, &mut cross, &mut view),
            "+" | "=" | "PgUP" => zoom_about(&mut view, cur, (dw, dh), 1.5),
            "-" | "_" | "PgDOWN" => zoom_about(&mut view, cur, (dw, dh), 1.0 / 1.5),
            "f" | "F" => {
                if let Projection::Hemisphere { north } = view.proj {
                    view.proj = Projection::Hemisphere { north: !north };
                    view.pan = (0.0, 0.0);
                    cross = None;
                }
            }
            "0" => {
                view.zoom = 1.0;
                view.pan = (0.0, 0.0);
                cross = None;
            }
            "c" | "C" => opts.figures = !opts.figures,
            "n" | "N" => opts.names = !opts.names,
            "RESIZE" => cross = None,
            _ => {}
        }
    }
}

/// Zoom in or out while keeping the sky under the crosshair still.
fn zoom_about(view: &mut View, cross: (i32, i32), dots: (i32, i32), by: f64) {
    let (dw, dh) = (dots.0 as f64, dots.1 as f64);
    let r = (dw.min(dh) / 2.0) - 2.0;
    let off = (
        (cross.0 as f64 - dw / 2.0) / r,
        (cross.1 as f64 - dh / 2.0) / r,
    );
    // What the crosshair is on now, in unit-disc coordinates.
    let under = (
        view.pan.0 + off.0 / view.zoom,
        view.pan.1 + off.1 / view.zoom,
    );
    view.zoom = (view.zoom * by).clamp(1.0, 60.0);
    view.pan = (under.0 - off.0 / view.zoom, under.1 - off.1 / view.zoom);
}
