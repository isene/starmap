//! Putting a right ascension and a declination on the screen.
//!
//! Both projections here are azimuthal equidistant: distance from the
//! centre is proportional to angle from the centre. That is what paper
//! planispheres use, it keeps constellations recognisable, and it makes
//! zoom a plain multiplication.

/// Local sidereal time in degrees, from a Julian date and a longitude.
/// Handy for callers that have no ephemeris of their own; anything with
/// one (astro has orbit) should pass its own value.
pub fn lst_deg(jd: f64, lon_deg: f64) -> f64 {
    let d = jd - 2451545.0;
    let gmst = 280.46061837 + 360.98564736629 * d;
    (gmst + lon_deg).rem_euclid(360.0)
}

/// Altitude and azimuth (from north through east) of an equatorial
/// position for an observer at `lat`, when the local sidereal time is
/// `lst` degrees.
pub fn altaz(ra: f64, dec: f64, lst: f64, lat: f64) -> (f64, f64) {
    let ha = (lst - ra).to_radians();
    let (d, p) = (dec.to_radians(), lat.to_radians());
    let (sin_ha, cos_ha) = ha.sin_cos();
    let (sin_d, cos_d) = d.sin_cos();
    let (sin_p, cos_p) = p.sin_cos();
    let alt = (sin_d * sin_p + cos_d * cos_p * cos_ha).clamp(-1.0, 1.0).asin();
    let az = (-cos_d * sin_ha).atan2(sin_d * cos_p - cos_d * sin_p * cos_ha);
    (alt.to_degrees(), (az.to_degrees() + 360.0) % 360.0)
}

/// Which sky, and from where.
#[derive(Clone, Copy, Debug)]
pub enum Projection {
    /// What is over your head now: zenith at the centre, horizon at the
    /// rim, north up and east to the left, because you are looking up
    /// rather than at a map.
    Horizon { lst_deg: f64, lat_deg: f64 },
    /// The celestial sphere as a map: pole at the centre, equator at the
    /// rim, right ascension running anticlockwise from the top.
    Hemisphere { north: bool },
}

/// A projection plus where the eye is inside it.
///
/// Positions come out in a unit disc: the rim of the sky is at radius 1
/// when `zoom` is 1. Zooming multiplies, panning slides the centre, so
/// the two compose without any special cases.
#[derive(Clone, Copy, Debug)]
pub struct View {
    pub proj: Projection,
    pub zoom: f64,
    /// Where the centre of the screen sits in unit-disc coordinates.
    pub pan: (f64, f64),
}

impl View {
    pub fn new(proj: Projection) -> Self {
        Self { proj, zoom: 1.0, pan: (0.0, 0.0) }
    }

    /// Unit-disc position of an equatorial point, or `None` when it is
    /// not in this sky at all (below the horizon, or the wrong
    /// hemisphere).
    pub fn place(&self, ra: f64, dec: f64) -> Option<(f64, f64)> {
        match self.proj {
            Projection::Horizon { lst_deg, lat_deg } => {
                let (alt, az) = altaz(ra, dec, lst_deg, lat_deg);
                if alt < 0.0 {
                    return None;
                }
                // Straight down from the zenith is the centre, the
                // horizon is the rim, east is to the left.
                let r = (90.0 - alt) / 90.0;
                let a = az.to_radians();
                Some((-r * a.sin(), -r * a.cos()))
            }
            Projection::Hemisphere { north } => {
                // A whole hemisphere plus 30° of the other side, so the
                // constellations that straddle the equator stay whole on
                // both maps: Orion, and Canis Major with Sirius in it.
                let pole_dist = if north { 90.0 - dec } else { 90.0 + dec };
                if pole_dist > 120.0 {
                    return None;
                }
                let r = pole_dist / 90.0;
                // RA anticlockwise from the top on the north map, the
                // mirror of it on the south, which is how the sky turns
                // seen from each pole.
                let a = ra.to_radians() * if north { 1.0 } else { -1.0 };
                Some((-r * a.sin(), -r * a.cos()))
            }
        }
    }

    /// Unit-disc position after zoom and pan: what actually gets drawn.
    pub fn screen(&self, ra: f64, dec: f64) -> Option<(f64, f64)> {
        let (x, y) = self.place(ra, dec)?;
        Some((
            (x - self.pan.0) * self.zoom,
            (y - self.pan.1) * self.zoom,
        ))
    }

    /// How much sky one unit of the disc covers, in degrees. Used for
    /// labels and for deciding how faint to go.
    pub fn degrees_across(&self) -> f64 {
        180.0 / self.zoom
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Polaris sits at altitude ≈ latitude, due north, at every hour.
    #[test]
    fn polaris_holds_still() {
        for lst in [0.0, 90.0, 187.5, 300.0] {
            let (alt, az) = altaz(37.95, 89.26, lst, 59.9);
            assert!((alt - 59.9).abs() < 1.0, "alt {alt} at lst {lst}");
            assert!(az < 3.0 || az > 357.0, "az {az} at lst {lst}");
        }
    }

    #[test]
    fn horizon_hides_what_is_down() {
        let v = View::new(Projection::Horizon { lst_deg: 0.0, lat_deg: 60.0 });
        // The south celestial pole is never up from +60°.
        assert!(v.place(0.0, -90.0).is_none());
        // The north celestial pole always is, and lands two thirds out.
        let (x, y) = v.place(0.0, 90.0).unwrap();
        let r = (x * x + y * y).sqrt();
        assert!((r - (90.0 - 60.0) / 90.0).abs() < 0.02, "r {r}");
    }

    #[test]
    fn hemisphere_puts_the_pole_in_the_middle() {
        let n = View::new(Projection::Hemisphere { north: true });
        let (x, y) = n.place(123.0, 90.0).unwrap();
        assert!(x.hypot(y) < 1e-9, "north pole should be dead centre");
        // The equator lands on the rim.
        let (x, y) = n.place(0.0, 0.0).unwrap();
        assert!((x.hypot(y) - 1.0).abs() < 1e-9);
        // Deep south is off this map.
        assert!(n.place(0.0, -30.0).is_some(), "a little south still shows");
        assert!(n.place(0.0, -60.0).is_none(), "far south does not");
    }

    #[test]
    fn zoom_scales_and_pan_slides() {
        let mut v = View::new(Projection::Hemisphere { north: true });
        let far = v.screen(0.0, 0.0).unwrap();
        v.zoom = 4.0;
        let near = v.screen(0.0, 0.0).unwrap();
        assert!((near.0 / far.0 - 4.0).abs() < 1e-9 || near.0.abs() < 1e-12);
        v.pan = v.place(0.0, 0.0).unwrap();
        let centred = v.screen(0.0, 0.0).unwrap();
        assert!(centred.0.hypot(centred.1) < 1e-9, "panned-to point is centred");
    }

    #[test]
    fn sidereal_time_advances_a_degree_an_hour() {
        let a = lst_deg(2461250.0, 10.7);
        let b = lst_deg(2461250.0 + 1.0 / 24.0, 10.7);
        let gained = (b - a).rem_euclid(360.0);
        assert!((gained - 15.04).abs() < 0.1, "gained {gained}");
    }
}
