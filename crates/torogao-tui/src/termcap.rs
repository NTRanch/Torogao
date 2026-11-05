//! Separable terminal-capability detection for the TUI.
//!
//! Adapted from the lumen-engine Stage 2 termcap approach: the
//! module names the terminal's richest image protocol and the
//! universal halfblock fallback without depending on the widget
//! layer. Detection is not rendering: the bake-off reports image
//! protocols as absent until an actual image path is integrated.

/// An image protocol a terminal may offer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Protocol {
    /// Kitty graphics protocol.
    Kitty,
    /// Sixel graphics.
    Sixel,
    /// iTerm2 inline images.
    Iterm2,
    /// Unicode halfblocks, the universal fallback.
    Halfblocks,
}

impl Protocol {
    /// Stable display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Kitty => "kitty",
            Self::Sixel => "sixel",
            Self::Iterm2 => "iterm2",
            Self::Halfblocks => "halfblocks",
        }
    }
}

/// The detected terminal profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalProfile {
    /// Richest detected image protocol.
    pub protocol: Protocol,
}

impl TerminalProfile {
    /// Detects a profile from environment values supplied by the
    /// caller, keeping this module free of environment mutation.
    pub fn detect(term: Option<&str>, term_program: Option<&str>) -> Self {
        let protocol = if contains(term, "kitty")
            || term_program == Some("kitty")
            || term_program == Some("ghostty")
        {
            Protocol::Kitty
        } else if contains(term, "foot") {
            Protocol::Sixel
        } else if term_program == Some("iTerm.app") {
            Protocol::Iterm2
        } else {
            Protocol::Halfblocks
        };
        Self { protocol }
    }

    /// Detects a profile from the current process environment.
    pub fn from_env() -> Self {
        Self::detect(
            std::env::var("TERM").ok().as_deref(),
            std::env::var("TERM_PROGRAM").ok().as_deref(),
        )
    }
}

fn contains(value: Option<&str>, needle: &str) -> bool {
    value.is_some_and(|text| text.contains(needle))
}

/// One halfblock cell: the upper-half glyph with the top pixel as
/// foreground and the bottom pixel as background.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HalfblockCell {
    /// The rendered glyph, always `▀`.
    pub glyph: char,
    /// Top pixel as an RGB triple.
    pub foreground: [u8; 3],
    /// Bottom pixel as an RGB triple.
    pub background: [u8; 3],
}

/// Composes two opaque pixels over the panel's base colour.
///
/// Transparent pixels use the base colour. This is the pure
/// composition rule from the separable termcap approach; the
/// bake-off uses it in tests only because the scenario carries
/// no image content.
pub fn compose_halfblock(top: [u8; 4], bottom: [u8; 4], base: [u8; 3]) -> HalfblockCell {
    HalfblockCell {
        glyph: '▀',
        foreground: composite(top, base),
        background: composite(bottom, base),
    }
}

fn composite(pixel: [u8; 4], base: [u8; 3]) -> [u8; 3] {
    let alpha = u16::from(pixel[3]);
    let mut output = [0_u8; 3];
    for channel in 0..3 {
        let value =
            (u16::from(pixel[channel]) * alpha + u16::from(base[channel]) * (255 - alpha)) / 255;
        output[channel] = u8::try_from(value).expect("an averaged channel fits u8");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_named_terminal_protocols() {
        assert_eq!(
            TerminalProfile::detect(Some("xterm-kitty"), None).protocol,
            Protocol::Kitty
        );
        assert_eq!(
            TerminalProfile::detect(Some("foot"), None).protocol,
            Protocol::Sixel
        );
        assert_eq!(
            TerminalProfile::detect(Some("xterm"), Some("iTerm.app")).protocol,
            Protocol::Iterm2
        );
        assert_eq!(
            TerminalProfile::detect(Some("screen-256color"), None).protocol,
            Protocol::Halfblocks
        );
    }

    #[test]
    fn halfblocks_compose_transparency_over_the_base() {
        let cell = compose_halfblock([255, 0, 0, 255], [0, 0, 0, 0], [24, 24, 28]);
        assert_eq!(cell.glyph, '▀');
        assert_eq!(cell.foreground, [255, 0, 0]);
        assert_eq!(cell.background, [24, 24, 28]);
    }
}
