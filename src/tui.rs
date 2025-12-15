use bstr::BString;
use std::default::Default;
use std::io::{Write, Cursor};
use anyhow::Result;
use crossterm::{
    cursor,
    queue,
    execute,
    style,
    terminal::{Clear, ClearType},
};
use ratatui::{
    *,
    layout::*,
    style::*,
    buffer::Buffer,
};
mod backend;
pub mod widget;
pub mod status_bar;
pub mod ansi;
pub mod text;
pub use backend::{Drawer};

pub struct MoveUp(pub u16);
impl crossterm::Command for MoveUp {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        if self.0 > 0 {
            cursor::MoveUp(self.0).write_ansi(f)
        } else {
            Ok(())
        }
    }
}

pub struct MoveDown(pub u16);
impl crossterm::Command for MoveDown {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        if self.0 > 0 {
            cursor::MoveDown(self.0).write_ansi(f)
        } else {
            Ok(())
        }
    }
}

pub fn allocate_height<W: Write>(stdout: &mut W, height: u16) -> Result<()> {
    for _ in 0 .. height {
        // vertical tab, this doesn't change x
        queue!(stdout, style::Print("\x0b"))?;
    }
    queue!(stdout, MoveUp(height))?;
    Ok(())
}

fn cell_is_empty(cell: &ratatui::buffer::Cell) -> bool {
    cell.symbol() == " " && cell.bg == Color::Reset && !cell.modifier.intersects(Modifier::UNDERLINED | Modifier::REVERSED)
}

fn buffer_nonempty_height(buffer: &Buffer) -> u16 {
    let trailing_empty_lines = buffer.content()
        .chunks(buffer.area.width as _)
        .rev()
        .take_while(|line| line.iter().all(cell_is_empty))
        .count();
    buffer.area.height - trailing_empty_lines as u16
}

fn render_indent(area: Rect, buffer: &mut Buffer, line_width: u16, alignment: Alignment, style: Option<Style>) -> u16 {
    let indent = match alignment {
        Alignment::Left => return 0,
        Alignment::Right => area.width.saturating_sub(line_width),
        Alignment::Center => area.width.saturating_sub(line_width) / 2,
    };

    let index = buffer.index_of(area.x, area.y);
    for cell in &mut buffer.content[index .. index + indent as usize] {
        cell.reset();
        if let Some(style) = style {
            cell.set_style(style);
        }
    }
    indent
}

#[derive(Debug)]
pub enum WidgetWrapper {
    Widget(widget::Widget),
    Ansi(ansi::Parser),
}

impl WidgetWrapper {
    pub fn as_ref(&self) -> &widget::Widget {
        match self {
            Self::Widget(w) => w,
            Self::Ansi(p) => &p.widget,
        }
    }

    pub fn as_mut(&mut self) -> &mut widget::Widget {
        match self {
            Self::Widget(w) => w,
            Self::Ansi(p) => p.as_widget(),
        }
    }
}

impl From<widget::Widget> for WidgetWrapper {
    fn from(widget: widget::Widget) -> Self {
        Self::Widget(widget)
    }
}

impl From<ansi::Parser> for WidgetWrapper {
    fn from(parser: ansi::Parser) -> Self {
        Self::Ansi(parser)
    }
}

#[derive(Default)]
struct Widgets {
    inner: Vec<WidgetWrapper>,
    pub height: u16,
    max_height: u16,
}

impl Widgets {
    fn get_height(&self) -> u16 {
        self.inner.iter().map(|w| w.as_ref().line_count).sum()
    }

    fn refresh(&mut self, area: Rect) {
        self.max_height = area.height;

        let mut height = 0;
        for w in &mut self.inner {
            let w = w.as_mut();
            if w.hidden || self.max_height <= height as _ {
                w.line_count = 0;
            } else {
                w.line_count = w.get_height_for_width(area);
                height += w.line_count;
            }
        }
    }

    fn render<W :Write>(
        &self,
        drawer: &mut Drawer<W>,
        buffer: &mut Buffer,
        area: Rect,
    ) -> std::io::Result<()> {

        let widgets = self.inner.iter().map(|w| w.as_ref()).filter(|w| !w.hidden && w.line_count > 0);
        let layout = Layout::vertical(widgets.clone().map(|w| w.constraint.unwrap_or(Constraint::Max(w.line_count))));
        let layouts = layout.split(area);

        for (widget, area) in widgets.zip(layouts.iter()) {
            widget.render(drawer, buffer, Some(area.height as usize))?;
        }
        Ok(())
    }
}

#[derive(Default)]
pub struct Tui {
    counter: usize,
    widgets: Widgets,
    buffer: Buffer,
    border_buffer: Buffer,
    prev_status_bar_position: usize,
    pub dirty: bool,
}

