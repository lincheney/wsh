use unicode_width::UnicodeWidthStr;
use std::io::{Result, Write};
use crossterm::{
    queue,
    cursor::{MoveUp, MoveDown, MoveToColumn},
    terminal::{Clear, ClearType},
    style::{
        Print,
        SetAttribute,
        Attribute as CAttribute,
        SetColors,
        ResetColor,
        Colors,
        SetUnderlineColor,
        Color,
    },
};
use super::{Cell, Style, Modifier};

pub trait Canvas {
    fn get_cell(&self, pos: (u16, u16)) -> &Cell;
    fn get_cell_mut(&mut self, pos: (u16, u16)) -> &mut Cell;
    fn set_cell(&mut self, pos: (u16, u16), cell: &Cell);
    fn get_cell_range(&self, start: (u16, u16), end: (u16, u16)) -> &[Cell];
    fn get_cell_range_mut(&mut self, start: (u16, u16), end: (u16, u16)) -> &mut [Cell];
    fn get_size(&self) -> (u16, u16);
}

#[derive(Default)]
pub struct DummyCanvas {
    pub size: (u16, u16),
    cell: Cell,
}
impl Canvas for DummyCanvas {
    fn get_cell(&self, _pos: (u16, u16)) -> &Cell {
        &self.cell
    }

    fn get_cell_mut(&mut self, _pos: (u16, u16)) -> &mut Cell {
        &mut self.cell
    }

    fn set_cell(&mut self, _pos: (u16, u16), _cell: &Cell) {
    }

    fn get_cell_range(&self, _start: (u16, u16), _end: (u16, u16)) -> &[Cell] {
        &[]
    }

    fn get_cell_range_mut(&mut self, _start: (u16, u16), _end: (u16, u16)) -> &mut [Cell] {
        &mut []
    }

    fn get_size(&self) -> (u16, u16) {
        self.size
    }
}

pub struct Drawer<'a, 'b, W, C> {
    canvas: &'a mut C,
    pub writer: &'b mut W,
    real_pos: (u16, u16),
    pos: (u16, u16),
    fg: Color,
    bg: Color,
    underline_color: Color,
    modifier: Modifier,
}

impl<'a, 'b, W, C> Drawer<'a, 'b, W, C> {
    pub fn new(canvas: &'a mut C, writer: &'b mut W, pos: (u16, u16)) -> Self {
        Self {
            canvas,
            writer,
            real_pos: pos,
            pos,
            fg: Color::Reset,
            bg: Color::Reset,
            underline_color: Color::Reset,
            modifier: Modifier::default(),
        }
    }

    pub fn set_pos(&mut self, pos: (u16, u16)) {
        self.pos = pos;
        self.real_pos = pos;
    }

    pub fn get_pos(&mut self) -> (u16, u16) {
        self.pos
    }

    pub fn move_to(&mut self, pos: (u16, u16)) {
        self.pos = pos;
    }
}

impl<W, C: Canvas> Drawer<'_, '_, W, C> {
    pub fn term_height(&self) -> u16 {
        self.canvas.get_size().1
    }

    pub fn term_width(&self) -> u16 {
        self.canvas.get_size().0
    }

    pub fn validate_pos(&self, pos: (u16, u16)) -> bool {
        let size = self.canvas.get_size();
        pos.0 <= size.0 && pos.1 < size.1
    }

    pub fn try_move_to(&mut self, pos: (u16, u16)) -> bool {
        if self.validate_pos(pos) {
            self.pos = pos;
            true
        } else {
            false
        }
    }
}

impl<W: Write, C: Canvas> Drawer<'_, '_, W, C> {
    pub fn move_to_pos(&mut self, pos: (u16, u16)) -> Result<()> {
        if pos == (0, self.real_pos.1 + 1) {
            if self.real_pos.0 == self.term_width() {
                // 1 past the end of line is technically the same?
                // we need to do it this way to trigger terminal line wrapping
            } else {
                queue!(self.writer, Print("\r\n"))?;
            }
        } else {
            if pos.0 != self.real_pos.0 {
                queue!(self.writer, MoveToColumn(pos.0))?;
            }
            if pos.1 > self.real_pos.1 {
                queue!(self.writer, MoveDown(pos.1 - self.real_pos.1))?;
            } else if pos.1 < self.real_pos.1 {
                queue!(self.writer, MoveUp(self.real_pos.1 - pos.1))?;
            }
        }

        self.pos = pos;
        self.real_pos = pos;
        Ok(())
    }

