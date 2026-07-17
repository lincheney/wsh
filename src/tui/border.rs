use std::ops::ControlFlow;
use std::borrow::Cow;
use std::io::Write;
use bstr::{ByteSlice};
use super::cell::Cell;
use super::style::Style;
use super::text::{Text, Alignment};
use super::drawer::{Drawer, Canvas};

bitflags::bitflags! {
    #[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
    pub struct Sides: u8 {
        const TOP    = 1 << 0;
        const RIGHT  = 1 << 1;
        const BOTTOM = 1 << 2;
        const LEFT   = 1 << 3;
        const ALL    = Self::TOP.bits() | Self::RIGHT.bits() | Self::BOTTOM.bits() | Self::LEFT.bits();
    }
}

#[derive(Debug, Clone, Copy, Default, strum::EnumString)]
pub enum Kind {
    #[default]
    Plain,
    Rounded,
    Double,
    Thick,
}

struct Chars<'a> {
    top_left:     &'a str,
    top_right:    &'a str,
    bottom_left:  &'a str,
    bottom_right: &'a str,
    horizontal:   &'a str,
    vertical:     &'a str,
}

impl From<Kind> for Chars<'static> {
    fn from(kind: Kind) -> Self {
        match kind {
            Kind::Plain =>   Chars{ top_left: "┌", top_right: "┐", bottom_left: "└", bottom_right: "┘", horizontal: "─", vertical: "│" },
            Kind::Rounded => Chars{ top_left: "╭", top_right: "╮", bottom_left: "╰", bottom_right: "╯", horizontal: "─", vertical: "│" },
            Kind::Double =>  Chars{ top_left: "╔", top_right: "╗", bottom_left: "╚", bottom_right: "╝", horizontal: "═", vertical: "║" },
            Kind::Thick =>   Chars{ top_left: "┏", top_right: "┓", bottom_left: "┗", bottom_right: "┛", horizontal: "━", vertical: "┃" },
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Title {
    pub text: Text<()>,
    pub alignment: Alignment,
}

impl Title {
    pub fn new(text: Text<()>, alignment: Alignment) -> Self {
        Self { text, alignment }
    }

    fn make_cells(&self, width: usize) -> Option<Vec<Cell>> {
        let first_line = self.text.get().first()?;
        let first_line = first_line.lines().next()?;

        let highlights = self.text.highlights.iter();
        let mut cells = vec![];
        super::wrap::wrap(first_line.into(), highlights, Some(&self.text.style), width - 2, 0, Some(|_, token: super::wrap::WrapToken, _wrapped_no, _lineno, style| {
            if let Some(string) = token.as_str() {
                let mut cell = Cell::new(string);
                if let Some(style) = style {
                    cell.style = style;
                }
                cells.push(cell);
            }
            ControlFlow::Break(())
        }));
        Some(cells)
    }

    fn get_sizes(&self, cells: &[Cell], width: usize) -> (usize, usize) {
        let title_len: usize = cells.iter().map(|c| c.width()).sum();
        let clamped = title_len.min(width);
        let spare = width.saturating_sub(clamped);
        let x_offset = match self.alignment {
            Alignment::Left   => 0,
            Alignment::Center => spare / 2,
            Alignment::Right  => spare,
        };
        (title_len, x_offset)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Border {
    pub sides: Sides,
    pub kind: Kind,
    pub style: Style,
    pub title_top:    Option<Title>,
    pub title_bottom: Option<Title>,
}

impl Border {
    /// Returns the number of columns consumed by left and right borders.
    pub fn inner_width(&self, width: u16) -> u16 {
        let left  = if self.has_left()  { 1 } else { 0 };
        let right = if self.has_right() { 1 } else { 0 };
        let left  = left.min(width);
        let right = right.min(width.saturating_sub(left));
        left + right
    }

    /// Returns how many rows the top and bottom borders consume.
    pub fn inner_height(&self) -> u16 {
          (if self.has_top()    { 1 } else { 0 })
        + (if self.has_bottom() { 1 } else { 0 })
    }

    pub fn has_top(&self)    -> bool { self.sides.contains(Sides::TOP) }
    pub fn has_bottom(&self) -> bool { self.sides.contains(Sides::BOTTOM) }
    pub fn has_left(&self)   -> bool { self.sides.contains(Sides::LEFT) }
    pub fn has_right(&self)  -> bool { self.sides.contains(Sides::RIGHT) }

    pub fn clear(&mut self) {
        self.sides = Sides::empty();
    }

    pub fn render_top<W: Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
        width:  u16,
    ) -> std::io::Result<()> {
        if !self.has_top() {
            return Ok(())
        }

        let chars = Chars::from(self.kind);
        let cell = Cell::new_with_style(chars.horizontal, self.style.clone());
        let left = if self.has_left() {
            Cow::Owned(Cell::new_with_style(chars.top_left, self.style.clone()))
        } else {
            Cow::Borrowed(&cell)
        };
        let right = if self.has_right() {
            Cow::Owned(Cell::new_with_style(chars.top_right, self.style.clone()))
        } else {
            Cow::Borrowed(&cell)
        };
        render_row(
            drawer,
            width,
            &left,
            &right,
            &cell,
            self.title_top.as_ref(),
        )
    }

    pub fn render_bottom<W: Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
        width:  u16,
    ) -> std::io::Result<()> {
        if !self.has_bottom() {
            return Ok(())
        }

        let chars = Chars::from(self.kind);
        let cell = Cell::new_with_style(chars.horizontal, self.style.clone());
        let left = if self.has_left() {
            Cow::Owned(Cell::new_with_style(chars.bottom_left, self.style.clone()))
        } else {
            Cow::Borrowed(&cell)
        };
        let right = if self.has_right() {
            Cow::Owned(Cell::new_with_style(chars.bottom_right, self.style.clone()))
        } else {
            Cow::Borrowed(&cell)
        };
        render_row(
            drawer,
            width,
            &left,
            &right,
            &cell,
            self.title_bottom.as_ref(),
        )
    }

    pub fn render_left<W: Write, C: Canvas>(&self, drawer: &mut Drawer<W, C>) -> std::io::Result<()> {
        if self.has_left() {
            let chars = Chars::from(self.kind);
            let cell = Cell::new_with_style(chars.vertical, self.style.clone());
            drawer.draw_cell(&cell, false)
        } else {
            Ok(())
        }
    }

    pub fn render_right<W: Write, C: Canvas>(&self, drawer: &mut Drawer<W, C>) -> std::io::Result<()> {
        if self.has_right() {
            let chars = Chars::from(self.kind);
            let cell = Cell::new_with_style(chars.vertical, self.style.clone());
            drawer.draw_cell(&cell, false)
        } else {
            Ok(())
        }
    }
}

fn render_row<W: Write, C: Canvas>(
    drawer: &mut Drawer<W, C>,
    width:  u16,
    left:   &Cell,
    right:  &Cell,
    fill:   &Cell,
    title:  Option<&Title>,
) -> std::io::Result<()> {

    if width == 0 {
        return Ok(());
    }

    drawer.draw_cell(left, false)?;

    // title overlay
    if width >= 3
        && let Some(title) = title
        && let Some(title_cells) = title.make_cells(width as usize - 2)
    {
        let (title_width, x_offset) = title.get_sizes(&title_cells, width as usize - 2);
        for _ in 0..x_offset {
            drawer.draw_cell(fill, false)?;
        }
        for c in &title_cells {
            drawer.draw_cell(c, false)?;
        }
        for _ in 1 + x_offset + title_width .. width as usize - 1 {
            drawer.draw_cell(fill, false)?;
        }
    } else {
        for _ in 1 .. width - 1 {
            drawer.draw_cell(fill, false)?;
        }
    }

    if width > 1 {
        drawer.draw_cell(right, false)?;
    }
    Ok(())
}
