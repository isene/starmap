//! # starmap
//!
//! The naked-eye sky, drawn in braille in a terminal.
//!
//! Two embedded tables: 9,096 stars from the Yale Bright Star Catalogue
//! with Hipparcos distances, and the 150 constellation stick figures.
//! Two ways to look at them:
//!
//! * [`Projection::Horizon`] — the sky over an observer's head at an
//!   instant. Zenith at the centre, horizon at the rim, north up and east
//!   to the left, the way a planisphere reads.
//! * [`Projection::Hemisphere`] — the celestial sphere as a map, pole at
//!   the centre, equator at the rim, north or south.
//!
//! [`panel`] draws a chart into a rectangle. [`pick`] runs an interactive
//! one that owns the keyboard and hands back the star the user chose.
//!
//! No network, no image protocol, no allocation per frame beyond the
//! canvas, and nothing at all between keypresses.
//!
//! ```no_run
//! let s = starmap::stars();
//! let sirius = s.iter().find(|s| s.name == "Sirius").unwrap();
//! assert!((sirius.dist_pc.unwrap() - 2.64).abs() < 0.01);
//! ```

mod canvas;
mod pick;
mod proj;
mod render;

pub use canvas::Canvas;
pub use pick::{pick, Picked};
pub use proj::{altaz, lst_deg, Projection, View};
pub use render::{panel, Body, Opts};

use std::sync::OnceLock;

/// Yale Bright Star Catalogue with Hipparcos parallaxes:
/// `ra,dec,mag,bv,spectral,hd,hip,dist_pc,name`, brightest first.
const STAR_DATA: &str = include_str!("../data/stars.csv");
/// Constellation stick figures: `ABR:ra,dec ra,dec …`, one stroke each.
const LINE_DATA: &str = include_str!("../data/constellations.csv");

/// One catalogue star. Positions are J2000 degrees.
#[derive(Debug, Clone)]
pub struct Star {
    pub ra: f64,
    pub dec: f64,
    /// Apparent visual magnitude.
    pub mag: f64,
    /// B−V colour index, where the catalogue has one.
    pub bv: Option<f64>,
    /// Spectral type as the catalogue spells it, e.g. `A1Vm`.
    pub spectral: &'static str,
    /// Henry Draper number, 0 if none.
    pub hd: u32,
    /// Hipparcos number, 0 if none.
    pub hip: u32,
    /// Distance in parsecs, `None` where the parallax is negative or
    /// worse than 20% uncertain. About 81% of the catalogue has one.
    pub dist_pc: Option<f64>,
    /// IAU proper name, empty for the 8,763 stars without one.
    pub name: &'static str,
}

impl Star {
    /// Absolute visual magnitude, if the distance is known.
    /// `M = m + 5 − 5·log₁₀(d)`.
    pub fn absmag(&self) -> Option<f64> {
        self.dist_pc.map(|d| self.mag + 5.0 - 5.0 * d.log10())
    }

    /// What to call it: the proper name, else HD, else HIP, else the
    /// position. Never empty, so a caller can always label a pick.
    pub fn label(&self) -> String {
        if !self.name.is_empty() {
            self.name.to_string()
        } else if self.hd > 0 {
            format!("HD {}", self.hd)
        } else if self.hip > 0 {
            format!("HIP {}", self.hip)
        } else {
            format!("{:.2} {:+.2}", self.ra, self.dec)
        }
    }
}

/// One stroke of a constellation figure, as equatorial waypoints.
pub struct Figure {
    /// Three-letter IAU abbreviation, e.g. `Ori`.
    pub abbr: &'static str,
    pub pts: Vec<(f64, f64)>,
}

/// Every catalogue star, brightest first. Parsed once, on first call.
pub fn stars() -> &'static [Star] {
    &catalog().0
}

/// Every constellation stroke.
pub fn figures() -> &'static [Figure] {
    &catalog().1
}

fn catalog() -> &'static (Vec<Star>, Vec<Figure>) {
    static CAT: OnceLock<(Vec<Star>, Vec<Figure>)> = OnceLock::new();
    CAT.get_or_init(|| {
        let mut stars = Vec::with_capacity(9200);
        for line in STAR_DATA.lines() {
            if line.starts_with('#') {
                continue;
            }
            let f: Vec<&str> = line.splitn(9, ',').collect();
            if f.len() < 9 {
                continue;
            }
            let (Ok(ra), Ok(dec), Ok(mag)) = (f[0].parse(), f[1].parse(), f[2].parse()) else {
                continue;
            };
            stars.push(Star {
                ra,
                dec,
                mag,
                bv: f[3].parse().ok(),
                spectral: f[4],
                hd: f[5].parse().unwrap_or(0),
                hip: f[6].parse().unwrap_or(0),
                dist_pc: f[7].parse().ok().filter(|d: &f64| *d > 0.0),
                name: f[8],
            });
        }
        let mut figs = Vec::with_capacity(160);
        for line in LINE_DATA.lines() {
            let Some((abbr, rest)) = line.split_once(':') else { continue };
            let pts: Vec<(f64, f64)> = rest
                .split(' ')
                .filter_map(|p| {
                    let (a, b) = p.split_once(',')?;
                    Some((a.parse().ok()?, b.parse().ok()?))
                })
                .collect();
            if pts.len() > 1 {
                figs.push(Figure { abbr, pts });
            }
        }
        (stars, figs)
    })
}

