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
pub use widget::Widget;
pub mod command_line;
pub mod status_bar;
pub mod text;
pub mod layout;
pub mod error_message;
pub use drawer::{Drawer, Canvas, DummyCanvas};
pub use error_message::ErrorMessage;

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

#[derive(Default)]
pub struct Tui {
    pub nodes: layout::Nodes,
    buffer: Buffer,
    top_y: u32,
    pub max_height: u32,
    pub dirty: bool,
    error_msg: Option<ErrorMessage>,
}

impl Tui {

    pub fn get_size(&self) -> (u16, u16) {
        (self.buffer.area.width, self.buffer.area.height)
    }

    pub fn add(&mut self, widget: Widget) -> usize {
        self.dirty = true;
        self.nodes.add(layout::NodeKind::Widget(widget)).id
    }

    pub fn add_message(&mut self, message: &str) -> usize {
        let mut widget = widget::Widget::default();
        widget.inner.clear();
        for line in message.split('\n') {
            widget.inner.push_line(line.into(), None);
        }
        self.add(widget)
    }

    pub fn add_error_message(&mut self, message: &str) -> usize {
        self.dirty = true;
        let error = self.error_msg.get_or_insert_with(|| ErrorMessage::new(&mut self.nodes));
        error.add_error(message, &mut self.nodes);
        error.id
    }

    pub fn add_zle_message(&mut self, message: &[u8]) -> usize {
        let mut widget = widget::Widget::default();
        widget.ansi.ocrnl = true; // treat \r as \n
        widget.feed_ansi(message.into());
        self.add(widget)
    }

    pub fn get_node(&self, id: usize) -> Option<&layout::Node> {
        self.nodes.get_node(id)
    }

    pub fn get_node_mut(&mut self, id: usize) -> Option<&mut layout::Node> {
        self.dirty = true;
        self.nodes.get_node_mut(id)
    }

    pub fn remove(&mut self, id: usize) -> Option<layout::Node> {
        self.dirty = true;
        self.nodes.remove(id)
    }

    pub fn render_to_string(&self, id: usize, width: Option<u16>) -> Option<BString> {
        let node = self.get_node(id)?;
        let width = width.unwrap_or(self.buffer.area.width);
        let width = std::num::NonZero::new(width).map_or(80, |w| w.get());

        // refresh tmp size
        node.refresh(&self.nodes.map, width, None, Some(Constraint::Min(0)), true);

        let mut string = vec![];
        let mut writer = Cursor::new(&mut string);
        // always start with a reset
        writer.write_all(b"\x1b[0m").unwrap();
        let mut canvas = drawer::DummyCanvas::default();
        canvas.size = (width, u16::MAX);
        let mut drawer = drawer::Drawer::new(&mut canvas, &mut writer, (0, 0));

        self.nodes.render_node(node, &mut drawer, true).unwrap();
        Some(string.into())
    }

    pub fn clear_all(&mut self) {
        self.clear_error();
        self.nodes.clear_all();
        self.dirty = true;
    }

    pub fn clear_non_persistent(&mut self) {
        self.clear_error();
        self.nodes.clear_non_persistent();
        self.dirty = true;
    }

    pub fn reset(&mut self) {
        self.top_y = 0;
        self.buffer.reset();
        self.nodes.height.set(0);
        self.dirty = true;
    }

    fn clear_error(&mut self) {
        if let Some(error_msg) = &mut self.error_msg {
            error_msg.clear(&mut self.nodes);
            self.dirty = true;
        }
    }

