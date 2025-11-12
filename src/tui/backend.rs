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
        Color as CColor,
        Colors,
        SetForegroundColor,
        SetBackgroundColor,
        SetUnderlineColor,
    },
};
use ratatui::{
    style::{Color, Modifier},
    buffer::{Cell, Buffer},
};

pub struct Drawer<'a, 'b, W: Write> {
    buffer: &'a mut Buffer,
    pub writer: &'b mut W,
    last_pos: (u16, u16),
    pub cur_pos: (u16, u16),
    fg: Color,
    bg: Color,
    underline_color: Color,
    modifier: Modifier,
}

impl<'a, 'b, W: Write> Drawer<'a, 'b, W> {
    pub fn new(buffer: &'a mut Buffer, writer: &'b mut W, pos: (u16, u16)) -> Self {
        Self {
            buffer,
            writer,
            last_pos: pos,
            cur_pos: pos,
            fg: Color::default(),
            bg: Color::default(),
            underline_color: Color::default(),
            modifier: Modifier::default(),
        }
    }

    fn term_width(&self) -> u16 {
        self.buffer.area.width
    }

    pub fn set_pos(&mut self, pos: (u16, u16)) {
        self.cur_pos = pos;
        self.last_pos = pos;
    }

    pub fn move_to_pos(&mut self, pos: (u16, u16)) -> Result<()> {
        if pos.0 != self.last_pos.0 {
            queue!(self.writer, MoveToColumn(pos.0))?;
        }
        if pos.1 > self.last_pos.1 {
            queue!(self.writer, MoveDown(pos.1 - self.last_pos.1))?;
        } else if pos.1 < self.last_pos.1 {
            queue!(self.writer, MoveUp(self.last_pos.1 - pos.1))?;
        }
        self.cur_pos = pos;
        self.last_pos = pos;
        Ok(())
    }

    pub fn move_to_cur_pos(&mut self) -> Result<()> {
        if self.cur_pos != self.last_pos {
            if self.cur_pos.0 < self.term_width() {
                self.move_to_pos(self.cur_pos)?;
            } else {
                // oh tricky
                // in order to get back to the very very edge of the screen, we have to reprint the
                // char just before
                // TODO what about when it is a multi width char
                let pos = (self.term_width() - 1, self.cur_pos.1);
                self.move_to_pos(pos)?;
                let cell = self.buffer[pos].clone();
                self.draw_cell(&cell, true)?;
            }
        }
        Ok(())
    }

    pub fn clear_cells(&mut self, (x, y): (u16, u16), n: u16) {
        for i in 0..n {
            self.buffer[(x + i, y)].reset();
        }
    }

    pub fn reset_colours(&mut self) -> Result<()> {
        queue!(
            self.writer,
            SetForegroundColor(CColor::Reset),
            SetBackgroundColor(CColor::Reset),
            SetUnderlineColor(CColor::Reset),
            SetAttribute(CAttribute::Reset),
        )
    }

    pub fn goto_newline(&mut self) -> Result<()> {
        self.clear_to_end_of_line()?;
        self.cur_pos = (0, self.cur_pos.1 + 1);
        Ok(())
    }

    pub fn clear_to_end_of_line(&mut self) -> Result<()> {
        // clear the rest of this line
        let width = self.term_width();
        if self.cur_pos.0 < width {

            let i = self.buffer.index_of(self.cur_pos.0, self.cur_pos.1);
            let cells = &mut self.buffer.content[i..i + (width - self.cur_pos.0) as usize];

            if !cells.iter().all(super::cell_is_empty) {
                for c in cells {
                    c.reset();
                }
                self.move_to_cur_pos()?;
                queue!(self.writer, Clear(ClearType::UntilNewLine))?;
            }

        }
        Ok(())
    }

    pub fn clear_to_end_of_screen(&mut self) -> Result<()> {
        // clear everything from cursor onwards
        let width = self.term_width();
        let i = if self.cur_pos.0 < width {
            self.buffer.index_of(self.cur_pos.0, self.cur_pos.1)
        } else if self.cur_pos.1 + 1 < self.buffer.area.height {
            self.buffer.index_of(0, self.cur_pos.1 + 1)
        } else {
            // we already at bottom of screen
            return Ok(())
        };
        let cells = &mut self.buffer.content[i..];

        if !cells.iter().all(super::cell_is_empty) {
            for c in cells {
                c.reset();
            }
            self.move_to_cur_pos()?;
            queue!(self.writer, Clear(ClearType::FromCursorDown))?;
        }
        Ok(())
    }

    pub fn draw_lines<'c, I: Iterator<Item=&'c [Cell]>>(&mut self, lines: I) -> Result<()> {
        for (i, line) in lines.enumerate() {
            if i > 0 {
                self.goto_newline()?;
            }
            let mut skip = 0;
            for cell in line {
                if skip == 0 {
                    skip = cell.symbol().width() - 1;
                    self.draw_cell(cell, false)?;
                } else {
                    skip -= 1;
                }
            }
        }
        self.clear_to_end_of_line()?;
        Ok(())
    }

    pub fn draw_cell(&mut self, cell: &Cell, force: bool) -> Result<()> {
        let cell_width = cell.symbol().width() as u16;
        let will_wrap = self.cur_pos.0 + cell_width > self.term_width();

        let mut cur_pos = self.cur_pos;
        if will_wrap {
            // not actually enough space to fit this char
            self.clear_cells(cur_pos, self.term_width() - cur_pos.0);
            // wrap to next line
            cur_pos = (0, cur_pos.1 + 1);
        }

        let draw = force || will_wrap || &self.buffer[cur_pos] != cell;
        if draw {
            // move to the location
            self.move_to_cur_pos()?;

            if cell.modifier != self.modifier {
                self.draw_modifier(cell.modifier)?;
            }
            if cell.fg != self.fg || cell.bg != self.bg {
                queue!(self.writer, SetColors(Colors::new(cell.fg.into(), cell.bg.into())))?;
                self.fg = cell.fg;
                self.bg = cell.bg;
            }
            if cell.underline_color != self.underline_color {
                let color = CColor::from(cell.underline_color);
                queue!(self.writer, SetUnderlineColor(color))?;
                self.underline_color = cell.underline_color;
            }

            queue!(self.writer, Print(cell.symbol()))?;
            self.buffer[cur_pos] = cell.clone();
        }

        cur_pos.0 += 1;
        // clear the remaining cells
        self.clear_cells(cur_pos, cell_width - 1);
        cur_pos.0 += cell_width - 1;

        self.cur_pos = cur_pos;
        if draw {
            self.last_pos = cur_pos;
        }

        Ok(())
    }

    fn draw_modifier(&mut self, new: Modifier) -> Result<()> {
        //use crossterm::Attribute;
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