/// Effective temperature from the B−V colour index (Ballesteros 2012).
pub fn teff_from_bv(bv: f64) -> f64 {
    4600.0 * (1.0 / (0.92 * bv + 1.7) + 1.0 / (0.92 * bv + 0.62))
}

/// The colour a star of this temperature shows, saturated a little past
/// the truth so A and F whites stay apart on a black terminal.
pub fn teff_rgb(teff: f64) -> (u8, u8, u8) {
    const STOPS: [(f64, (u8, u8, u8)); 7] = [
        (40000.0, (120, 150, 255)),
        (20000.0, (160, 195, 255)),
        (9700.0, (225, 235, 255)),
        (7200.0, (255, 245, 200)),
        (5800.0, (255, 215, 90)),
        (4400.0, (255, 150, 60)),
        (3000.0, (255, 90, 60)),
    ];
    let t = teff.clamp(3000.0, 40000.0);
    for w in STOPS.windows(2) {
        let ((t0, c0), (t1, c1)) = (w[0], w[1]);
        if t <= t0 && t >= t1 {
            let f = (t0.ln() - t.ln()) / (t0.ln() - t1.ln());
            let mix = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * f) as u8;
            return (mix(c0.0, c1.0), mix(c0.1, c1.1), mix(c0.2, c1.2));
        }
    }
    STOPS[0].1
}

/// The colour to draw a star: its own, dimmed by how faint it is. One
/// dot is one dot, so brightness has to live in the colour.
pub fn star_rgb(s: &Star) -> (u8, u8, u8) {
    let rgb = teff_rgb(teff_from_bv(s.bv.unwrap_or(0.0)));
    let f = (1.15 - (s.mag + 1.5) * 0.115).clamp(0.32, 1.0);
    (
        (rgb.0 as f64 * f) as u8,
        (rgb.1 as f64 * f) as u8,
        (rgb.2 as f64 * f) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogue_parses_brightest_first() {
        let s = stars();
        assert!(s.len() > 9000, "only {} stars", s.len());
        assert_eq!(s[0].name, "Sirius");
        assert!((s[0].mag + 1.46).abs() < 0.01);
        assert!(s.windows(2).all(|w| w[0].mag <= w[1].mag), "not sorted by magnitude");
    }

    /// Distances against values every amateur knows.
    #[test]
    fn distances_are_right() {
        let by = |n: &str| stars().iter().find(|s| s.name == n).unwrap().clone();
        assert!((by("Sirius").dist_pc.unwrap() - 2.64).abs() < 0.05);
        assert!((by("Vega").dist_pc.unwrap() - 7.68).abs() < 0.15);
        assert!((by("Canopus").dist_pc.unwrap() - 95.9).abs() < 3.0);
        // Absolute magnitude of Sirius is +1.45.
        assert!((by("Sirius").absmag().unwrap() - 1.45).abs() < 0.1);
        // Betelgeuse's Hipparcos parallax is 7.63 ± 1.64 mas, 21% off,
        // which is why its distance is argued about in the literature.
        // Over the 20% bar, so the catalogue admits it does not know.
        assert!(by("Betelgeuse").dist_pc.is_none(), "should refuse a bad parallax");
    }

    #[test]
    fn most_stars_have_a_distance() {
        let n = stars().iter().filter(|s| s.dist_pc.is_some()).count();
        assert!(n > 7000, "only {n} with distance");
        assert!(n < stars().len(), "all of them? the filter is not working");
    }

    #[test]
    fn cross_ids_and_labels() {
        let sirius = stars().iter().find(|s| s.name == "Sirius").unwrap();
        assert_eq!(sirius.hd, 48915);
        assert_eq!(sirius.hip, 32349);
        let nameless = stars().iter().find(|s| s.name.is_empty() && s.hd > 0).unwrap();
        assert!(nameless.label().starts_with("HD "));
    }

    #[test]
    fn figures_load() {
        let f = figures();
        assert!(f.len() > 100, "only {} strokes", f.len());
        assert!(f.iter().any(|f| f.abbr == "Ori"));
    }

    #[test]
    fn colour_runs_blue_to_red() {
        let hot = teff_rgb(teff_from_bv(-0.3));
        let cool = teff_rgb(teff_from_bv(1.6));
        assert!(hot.2 > hot.0, "hot star should be blue-ish");
        assert!(cool.0 > cool.2, "cool star should be red-ish");
    }
}
