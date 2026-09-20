//! # starmap
//!
//! The naked-eye sky in a terminal: real pixels through glow where the
//! terminal shows images, braille elsewhere.
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
//! [`panel`] draws a chart into a rectangle in braille; [`panel_pixels`]
//! draws the same chart as a picture for glow, with the names as text
//! showing through holes in it. [`pick`] runs an interactive braille one
//! that owns the keyboard and hands back the star the user chose.
//!
//! Star colours are the colour of a black body at the star's temperature,
//! from its B−V index (Flower 1996 as corrected by Torres 2010, Ballesteros
//! 2012 for the reddest) or its spectral type, through the CIE 1931
//! observer to sRGB. No network, no allocation per frame beyond the
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
pub use render::{panel, panel_pixels, Body, Opts, Picture};

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

/// Effective temperature from the B−V colour index. Flower's (1996)
/// polynomial with the coefficients as Torres (2010) corrected them,
/// which fits real stars from late O to late K; past B−V 1.5 it runs
/// away, so the reddest stars blend into Ballesteros' (2012) black-body
/// fit instead.
pub fn teff_from_bv(bv: f64) -> f64 {
    let ballesteros = 4600.0 * (1.0 / (0.92 * bv + 1.7) + 1.0 / (0.92 * bv + 0.62));
    if bv > 1.6 {
        return ballesteros;
    }
    const A: [f64; 8] = [
        3.979_145_106_714_099,
        -0.654_992_268_598_245,
        1.740_690_042_385_095,
        -4.608_815_154_057_166,
        6.792_599_779_944_473,
        -5.396_909_891_322_525,
        2.192_970_376_522_490,
        -0.359_495_739_295_671,
    ];
    let x = bv.max(-0.35);
    let log_t = A.iter().rev().fold(0.0, |acc, a| acc * x + a);
    let flower = 10f64.powf(log_t);
    if bv <= 1.3 {
        flower
    } else {
        let f = (bv - 1.3) / 0.3;
        flower * (1.0 - f) + ballesteros * f
    }
}

/// Effective temperature from a spectral type such as `K1III` or `A1Vm`,
/// for a star the catalogue has no B−V for. Dwarf calibration (Pecaut &
/// Mamajek 2013), read between the listed subclasses.
pub fn teff_from_spectral(spectral: &str) -> Option<f64> {
    const CLASSES: [(u8, [f64; 10]); 7] = [
        (b'O', [44000.0, 43000.0, 42000.0, 41000.0, 40000.0, 39000.0, 37000.0, 35000.0, 33000.0, 31500.0]),
        (b'B', [30000.0, 25400.0, 20600.0, 17000.0, 16400.0, 15700.0, 14500.0, 14000.0, 12500.0, 10700.0]),
        (b'A', [9700.0, 9200.0, 8840.0, 8550.0, 8270.0, 8080.0, 8000.0, 7800.0, 7500.0, 7440.0]),
        (b'F', [7220.0, 7020.0, 6820.0, 6750.0, 6670.0, 6550.0, 6350.0, 6280.0, 6180.0, 6050.0]),
        (b'G', [5930.0, 5860.0, 5770.0, 5720.0, 5680.0, 5660.0, 5600.0, 5550.0, 5480.0, 5380.0]),
        (b'K', [5270.0, 5170.0, 5100.0, 4830.0, 4600.0, 4440.0, 4300.0, 4100.0, 3990.0, 3930.0]),
        (b'M', [3850.0, 3660.0, 3560.0, 3430.0, 3210.0, 3060.0, 2810.0, 2680.0, 2570.0, 2380.0]),
    ];
    let b = spectral.trim_start_matches(|c: char| !c.is_ascii_alphabetic()).as_bytes();
    let letter = b.first()?.to_ascii_uppercase();
    let row = CLASSES.iter().find(|(l, _)| *l == letter)?.1;
    let sub = b.get(1).filter(|d| d.is_ascii_digit()).map_or(5.0, |d| (d - b'0') as f64);
    // A decimal subclass like K2.5 reads between two rows.
    let frac = if b.get(2) == Some(&b'.') { b.get(3).filter(|d| d.is_ascii_digit()).map_or(0.0, |d| (d - b'0') as f64 / 10.0) } else { 0.0 };
    let i = sub as usize;
    let next = if i + 1 < 10 { row[i + 1] } else { CLASSES.iter().position(|(l, _)| *l == letter).and_then(|k| CLASSES.get(k + 1)).map_or(row[9], |c| c.1[0]) };
    Some(row[i] * (1.0 - frac) + next * frac)
}

/// The temperature to colour a star by: from B−V when the catalogue has
/// it, else from the spectral type, else a Sun-like default.
pub fn star_teff(s: &Star) -> f64 {
    match s.bv {
        Some(bv) => teff_from_bv(bv),
        None => teff_from_spectral(s.spectral).unwrap_or(5800.0),
    }
}

