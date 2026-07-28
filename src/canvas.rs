//! A braille dot canvas.
//!
//! Braille cells carry a 2×4 dot matrix, so one character cell is eight
//! sub-pixels and those dots come out close to square in every monospace
//! font worth using. Colour is per CELL, since a braille glyph is one
//! character: the brightest thing in a cell wins it.

use crust::style;
use crust::Cursor;

/// Braille dot bit for a sub-pixel within a cell. Rows 0-2 use bits
/// 0,1,2 / 3,4,5; row 3 uses bits 6,7.
const DOTS: [[u8; 2]; 4] = [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

pub struct Canvas {
    pub w: usize,
    pub h: usize,
    bits: Vec<u8>,
    color: Vec<Option<(u8, u8, u8)>>,
    weight: Vec<f64>,
}

impl Canvas {
    /// A canvas `w`×`h` character cells, so `2w`×`4h` dots.
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            bits: vec![0; w * h],
            color: vec![None; w * h],
            weight: vec![f64::NEG_INFINITY; w * h],
        }
    }

    /// Dots across and dots down.
    pub fn dots(&self) -> (f64, f64) {
        (self.w as f64 * 2.0, self.h as f64 * 4.0)
    }

    /// Light one dot. `weight` decides who owns the cell's colour when
    /// several things land in it: a bright star beats a figure line.
    pub fn set(&mut self, x: i32, y: i32, rgb: (u8, u8, u8), weight: f64) {
        if x < 0 || y < 0 {
            return;
        }
        let (px, py) = (x as usize, y as usize);
        let (cx, cy) = (px / 2, py / 4);
        if cx >= self.w || cy >= self.h {
            return;
        }
        let i = cy * self.w + cx;
        self.bits[i] |= DOTS[py % 4][px % 2];
        if weight > self.weight[i] {
            self.weight[i] = weight;
            self.color[i] = Some(rgb);
        }
    }

    pub fn disc(&mut self, x: i32, y: i32, r: i32, rgb: (u8, u8, u8), weight: f64) {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx * dx + dy * dy <= r * r {
                    self.set(x + dx, y + dy, rgb, weight);
                }
            }
        }
    }

    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, rgb: (u8, u8, u8), weight: f64) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        loop {
            self.set(x, y, rgb, weight);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    /// A circle of `r` dots about (`cx`, `cy`), for horizons and rims.
    pub fn circle(&mut self, cx: f64, cy: f64, r: f64, rgb: (u8, u8, u8), weight: f64) {
        let steps = ((r * 6.0) as usize).clamp(180, 2400);
        for i in 0..steps {
            let a = i as f64 * std::f64::consts::TAU / steps as f64;
            self.set((cx + r * a.sin()) as i32, (cy + r * a.cos()) as i32, rgb, weight);
        }
    }

    /// The canvas as one printable frame, its top left cell at
    /// (`left`, `top`). Colour is switched only when it changes, so a run
    /// of same-coloured stars costs one escape sequence, not one per cell.
    pub fn frame(&self, left: u16, top: u16) -> String {
        let mut out = String::with_capacity(self.w * self.h * 3);
        for y in 0..self.h {
            out.push_str(&Cursor::at(left, top + y as u16));
            let mut cur: Option<(u8, u8, u8)> = None;
            for x in 0..self.w {
                let i = y * self.w + x;
                let b = self.bits[i];
                if b == 0 {
                    out.push(' ');
                    continue;
                }
                if self.color[i] != cur {
                    if let Some((r, g, bl)) = self.color[i] {
                        out.push_str(&style::set_fg_rgb(r, g, bl));
                        cur = self.color[i];
                    }
                }
                out.push(char::from_u32(0x2800 + b as u32).unwrap_or(' '));
            }
            out.push_str(style::RESET);
        }
        out
    }
}