impl Tui {

    pub fn add(&mut self, mut widget: WidgetWrapper) -> (usize, &mut WidgetWrapper) {
        let id = self.counter;
        widget.as_mut().id = id;
        self.counter += 1;
        self.dirty = true;
        self.widgets.inner.push(widget);
        (id, self.widgets.inner.last_mut().unwrap())
    }

    pub fn add_message(&mut self, message: String) -> (usize, &mut WidgetWrapper) {
        let mut widget = widget::Widget::default();
        widget.inner.clear();
        for line in message.split('\n') {
            widget.inner.push_line(line.into(), None);
        }
        self.add(widget.into())
    }

    pub fn add_error_message(&mut self, message: String) -> (usize, &mut WidgetWrapper) {
        let mut widget = widget::Widget::default();
        widget.inner.clear();
        for line in message.split('\n') {
            widget.inner.push_line(line.into(), Some(text::Highlight{
                style: Style::new().fg(Color::Red),
                namespace: (),
                blend: true,
            }));
        }
        self.add(widget.into())
    }

    pub fn add_zle_message(&mut self, message: &[u8]) -> (usize, &mut WidgetWrapper) {
        let mut parser = ansi::Parser::default();
        parser.ocrnl = true; // treat \r as \n
        parser.feed(message.into());
        self.add(parser.widget.into())
    }

