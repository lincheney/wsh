use std::rc::Rc;
pub use crossterm::style::Color;

bitflags::bitflags! {
    #[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Modifier: u16 {
        const BOLD        = 1 << 0;
        const DIM         = 1 << 1;
        const ITALIC      = 1 << 2;
        const UNDERLINED  = 1 << 3;
        const SLOW_BLINK  = 1 << 4;
        const RAPID_BLINK = 1 << 5;
        const REVERSED    = 1 << 6;
        const HIDDEN      = 1 << 7;
        const CROSSED_OUT = 1 << 8;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hyperlink {
    pub url: Rc<str>,
    pub id: Option<Rc<str>>,
}

/// A style description where `None` fields and unset modifier bits mean "not explicitly set".
///
/// Two styles are merged with `patch`: the other style's explicitly-set fields override self.
/// `modifier_mask` tracks which modifier bits were explicitly set; `modifier` holds their values.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    pub fg:              Option<Color>,
    pub bg:              Option<Color>,
    pub underline_color: Option<Color>,
    pub hyperlink:       Option<Rc<Hyperlink>>,
    /// actual values of modifier bits
    pub modifier:        Modifier,
    /// which modifier bits are explicitly set
    pub modifier_mask:   Modifier,
}

impl Style {
    pub const fn new() -> Self {
        Self {
            fg: None,
            bg: None,
            underline_color: None,
            hyperlink: None,
            modifier: Modifier::empty(),
            modifier_mask: Modifier::empty(),
        }
    }

    /// Merge `other` on top of `self`. Fields explicitly set in `other` override `self`.
    pub fn patch(self, other: Style) -> Style {
        Style {
            fg: other.fg.or(self.fg),
            bg: other.bg.or(self.bg),
            underline_color: other.underline_color.or(self.underline_color),
            hyperlink: other.hyperlink.or(self.hyperlink),
            modifier: (!other.modifier_mask & self.modifier) | (other.modifier_mask & other.modifier),
            modifier_mask: self.modifier_mask.union(other.modifier_mask),
        }
    }

    pub const fn add_modifier(mut self, m: Modifier) -> Self {
        self.modifier = self.modifier.union(m);
        self.modifier_mask = self.modifier_mask.union(m);
        self
    }

    pub const fn remove_modifier(mut self, m: Modifier) -> Self {
        self.modifier = self.modifier.difference(m);
        self.modifier_mask = self.modifier_mask.union(m);
        self
    }

    pub const fn has_modifier(&self, m: Modifier) -> bool {
        self.modifier_mask.contains(m) && self.modifier.contains(m)
    }

    pub const fn fg(mut self, c: Color) -> Self {
        self.fg = Some(c);
        self
    }

    pub const fn bg(mut self, c: Color) -> Self {
        self.bg = Some(c);
        self
    }

    pub const fn underline_color(mut self, c: Color) -> Self {
        self.underline_color = Some(c);
        self
    }

    pub fn hyperlink(mut self, url: Option<Rc<Hyperlink>>) -> Self {
        self.hyperlink = url;
        self
    }
}