/// The colour a black body of this temperature shows to the eye, as sRGB
/// with the brightest channel at full: brightness is drawn by size, the
/// colour by the spectrum. Read from a table of 400 temperatures spaced
/// evenly in the logarithm from 1000 K to 60000 K, a step of one percent,
/// built once from [`black_body_rgb`].
pub fn teff_rgb(teff: f64) -> (u8, u8, u8) {
    static TABLE: OnceLock<Vec<(u8, u8, u8)>> = OnceLock::new();
    const N: usize = 400;
    let (lo, hi) = (1000f64.ln(), 60000f64.ln());
    let table = TABLE.get_or_init(|| {
        (0..N).map(|i| black_body_rgb((lo + (hi - lo) * i as f64 / (N - 1) as f64).exp())).collect()
    });
    let f = ((teff.clamp(1000.0, 60000.0).ln() - lo) / (hi - lo) * (N - 1) as f64).clamp(0.0, (N - 1) as f64);
    let (i, w) = (f as usize, f.fract());
    let (a, b) = (table[i], table[(i + 1).min(N - 1)]);
    let mix = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * w).round() as u8;
    (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
}

/// The colour worked out from the spectrum: Planck's law over the CIE
/// 1931 observer (Wyman, Sloan and Shirley's 2013 fit of the matching
/// functions), to XYZ, to linear sRGB with the D65 white, then the sRGB
/// curve. About a fifth of a millisecond, so the table above is what
/// the charts read.
pub fn black_body_rgb(teff: f64) -> (u8, u8, u8) {
    let t = teff.clamp(1000.0, 60000.0);
    let lobe = |l: f64, mu: f64, s1: f64, s2: f64| {
        let s = if l < mu { s1 } else { s2 };
        (-0.5 * ((l - mu) / s).powi(2)).exp()
    };
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    for nm in 360..=830 {
        let l = nm as f64;
        // Planck, up to a constant: the normalisation goes at the end.
        let b = (l * 1e-9).powi(-5) / ((1.438_776_9e-2 / (l * 1e-9 * t)).exp() - 1.0);
        x += b * (1.056 * lobe(l, 599.8, 37.9, 31.0) + 0.362 * lobe(l, 442.0, 16.0, 26.7) - 0.065 * lobe(l, 501.1, 20.4, 26.2));
        y += b * (0.821 * lobe(l, 568.8, 46.9, 40.5) + 0.286 * lobe(l, 530.9, 16.3, 31.1));
        z += b * (1.217 * lobe(l, 437.0, 11.8, 36.0) + 0.681 * lobe(l, 459.0, 26.0, 13.8));
    }
    let lin = [
        3.2406 * x - 1.5372 * y - 0.4986 * z,
        -0.9689 * x + 1.8758 * y + 0.0415 * z,
        0.0557 * x - 0.2040 * y + 1.0570 * z,
    ];
    let top = lin.iter().cloned().fold(f64::MIN, f64::max).max(1e-12);
    let curve = |c: f64| {
        let c = (c / top).max(0.0);
        let c = if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
        (c * 255.0).round().clamp(0.0, 255.0) as u8
    };
    (curve(lin[0]), curve(lin[1]), curve(lin[2]))
}

/// The colour to draw a star in braille: its own, dimmed by how faint it
/// is. One dot is one dot, so brightness has to live in the colour.
pub fn star_rgb(s: &Star) -> (u8, u8, u8) {
    let rgb = teff_rgb(star_teff(s));
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
    fn temperatures_match_the_textbook_stars() {
        let sun = teff_from_bv(0.65);
        assert!((5600.0..5850.0).contains(&sun), "Sun-like B-V 0.65 gave {sun}");
        let vega = teff_from_bv(0.0);
        assert!((9300.0..9800.0).contains(&vega), "Vega-like B-V 0.0 gave {vega}");
        let m = teff_from_bv(1.85);
        assert!((3100.0..3700.0).contains(&m), "Betelgeuse-like B-V 1.85 gave {m}");
        assert!((teff_from_spectral("K2.5III").unwrap() - 4965.0).abs() < 1.0);
        assert!((teff_from_spectral("M2").unwrap() - 3560.0).abs() < 1.0);
        assert!(teff_from_spectral("").is_none());
    }

    #[test]
    fn black_body_colours_go_from_orange_through_white_to_blue() {
        let (r, g, b) = teff_rgb(3000.0);
        assert!(r == 255 && (170..200).contains(&g) && (95..125).contains(&b), "3000 K gave {r} {g} {b}");
        let (t, e) = (teff_rgb(3000.0), black_body_rgb(3000.0));
        assert!(t.1.abs_diff(e.1) <= 1 && t.2.abs_diff(e.2) <= 1, "table {t:?} against the spectrum {e:?}");
        let (r, g, b) = teff_rgb(5800.0);
        assert!(r == 255 && g > 235 && (225..245).contains(&b), "5800 K gave {r} {g} {b}");
        let (r, g, b) = teff_rgb(20000.0);
        assert!(b == 255 && (160..185).contains(&r) && g > r, "20000 K gave {r} {g} {b}");
    }


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
