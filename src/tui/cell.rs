use unicode_width::{UnicodeWidthStr};
use byteyarn::Yarn;
use super::style::Style;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    text: Yarn,
    pub style: Style,
}

impl Default for Cell {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Cell {
    pub const EMPTY: Cell = Cell {
        text: Yarn::new(" "),
        style:  Style::new(),
    };

    pub fn new(text: &str) -> Self {
        Self::new_with_style(text, Style::new())
    }

    pub fn new_with_style(text: &str, style: Style) -> Self {
        Self {
            text: Yarn::copy(text),
            style,
        }
    }

    pub fn width(&self) -> usize {
        self.text().width()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, s: &str) {
        self.text = Yarn::copy(s);
    }

    pub fn reset(&mut self) {
        *self = Self::EMPTY;
    }
}
