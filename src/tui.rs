use std::default::Default;
use anyhow::Result;
use crossterm::{
    cursor,
    queue,
    terminal::{Clear, ClearType},
};
use ratatui::{
    *,
    layout::*,
    widgets::*,
    style::*,
    buffer::Buffer,
};
use crate::ui::Ui;
mod backend;

#[derive(Default)]
pub struct Widget{
    id: usize,
    pub constraint: Constraint,
    pub inner: Paragraph<'static>,
    pub align: Alignment,
    pub style: Style,
    pub border_style: Style,
    pub border_title_style: Style,
    pub border_type: BorderType,
    pub block: Block<'static>,
    pub persist: bool,
    pub hidden: bool,

    line_count: usize,
}

impl Widget {

    pub fn replace_tabs(mut text: String) -> String {
        let tab = "    ";
        if text.contains('\t') {
            text = text.replace('\t', tab)
        }
        text
    }

}

pub struct Tui {
    terminal: ratatui::DefaultTerminal,
    counter: usize,
    widgets: Vec<Widget>,

    pub dirty: bool,
    width: u16,
    max_height: u16,
    pub height: u16,

    old_buffer: Buffer,
    new_buffer: Buffer,
}

impl std::default::Default for Tui {
    fn default() -> Self {
        Self{
            terminal: ratatui::init_with_options(TerminalOptions{ viewport: Viewport::Inline(0) }),
            counter: 0,
            widgets: vec![],
            dirty: false,
            width: 0,
            height: 0,
            max_height: 0,

            old_buffer: Default::default(),
            new_buffer: Default::default(),
        }
    }
}

impl Tui {

    pub fn add(&mut self, mut widget: Widget) -> usize {
        let id = self.counter;
        widget.id = id;
        self.counter += 1;
        self.dirty = true;
        self.widgets.push(widget);
        id
    }

    pub fn add_error_message(&mut self, message: String) -> usize {
        let mut widget = Widget::default();
        widget.inner = Paragraph::new(message);
        widget.inner = widget.inner.fg(Color::Red);
        self.add(widget)
    }

    pub fn get_index(&self, id: usize) -> Option<usize> {
        for (i, w) in self.widgets.iter().enumerate() {
            match w.id.cmp(&id) {
                std::cmp::Ordering::Equal => return Some(i),
                std::cmp::Ordering::Greater => break,
                std::cmp::Ordering::Less => (),
            }
        }
        None
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut Widget> {
        self.get_index(id).map(|i| {
            self.dirty = true;
            &mut self.widgets[i]
        })
    }

    pub fn remove(&mut self, id: usize) -> Option<Widget> {
        self.get_index(id).map(|i| {
            self.dirty = true;
            self.widgets.remove(i)
        })
    }

    pub fn clear_all(&mut self) {
        self.widgets.clear();
        self.dirty = true;
    }

    pub fn clear_non_persistent(&mut self) {
        self.widgets.retain(|w| w.persist);
        self.dirty = true;
    }

    fn refresh(&mut self, width: u16, height: u16) {
        self.width = width;
        self.max_height = height;

        let mut area = Rect{
            x: 0,
            y: 0,
            width,
            height,
        };

        self.old_buffer.resize(area);
        self.new_buffer.resize(area);

        if self.widgets.is_empty() {
            return
        }

        let mut frame = self.terminal.get_frame();
        std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);

        let mut max_height = 0;
        let mut last_widget = 0;
        for (i, w) in self.widgets.iter_mut().enumerate() {
            if !w.hidden {
                w.line_count = w.inner.line_count(width);
                max_height += w.line_count;
                last_widget = i;
                if max_height >= area.height as _ {
                    break
                }
            }
        }

        let widgets = &self.widgets[..=last_widget];
        area.height = area.height.min(max_height as _);

        let filter = |w: &&Widget| !w.hidden && w.line_count > 0;

        let layout = Layout::vertical(widgets.iter().filter(filter).map(|w| w.constraint));
        let layouts = layout.split(area);

        for (widget, layout) in widgets.iter().filter(filter).zip(layouts.iter()) {
            frame.render_widget(&widget.inner, *layout);
        }

        std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);
    }

    fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.new_buffer, &mut self.old_buffer);
    }

    pub fn draw(
        &mut self,
        stdout: &mut std::io::Stdout,
        (width, height): (u16, u16),
        clear: bool,
    ) -> Result<()> {

        if clear {
            self.new_buffer.reset();
            self.old_buffer.reset();
            queue!(stdout, Clear(ClearType::FromCursorDown))?;
            self.dirty = true;
        }

        let max_height = height * 2 / 3;
        if max_height != self.max_height || width != self.width {
            self.dirty = true;
        }

        if self.dirty {
            self.swap_buffers();
            self.new_buffer.reset();
            self.refresh(width, max_height);
        }

        self.height = {
            let trailing_empty_lines = self.new_buffer.content()
                .chunks(self.new_buffer.area.width as _)
                .rev()
                .take_while(|line| line.iter().all(|c| {
                    c.symbol() == " " && c.bg == Color::Reset && !c.modifier.intersects(Modifier::UNDERLINED | Modifier::REVERSED)
                }))
                .count();
            self.new_buffer.area.height - trailing_empty_lines as u16
        };

        if self.height == 0 {
            queue!(stdout, Clear(ClearType::FromCursorDown))?;

        } else {
            let updates = self.old_buffer.diff(&self.new_buffer);

            if !updates.is_empty() {
                Ui::allocate_height(stdout, self.height)?;

                queue!(
                    stdout,
                    cursor::SavePosition,
                    cursor::MoveToColumn(0),
                    // clear everything below
                    cursor::MoveDown(self.height),
                    Clear(ClearType::FromCursorDown),
                    cursor::MoveUp(self.height),
                    cursor::MoveToNextLine(1),
                )?;

                // the last line will have been cleared so always redraw it
                {
                    let old = &mut self.old_buffer.content;
                    let start = ((self.height - 1) * width) as usize;
                    for cell in old[start .. start + width as usize].iter_mut() {
                        cell.reset();
                    }
                }
                let updates = self.old_buffer.diff(&self.new_buffer);

                backend::draw(stdout, updates.into_iter())?;
                queue!(stdout, cursor::RestorePosition)?;
            }
        }

        self.dirty = false;
        Ok(())
    }

}