    pub fn move_to_cur_pos(&mut self) -> Result<()> {
        if self.pos != self.real_pos {
            if self.pos.0 < self.term_width() {
                self.move_to_pos(self.pos)?;
            } else {
                // oh tricky
                // in order to get back to the very very edge of the screen, we have to reprint the
                // char just before
                // TODO what about when it is a multi width char
                let pos = (self.term_width() - 1, self.pos.1);
                self.move_to_pos(pos)?;
                let cell = self.canvas.get_cell(pos).clone();
                self.draw_cell(&cell, true)?;
            }
        }
        Ok(())
    }

    pub fn allocate_height(&mut self, height: u16) -> Result<()> {
        self.move_to_cur_pos()?;
        super::allocate_height(self.writer, height)?;
        Ok(())
    }

    pub fn clear_cells(&mut self, (x, y): (u16, u16), n: u16) {
        for i in 0..n {
            self.canvas.get_cell_mut((x + i, y)).reset();
        }
    }

    pub fn reset_colours(&mut self) -> Result<()> {
        self.fg = Color::Reset;
        self.bg = Color::Reset;
        self.underline_color = Color::Reset;
        self.modifier = Modifier::default();
        queue!(self.writer, ResetColor)
    }

    fn do_clear(&mut self, clear: ClearType, cell: Option<&Cell>) -> Result<()> {
        if let Some(cell) = cell {
            self.print_style_of_cell(cell)?;
        } else {
            self.reset_colours()?;
        }
        queue!(self.writer, Clear(clear))
    }

    pub fn goto_newline(&mut self, cell: Option<&Cell>) -> Result<()> {
        self.clear_to_end_of_line(cell)?;
        self.pos = (0, self.pos.1 + 1);
        Ok(())
    }

    fn cells_are_cleared(cells: &[Cell], style: Option<Style>) -> bool {
        if let Some(style) = style {
            cells.iter().all(|c| c.text() == " " && c.style == style)
        } else {
            cells.iter().all(super::cell_is_empty)
        }
    }

    pub fn clear_to_end_of_line(&mut self, cell: Option<&Cell>) -> Result<()> {
        // clear the rest of this line
        let width = self.term_width();
        if self.pos.0 < width {

            let cells = self.canvas.get_cell_range_mut(self.pos, (width, self.pos.1));

            let style = cell.map(|c| c.style);
            if !Self::cells_are_cleared(cells, style) {
                for c in cells.iter_mut() {
                    c.reset();
                }
                if let Some(style) = style {
                    for c in cells.iter_mut() {
                        c.style = style;
                    }
                }
                self.move_to_cur_pos()?;
                self.do_clear(ClearType::UntilNewLine, cell)?;
            }

        }
        Ok(())
    }

    pub fn clear_to_end_of_screen(&mut self, cell: Option<&Cell>) -> Result<()> {
        // clear everything from cursor onwards
        let width = self.term_width();
        let height = self.term_height();
        let cells = if self.pos.0 < width {
            self.canvas.get_cell_range_mut(self.pos, (0, height))
        } else if self.pos.1 + 1 < height {
            self.canvas.get_cell_range_mut((0, self.pos.1 + 1), (0, height))
        } else {
            // we already at bottom of screen
            return Ok(())
        };

        let style = cell.map(|c| c.style);
        if !Self::cells_are_cleared(cells, style) {
            for c in cells.iter_mut() {
                c.reset();
            }
            if let Some(style) = style {
                for c in cells.iter_mut() {
                    c.style = style;
                }
            }
            self.move_to_cur_pos()?;
            self.do_clear(ClearType::FromCursorDown, cell)?;
        }
        Ok(())
    }

    pub fn write_raw(&mut self, data: &[u8], pos: Option<(u16, u16)>) -> Result<()> {
        self.move_to_cur_pos()?;
        self.writer.write_all(data)?;
        if let Some(pos) = pos {
            self.set_pos(pos);
        }
        Ok(())
    }

    pub fn draw_cell_n_times(&mut self, cell: &Cell, force: bool, n: u16) -> Result<()> {
        if self.pos.0 + n == self.term_width() {
            // just clear
            self.clear_to_end_of_line(Some(cell))?;
            self.move_to((self.pos.0 + n, self.pos.1));
        } else {
            for _ in 0 .. n {
                self.draw_cell(cell, force)?;
            }
        }
        Ok(())
    }

    // pub fn draw_cells(&mut self, cells: &[Cell], force: bool) -> Result<()> {
        // for cell in cells {
            // self.draw_cell(cell, force)?;
        // }
        // Ok(())
    // }

