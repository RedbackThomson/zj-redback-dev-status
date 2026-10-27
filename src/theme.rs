use std::collections::BTreeMap;
use zellij_tile::prelude::{PaletteColor, Styling};

#[derive(Clone, Copy)]
pub enum Col {
    Rgb(u8, u8, u8),
    Bit(u8),
    Default,
}

impl Col {
    pub fn fg(self) -> String {
        match self {
            Col::Rgb(r, g, b) => format!("\x1b[38;2;{r};{g};{b}m"),
            Col::Bit(n) => format!("\x1b[38;5;{n}m"),
            Col::Default => "\x1b[39m".to_string(),
        }
    }

    pub fn bg(self) -> String {
        match self {
            Col::Rgb(r, g, b) => format!("\x1b[48;2;{r};{g};{b}m"),
            Col::Bit(n) => format!("\x1b[48;5;{n}m"),
            Col::Default => "\x1b[49m".to_string(),
        }
    }
}

impl From<PaletteColor> for Col {
    fn from(p: PaletteColor) -> Col {
        match p {
            PaletteColor::Rgb((r, g, b)) => Col::Rgb(r, g, b),
            PaletteColor::EightBit(n) => Col::Bit(n),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub text: Col,
    pub muted: Col,
    pub dim: Col,
    pub accent: Col,
    pub running: Col,
    pub error: Col,
    pub hl_bg: Col,
    pub title: Col,
}

impl Theme {
    /// Palette-indexed fallback used until Zellij sends the active theme; these
    /// indices follow the terminal emulator's own 16-color scheme.
    pub fn ansi() -> Theme {
        Theme {
            text: Col::Default,
            muted: Col::Bit(7),
            dim: Col::Bit(8),
            accent: Col::Bit(4),
            running: Col::Bit(2),
            error: Col::Bit(1),
            hl_bg: Col::Bit(236),
            title: Col::Bit(208),
        }
    }

    /// Derive colors from the user's active Zellij theme. Muted tones are the
    /// theme text blended toward its background; the current-session highlight
    /// is the background lifted slightly toward the text. Switching themes in
    /// config.kdl recolors the plugin live.
    pub fn from_styling(s: &Styling) -> Theme {
        let t = s.text_unselected;
        let base: Col = t.base.into();
        let bg: Col = t.background.into();
        Theme {
            text: base,
            muted: blend(base, bg, 0.40),
            dim: blend(base, bg, 0.62),
            accent: t.emphasis_0.into(),
            running: s.exit_code_success.base.into(),
            error: s.exit_code_error.base.into(),
            hl_bg: blend(bg, base, 0.14),
            // Zellij's theme exposes no orange role; a fixed peach, overridable
            // via color_title.
            title: Col::Rgb(239, 159, 118),
        }
    }

    /// Override roles from the plugin's KDL config without recompiling. Keys:
    /// color_text, color_muted, color_dim, color_accent, color_running,
    /// color_error, color_hl_bg, color_title.
    pub fn apply_overrides(&mut self, cfg: &BTreeMap<String, String>) {
        let set = |key: &str, slot: &mut Col| {
            if let Some(c) = cfg.get(key).and_then(|v| parse_color(v)) {
                *slot = c;
            }
        };
        set("color_text", &mut self.text);
        set("color_muted", &mut self.muted);
        set("color_dim", &mut self.dim);
        set("color_accent", &mut self.accent);
        set("color_running", &mut self.running);
        set("color_error", &mut self.error);
        set("color_hl_bg", &mut self.hl_bg);
        set("color_title", &mut self.title);
    }
}

fn blend(a: Col, b: Col, t: f32) -> Col {
    match (a, b) {
        (Col::Rgb(ar, ag, ab), Col::Rgb(br, bg, bb)) => {
            Col::Rgb(lerp(ar, br, t), lerp(ag, bg, t), lerp(ab, bb, t))
        }
        _ => a,
    }
}

fn lerp(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round().clamp(0.0, 255.0) as u8
}

/// Accepts "#rrggbb", a 0-255 8-bit index, or a basic ANSI color name
/// (optionally prefixed "bright").
fn parse_color(v: &str) -> Option<Col> {
    let v = v.trim();
    if let Some(hex) = v.strip_prefix('#') {
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Col::Rgb(r, g, b));
        }
        return None;
    }
    if let Ok(n) = v.parse::<u8>() {
        return Some(Col::Bit(n));
    }
    let (name, base) = match v.strip_prefix("bright ") {
        Some(rest) => (rest, 8u8),
        None => (v, 0u8),
    };
    let idx = match name {
        "black" => 0,
        "red" => 1,
        "green" => 2,
        "yellow" => 3,
        "blue" => 4,
        "magenta" => 5,
        "cyan" => 6,
        "white" => 7,
        "default" => return Some(Col::Default),
        _ => return None,
    };
    Some(Col::Bit(base + idx))
}