    pub fn get_index(&self, id: usize) -> Option<usize> {
        for (i, w) in self.widgets.inner.iter().enumerate() {
            match w.as_ref().id.cmp(&id) {
                std::cmp::Ordering::Equal => return Some(i),
                std::cmp::Ordering::Greater => break,
                std::cmp::Ordering::Less => (),
            }
        }
        None
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut WidgetWrapper> {
        self.get_index(id).map(|i| {
            self.dirty = true;
            &mut self.widgets.inner[i]
        })
    }

    pub fn remove(&mut self, id: usize) -> Option<WidgetWrapper> {
        self.get_index(id).map(|i| {
            self.dirty = true;
            self.widgets.inner.remove(i)
        })
    }

    pub fn render_to_string(&self, id: usize, width: Option<u16>) -> Option<BString> {
        self.get_index(id).map(|i| {
            let widget = self.widgets.inner[i].as_ref();

            let mut string = vec![];
            let mut writer = Cursor::new(&mut string);
            // always start with a reset
            writer.write_all(b"\x1b[0m").unwrap();
            let mut buffer = Buffer::default();
            let mut drawer = backend::Drawer::new(&mut buffer, &mut writer, (0, 0));
            // 3 lines in case you have borders
            let area = Rect{ x: 0, y: 0, width: width.unwrap_or(80), height: 3 };
            let mut border_buffer = Buffer::empty(area);

            widget.render(&mut drawer, &mut border_buffer, None).unwrap();
            // widget.render_iter(width.unwrap_or(self.buffer.area.width), |cells| {
                // for c in cells {
                    // drawer.print_cell(c).unwrap();
                // }
                // queue!(drawer.writer, crossterm::style::Print("\r\n")).unwrap();
            // });

            string.into()
        })
    }

    pub fn clear_all(&mut self) {
        self.widgets.inner.clear();
        self.dirty = true;
    }

    pub fn clear_non_persistent(&mut self) {
        self.widgets.inner.retain(|w| w.as_ref().persist);
        self.dirty = true;
    }

    pub fn reset(&mut self) {
        self.prev_status_bar_position = 0;
        self.buffer.reset();
        self.border_buffer.reset();
        self.widgets.height = 0;
        self.dirty = true;
    }

    pub async fn draw<W: Write>(
        &mut self,
        writer: &mut W,
        (width, height): (u16, u16),
        shell: &crate::shell::ShellClient,
        prompt: &mut crate::prompt::Prompt,
        buffer: &mut crate::buffer::Buffer,
        status_bar: &mut status_bar::StatusBar,
        clear: bool,
    ) -> Result<()> {

        let mut dirty = clear;
        if clear {
            self.reset();
            buffer.cursor_coord = (0, 0);
            buffer.draw_end_pos = (0, 0);
            status_bar.dirty = true;
            queue!(
                writer,
                cursor::MoveToColumn(0),
                Clear(ClearType::FromCursorDown),
            )?;
        }

        // take up at most 2/3 of the screen
        let max_height = (height * 2 / 3).max(1);
        // reset all if dimensions have changed
        if max_height != self.widgets.max_height || width != self.buffer.area.width {
            dirty = true;
        }

        // resize buffers
        let area = Rect{x: 0, y: 0, width, height: max_height};
        self.buffer.resize(area);
        // enough space to render borders
        self.border_buffer.resize(Rect{ height: 3, ..area});

        // quit early if nothing is dirty
        if !dirty && !prompt.dirty && !buffer.dirty && !self.dirty && !status_bar.dirty {
            return Ok(())
        }

        // old heights
        let old_buffer_height = (prompt.height + buffer.draw_end_pos.1) as usize;
        let old_widgets_height = self.widgets.get_height() as usize;
        let old_status_bar_height = status_bar.inner.as_ref().map_or(0, |w| w.line_count) as usize;
        let old_height = old_buffer_height + old_widgets_height + old_status_bar_height;

        // new heights
        if prompt.dirty {
            prompt.refresh_prompt(shell, area.width).await;
        }
        let new_buffer_height = prompt.height as usize + buffer.get_height_for_width(area.width as _, prompt.width as _).max(1) - 1;
        if status_bar.dirty {
            status_bar.refresh(area);
        }
        let new_status_bar_height = status_bar.get_height() as usize;
        if self.dirty {
            self.widgets.refresh(Rect{ height: max_height.saturating_sub(new_status_bar_height as u16), ..area });
        }
        let new_widgets_height = self.widgets.get_height() as usize;
        let new_height = new_buffer_height + new_widgets_height + new_status_bar_height;

        let mut drawer = backend::Drawer::new(&mut self.buffer, writer, buffer.cursor_coord);
        queue!(drawer.writer, crossterm::terminal::BeginSynchronizedUpdate)?;
        drawer.reset_colours()?;

        if (self.prev_status_bar_position < new_buffer_height + new_widgets_height) || old_status_bar_height > new_status_bar_height {
            if !clear && old_status_bar_height > 0 {
                // we don't know where exactly the status bar is,
                // but it possibly overlaps with the new drawing area
                // clear it
                drawer.move_to_pos((0, self.prev_status_bar_position as _))?;
                queue!(drawer.writer, Clear(ClearType::FromCursorDown))?;
                // it now needs to be redrawn
                status_bar.dirty = true;
            }
            self.prev_status_bar_position = self.prev_status_bar_position.max(new_buffer_height + new_widgets_height);
        }

        // allocate more height
        if new_height > old_height {
            drawer.move_to_pos((0, 0))?;
            allocate_height(drawer.writer, new_height as u16 - 1)?;
            status_bar.dirty = true;
        }

        // redraw the prompt
        if dirty || prompt.dirty {
            // move back to top of drawing area and redraw
            drawer.move_to_pos((0, 0))?;
            drawer.writer.write_all(prompt.as_bytes())?;
            drawer.set_pos((prompt.width, prompt.height - 1));
        }
        drawer.cur_pos = (prompt.width, prompt.height - 1);

        // redraw the buffer
        if dirty || buffer.dirty {
            buffer.render(&mut drawer)?;
        }
        // move to end of buffer
        drawer.cur_pos = buffer.draw_end_pos;

        // redraw the widgets
        if (dirty || self.dirty) && new_widgets_height > 0 {
            drawer.cur_pos = buffer.draw_end_pos;
            drawer.goto_newline()?;
            self.widgets.render(&mut drawer, &mut self.border_buffer, Rect{ height: new_widgets_height as u16, ..area})?;
        }

        for _ in new_buffer_height + new_widgets_height .. old_buffer_height + old_widgets_height {
            drawer.goto_newline()?;
        }
        drawer.clear_to_end_of_line()?;
        drawer.move_to_pos(buffer.cursor_coord)?;

        if new_status_bar_height > 0 && (dirty || status_bar.dirty) && let Some(widget) = &status_bar.inner {
            // save cursor position so we can go back to it
            queue!(drawer.writer, cursor::SavePosition)?;

            // redraw status bar
            // go to the bottom of the screen
            queue!(
                drawer.writer,
                // i dont actually know how far down the bottom of the screen is
                // so just go down by a bigger than amount than it could possibly be
                MoveDown(area.height * 10),
                MoveUp(new_status_bar_height as u16 - 1),
                cursor::MoveToColumn(0),
            )?;
            drawer.set_pos((0, area.height - new_status_bar_height as u16));
            widget.render(&mut drawer, &mut self.border_buffer, None)?;
            // clear everything else below
            drawer.clear_to_end_of_screen()?;

            // go back to cursor
            queue!(drawer.writer, cursor::RestorePosition)?;
        }

        drawer.reset_colours()?;
        execute!(writer, crossterm::terminal::EndSynchronizedUpdate)?;

        self.dirty = false;
        prompt.dirty = false;
        buffer.dirty = false;
        status_bar.dirty = false;
        Ok(())
    }

}