    pub fn draw_cell(&mut self, cell: &Cell, force: bool) -> Result<()> {
        let cell_width = cell.text().width() as u16;
        let will_wrap = self.pos.0 + cell_width > self.term_width();

        let mut pos = self.pos;
        if will_wrap {
            // not actually enough space to fit this char
            self.clear_cells(pos, self.term_width() - pos.0);
            // wrap to next line
            pos = (0, pos.1 + 1);
        }

        let draw = force || will_wrap || self.canvas.get_cell(pos) != cell;
        if draw {
            // move to the location
            self.move_to_cur_pos()?;
            self.print_cell(cell)?;
            self.canvas.set_cell(pos, cell);
        }

        // clear the remaining cells
        self.clear_cells((pos.0 + 1, pos.1), cell_width - 1);
        pos.0 += cell_width;

        self.pos = pos;
        if draw {
            self.real_pos = pos;
        }

        Ok(())
    }

    pub fn print_style_of_cell(&mut self, cell: &Cell) -> Result<()> {
        let cell_modifier = cell.style.modifier;
        let cell_fg = cell.style.fg.unwrap_or(Color::Reset);
        let cell_bg = cell.style.bg.unwrap_or(Color::Reset);
        let cell_uc = cell.style.underline_color.unwrap_or(Color::Reset);

        if cell_modifier != self.modifier {
            self.draw_modifier(cell_modifier)?;
        }
        if cell_fg != self.fg || cell_bg != self.bg {
            queue!(self.writer, SetColors(Colors::new(
                cell_fg,
                cell_bg,
            )))?;
            self.fg = cell_fg;
            self.bg = cell_bg;
        }
        if cell_uc != self.underline_color {
            queue!(self.writer, SetUnderlineColor(cell_uc))?;
            self.underline_color = cell_uc;
        }
        Ok(())
    }

    pub fn print_cell(&mut self, cell: &Cell) -> Result<()> {
        self.print_style_of_cell(cell)?;
        queue!(self.writer, Print(cell.text()))?;
        Ok(())
    }

    fn draw_modifier(&mut self, new: Modifier) -> Result<()> {
        let removed = self.modifier - new;
        if removed.contains(Modifier::REVERSED) {
            queue!(self.writer, SetAttribute(CAttribute::NoReverse))?;
        }
        if removed.contains(Modifier::BOLD) {
            queue!(self.writer, SetAttribute(CAttribute::NormalIntensity))?;
            if new.contains(Modifier::DIM) {
                queue!(self.writer, SetAttribute(CAttribute::Dim))?;
            }
        }
        if removed.contains(Modifier::ITALIC) {
            queue!(self.writer, SetAttribute(CAttribute::NoItalic))?;
        }
        if removed.contains(Modifier::UNDERLINED) {
            queue!(self.writer, SetAttribute(CAttribute::NoUnderline))?;
        }
        if removed.contains(Modifier::DIM) {
            queue!(self.writer, SetAttribute(CAttribute::NormalIntensity))?;
        }
        if removed.contains(Modifier::CROSSED_OUT) {
            queue!(self.writer, SetAttribute(CAttribute::NotCrossedOut))?;
        }
        if removed.contains(Modifier::SLOW_BLINK) || removed.contains(Modifier::RAPID_BLINK) {
            queue!(self.writer, SetAttribute(CAttribute::NoBlink))?;
        }

        let added = new - self.modifier;
        if added.contains(Modifier::REVERSED) {
            queue!(self.writer, SetAttribute(CAttribute::Reverse))?;
        }
        if added.contains(Modifier::BOLD) {
            queue!(self.writer, SetAttribute(CAttribute::Bold))?;
        }
        if added.contains(Modifier::ITALIC) {
            queue!(self.writer, SetAttribute(CAttribute::Italic))?;
        }
        if added.contains(Modifier::UNDERLINED) {
            queue!(self.writer, SetAttribute(CAttribute::Underlined))?;
        }
        if added.contains(Modifier::DIM) {
            queue!(self.writer, SetAttribute(CAttribute::Dim))?;
        }
        if added.contains(Modifier::CROSSED_OUT) {
            queue!(self.writer, SetAttribute(CAttribute::CrossedOut))?;
        }
        if added.contains(Modifier::SLOW_BLINK) {
            queue!(self.writer, SetAttribute(CAttribute::SlowBlink))?;
        }
        if added.contains(Modifier::RAPID_BLINK) {
            queue!(self.writer, SetAttribute(CAttribute::RapidBlink))?;
        }

        self.modifier = new;
        Ok(())
    }
}