    pub fn draw<W: Write>(
        &mut self,
        writer: &mut W,
        (width, height): (u32, u32),
        top_y: Option<u32>,
        mut cmdline: command_line::CommandLine<'_>,
        status_bar: &mut status_bar::StatusBar,
        clear: bool,
    ) -> Result<()> {

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

        // quit early if nothing is dirty
        if !clear && !cmdline.is_dirty() && !self.dirty && !status_bar.dirty {
            return Ok(())
        }

        // old heights
        let mut old_cmdline_height = cmdline.get_height();
        let mut old_widgets_height = self.nodes.get_height() as usize;
        let mut old_status_bar_height = status_bar.inner.as_ref().map_or(0, |w| w.line_count) as usize;
        if clear {
            old_cmdline_height = 0;
            old_widgets_height = 0;
            old_status_bar_height = 0;
        }

        let old_height = old_cmdline_height + old_widgets_height + old_status_bar_height;

        // refresh the widgets etc
        if cmdline.is_dirty() {
            cmdline.refresh(width as _);
        }
        if status_bar.dirty {
            status_bar.refresh(width as _);
        }
        if self.dirty {
            // the nodes are the main part of the ui that can be resized to fit on the screen
            // even if there is more space we could get by scrolling, we should avoid it because it is jarring
            // so this is the only one that cares about the height
            let max_height = self.max_height.saturating_sub(status_bar.get_height() as u32 + cmdline.get_height() as u32);
            self.nodes.refresh(
                width as _,
                Some(max_height as _),
            );
        }

        // new heights
        let new_cmdline_height = cmdline.get_height();
        let new_widgets_height = self.nodes.get_height() as usize;
        let new_status_bar_height = status_bar.get_height() as usize;
        let new_height = (new_cmdline_height + new_widgets_height + new_status_bar_height).min(self.max_height as _);

        // resize buffers
        let area = Rect{x: 0, y: 0, width: width as _, height: height as _};
        self.buffer.resize(area);

        let mut drawer = drawer::Drawer::new(&mut self.buffer, writer, cmdline.cursor_coord);
        queue!(drawer.writer, crossterm::terminal::BeginSynchronizedUpdate)?;
        drawer.reset_colours()?;

        // allocate more height
        if new_height > old_height {
            drawer.move_to((0, 0));
            drawer.allocate_height(new_height as u16 - 1)?;
        }

        if let Some(top_y) = top_y {
            self.top_y = top_y;
        }
        if self.top_y + new_height as u32 > height {
            // page has scrolled
            self.top_y = height - new_height as u32;
            // invalidate the status bar if the page has scrolled as it needs to stick to the bottom
            status_bar.dirty = true;
        }
        let status_bar_y = area.height.saturating_sub(self.top_y as u16 + new_status_bar_height as u16);

        // move back to top of drawing area
        drawer.move_to((0, 0));
        // draw cmdline
        cmdline.render(&mut drawer, clear)?;

        // redraw the widgets
        // if cmdline height has changed then the widgets get repositioned
        if (clear || self.dirty || old_cmdline_height != new_cmdline_height)
            && drawer.try_move_to(cmdline.draw_end_pos)
        {
            if new_widgets_height > 0 {
                // go to next line after end of buffer
                drawer.goto_newline(None)?;
                self.nodes.render(&mut drawer, false)?;
            }

            // clear everything from here to the start of status bar
            for _ in drawer.get_pos().1 + 1 .. status_bar_y {
                drawer.goto_newline(None)?;
            }
            drawer.clear_to_end_of_line(None)?;
        }

        // redraw status bar
        if new_status_bar_height > 0
            && (clear || status_bar.dirty)
            && status_bar.is_visible()
            && new_height >= new_cmdline_height + new_status_bar_height
            && drawer.try_move_to((0, status_bar_y))
        {
                status_bar.render(&mut drawer)?;
        }

        // go back to the cursor
        if drawer.validate_pos(cmdline.cursor_coord) {
            drawer.move_to_pos(cmdline.cursor_coord)?;
        }

        drawer.reset_colours()?;
        execute!(writer, crossterm::terminal::EndSynchronizedUpdate)?;

        self.dirty = false;
        cmdline.set_is_dirty(false);
        status_bar.dirty = false;
        Ok(())
    }

}
