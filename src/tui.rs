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
mod drawer;
mod wrap;
mod scroll;
pub mod widget;
pub mod command_line;
pub mod status_bar;
pub mod ansi;
pub mod text;
pub use drawer::{Drawer, Canvas};

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

pub fn allocate_height<W: Write>(stdout: &mut W, height: u16) -> std::io::Result<()> {
    for _ in 0 .. height {
        // vertical tab, this doesn't change x
        queue!(stdout, style::Print("\x0b"))?;
    }
    queue!(stdout, MoveUp(height))?;
    Ok(())
}

fn cell_is_empty(cell: &ratatui::buffer::Cell) -> bool {
    cell.symbol() == " " && cell.bg == Color::Reset && !cell.modifier.intersects(Modifier::UNDERLINED | Modifier::REVERSED | Modifier::CROSSED_OUT)
}

fn buffer_nonempty_height(buffer: &Buffer) -> u16 {
    let trailing_empty_lines = buffer.content()
        .chunks(buffer.area.width as _)
        .rev()
        .take_while(|line| line.iter().all(cell_is_empty))
        .count();
    buffer.area.height - trailing_empty_lines as u16
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
}

impl Widgets {
    fn get_height(&self) -> u16 {
        self.inner.iter().map(|w| w.as_ref().line_count).sum()
    }

    fn refresh(&mut self, area: Rect) {
        let mut height = 0;
        for w in &mut self.inner {
            let w = w.as_mut();
            if w.hidden || area.height <= height as _ {
                w.line_count = 0;
            } else {
                w.line_count = w.get_height_for_width(area).min(area.height - height);
                height += w.line_count;
            }
        }
    }

    fn render<W :Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
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
    max_height: u16,
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
            widget.inner.push_line(line.into(), Some(Style::new().fg(Color::Red).into()));
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
            let width = width.unwrap_or(self.buffer.area.width);
            let width = std::num::NonZero::new(width).map_or(80, |w| w.get());

            let mut string = vec![];
            let mut writer = Cursor::new(&mut string);
            // always start with a reset
            writer.write_all(b"\x1b[0m").unwrap();
            let mut canvas = drawer::DummyCanvas::default();
            canvas.size = (width, u16::MAX);
            let mut drawer = drawer::Drawer::new(&mut canvas, &mut writer, (0, 0));
            let mut border_buffer = Buffer::default();

            widget.render(&mut drawer, &mut border_buffer, None).unwrap();
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
        mut cmdline: command_line::CommandLine<'_>,
        status_bar: &mut status_bar::StatusBar,
        mut clear: bool,
    ) -> Result<()> {

        // take up at most 2/3 of the screen
        let max_height = (height * 2 / 3).max(1);
        // redraw all if dimensions have changed
        if max_height != self.max_height || width != self.buffer.area.width {
            self.max_height = max_height;
            clear = true;
        }

        if clear {
            self.reset();
            cmdline.reset();
            status_bar.reset();
            queue!(
                writer,
                cursor::MoveToColumn(0),
                Clear(ClearType::FromCursorDown),
            )?;
        }

        // resize buffers
        let area = Rect{x: 0, y: 0, width, height: self.max_height};
        self.buffer.resize(area);

        // quit early if nothing is dirty
        if !clear && !cmdline.is_dirty() && !self.dirty && !status_bar.dirty {
            return Ok(())
        }

        // old heights
        let mut old_cmdline_height = cmdline.get_height();
        let mut old_widgets_height = self.widgets.get_height() as usize;
        let mut old_status_bar_height = status_bar.inner.as_ref().map_or(0, |w| w.line_count) as usize;
        if clear {
            old_cmdline_height = 0;
            old_widgets_height = 0;
            old_status_bar_height = 0;
        }

        let old_height = old_cmdline_height + old_widgets_height + old_status_bar_height;

        // refresh the widgets etc
        if cmdline.is_dirty() {
            cmdline.refresh(area).await;
        }
        if self.dirty {
            self.widgets.refresh(Rect{ height: max_height.saturating_sub(status_bar.get_height()), ..area });
        }
        if status_bar.dirty {
            status_bar.refresh(area);
        }

        // new heights
        let new_cmdline_height = cmdline.get_height();
        let new_widgets_height = self.widgets.get_height() as usize;
        let new_status_bar_height = status_bar.get_height() as usize;
        let new_height = new_cmdline_height + new_widgets_height + new_status_bar_height;

        let mut drawer = drawer::Drawer::new(&mut self.buffer, writer, cmdline.cursor_coord);
        queue!(drawer.writer, crossterm::terminal::BeginSynchronizedUpdate)?;
        drawer.reset_colours()?;

        if (self.prev_status_bar_position < new_cmdline_height + new_widgets_height) || old_status_bar_height > new_status_bar_height {
            if !clear && old_status_bar_height > 0 {
                // we don't know where exactly the status bar is,
                // but it possibly overlaps with the new drawing area
                // so we clear it
                drawer.move_to((0, self.prev_status_bar_position as _));
                drawer.clear_to_end_of_screen(None)?;
                // it now needs to be redrawn
                status_bar.dirty = true;
            }
            self.prev_status_bar_position = self.prev_status_bar_position.max(new_cmdline_height + new_widgets_height);
        }

        // allocate more height
        if new_height > old_height {
            drawer.move_to((0, 0));
            drawer.allocate_height(new_height as u16 - 1)?;
            status_bar.dirty = true;
        }

        // move back to top of drawing area
        drawer.move_to((0, 0));
        cmdline.render(&mut drawer, clear)?;

        // redraw the widgets
        // if cmdline height has changed then the widgets get repositioned
        if (clear || self.dirty || old_cmdline_height != new_cmdline_height) && new_widgets_height > 0 {
            // go to next line after end of buffer
            drawer.move_to(cmdline.draw_end_pos);
            drawer.goto_newline(None)?;
            self.widgets.render(&mut drawer, &mut self.border_buffer, Rect{ height: new_widgets_height as u16, ..area})?;
        }

        // the prompt/buffer/widgets used to be bigger, so clear the extra bits
        let trailing_height = (old_cmdline_height + old_widgets_height).saturating_sub(new_cmdline_height + new_widgets_height);
        if trailing_height > 0 {
            drawer.move_to((area.width, (new_cmdline_height + new_widgets_height - 1) as _));
            for _ in 0 .. trailing_height {
                drawer.goto_newline(None)?;
            }
            drawer.clear_to_end_of_line(None)?;
        }

        // go back to the cursor
        drawer.move_to_pos(cmdline.cursor_coord)?;

        if new_status_bar_height > 0 && (clear || status_bar.dirty) && let Some(widget) = &status_bar.inner {
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
            // drawer.clear_to_end_of_screen(None)?;

            // go back to cursor
            queue!(drawer.writer, cursor::RestorePosition)?;
        }

        drawer.reset_colours()?;
        execute!(writer, crossterm::terminal::EndSynchronizedUpdate)?;

        self.dirty = false;
        cmdline.set_is_dirty(false);
        status_bar.dirty = false;
        Ok(())
    }

}
