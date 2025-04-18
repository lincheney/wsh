use std::default::Default;
use std::io::Write;
use anyhow::Result;
use crossterm::{
    cursor,
    queue,
    execute,
    terminal::{Clear, ClearType},
};
use ratatui::{
    *,
    text::*,
    layout::*,
    widgets::*,
    style::*,
    buffer::Buffer,
};
use crate::ui::Ui;
mod backend;
pub mod ansi;

fn buffer_nonempty_height(buffer: &Buffer) -> u16 {
    let trailing_empty_lines = buffer.content()
        .chunks(buffer.area.width as _)
        .rev()
        .take_while(|line| line.iter().all(|c| {
            c.symbol() == " " && c.bg == Color::Reset && !c.modifier.intersects(Modifier::UNDERLINED | Modifier::REVERSED)
        }))
        .count();
    buffer.area.height - trailing_empty_lines as u16
}

#[derive(Debug, Default)]
pub struct StyleOptions {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: Option<bool>,
    pub dim: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<Option<Color>>,
    pub strikethrough: Option<bool>,
    pub reversed: Option<bool>,
    pub blink: Option<bool>,
}

impl StyleOptions {
    pub fn as_style(&self) -> Style {
        let mut style = Style {
            fg: self.fg,
            bg: self.bg,
            underline_color: self.underline.flatten(),
            add_modifier: Modifier::empty(),
            sub_modifier: Modifier::empty(),
        };

        macro_rules! set_modifier {
            ($field:ident, $enum:ident) => (
                if let Some($field) = self.$field {
                    let value = Modifier::$enum;
                    if $field {
                        style.add_modifier.insert(value);
                    } else {
                        style.sub_modifier.insert(value);
                    }
                }
            )
        }

        set_modifier!(bold, BOLD);
        set_modifier!(dim, DIM);
        set_modifier!(italic, ITALIC);
        set_modifier!(strikethrough, CROSSED_OUT);
        set_modifier!(reversed, REVERSED);
        set_modifier!(blink, SLOW_BLINK);

        style
    }

    pub fn merge(&self, other: &Self) -> Self {
        Self{
            fg: other.fg.or(self.fg),
            bg: other.bg.or(self.bg),
            bold: other.bold.or(self.bold),
            dim: other.dim.or(self.dim),
            italic: other.italic.or(self.italic),
            underline: other.underline.or(self.underline),
            strikethrough: other.strikethrough.or(self.strikethrough),
            reversed: other.reversed.or(self.reversed),
            blink: other.blink.or(self.blink),
        }
    }

}

#[derive(Default, Debug)]
pub struct Widget{
    id: usize,
    pub constraint: Constraint,
    current_constraint: Constraint,
    pub inner: Text<'static>,
    pub style: StyleOptions,
    pub border_style: Style,
    pub border_title_style: Style,
    pub border_type: BorderType,
    pub block: Block<'static>,
    pub persist: bool,
    pub hidden: bool,

    line_count: u16,
    text_overrides_style: bool,
}

pub fn render_text(
    area: Rect,
    buffer: &mut Buffer,
    mut offset: (u16, u16),
    text: &Text,
    style: bool,
    override_style: Option<Style>,
) -> (u16, u16) {

    let mut new_offset = offset;
    for line in text.iter() {
        offset = new_offset;
        if offset.1 >= area.height {
            break
        }

        for graph in line.styled_graphemes(text.style) {

            use unicode_width::UnicodeWidthStr;
            let width = graph.symbol.width();
            if width == 0 {
                continue
            }

            let symbol = if graph.symbol.is_empty() { " " } else { graph.symbol };
            let cell = &mut buffer[(area.x + offset.0, area.y + offset.1)];
            cell.set_symbol(symbol);

            if style {
                if let Some(style) = override_style {
                    cell.set_style(graph.style.patch(style));
                } else {
                    cell.set_style(graph.style);
                }
            }

            offset.0 += width as u16;
            if offset.0 >= area.width {
                new_offset = (0, offset.1 + 1);
                if new_offset.1 >= area.height {
                    break
                }
                offset = new_offset;
            }
        }

        new_offset = (0, offset.1 + 1);
    }

    offset
}

impl Widget {

    pub fn replace_tabs(mut text: String) -> String {
        let tab = "    ";
        if text.contains('\t') {
            text = text.replace('\t', tab)
        }
        text
    }

    fn render(&self, area: Rect, buffer: &mut Buffer) {
        buffer.set_style(area, self.inner.style);
        self.block.render_ref(area, buffer);
        let area = self.block.inner(area);
        render_text(
            area,
            buffer,
            (0, 0),
            &self.inner,
            true,
            if self.text_overrides_style { Some(self.inner.style) } else { None },
        );
    }

    fn measure(&self, area: Rect, buffer: &mut Buffer) {
        self.block.render_ref(area, buffer);
        let area = self.block.inner(area);
        render_text(
            area,
            buffer,
            (0, 0),
            &self.inner,
            false,
            None,
        );
    }

    fn line_count(&self, area: Rect, buffer: &mut Buffer) -> u16 {
        let mut height = 0;
        if let Constraint::Min(min) = self.constraint {
            height = height.max(min);
        }

        if area.width >= 1 {
            buffer.resize(area);
            self.measure(area, buffer);

            height = height.max(buffer_nonempty_height(buffer))
        }
        height
    }

}

#[derive(Debug)]
pub enum WidgetWrapper {
    Widget(Widget),
    Ansi(ansi::Parser),
}

impl WidgetWrapper {
    pub fn as_ref(&self) -> &Widget {
        match self {
            Self::Widget(ref w) => w,
            Self::Ansi(p) => &p.widget,
        }
    }

    pub fn as_mut(&mut self) -> &mut Widget {
        match self {
            Self::Widget(ref mut w) => w,
            Self::Ansi(p) => p.as_widget(),
        }
    }
}

impl From<Widget> for WidgetWrapper {
    fn from(widget: Widget) -> Self {
        Self::Widget(widget)
    }
}

impl From<ansi::Parser> for WidgetWrapper {
    fn from(parser: ansi::Parser) -> Self {
        Self::Ansi(parser)
    }
}

#[derive(Default)]
pub struct Tui {
    counter: usize,
    widgets: Vec<WidgetWrapper>,

    pub dirty: bool,
    width: u16,
    max_height: u16,
    pub height: u16,

    old_buffer: Buffer,
    new_buffer: Buffer,
    line_count_buffer: Buffer,
}

impl Tui {

    pub fn add(&mut self, mut widget: WidgetWrapper) -> (usize, &mut WidgetWrapper) {
        let id = self.counter;
        widget.as_mut().id = id;
        self.counter += 1;
        self.dirty = true;
        self.widgets.push(widget);
        (id, self.widgets.last_mut().unwrap())
    }

    pub fn add_error_message(&mut self, message: String) -> (usize, &mut WidgetWrapper) {
        let mut widget = Widget::default();
        let message = Widget::replace_tabs(message);
        let message: Vec<_> = message.split('\n').map(|l| Line::from(l.to_owned())).collect();
        widget.inner = widget.inner.fg(Color::Red);
        widget.inner.lines = message;
        self.add(widget.into())
    }

    pub fn get_index(&self, id: usize) -> Option<usize> {
        for (i, w) in self.widgets.iter().enumerate() {
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
            &mut self.widgets[i]
        })
    }

    pub fn remove(&mut self, id: usize) -> Option<WidgetWrapper> {
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
        self.widgets.retain(|w| w.as_ref().persist);
        self.dirty = true;
    }

    fn refresh(&mut self, area: Rect) {
        self.width = area.width;
        self.max_height = area.height;

        if self.widgets.is_empty() {
            return
        }

        let mut max_height = 0;
        let mut last_widget = 0;
        for (i, w) in self.widgets.iter_mut().enumerate() {
            let w = w.as_mut();
            if !w.hidden {
                w.line_count = w.line_count(area, &mut self.line_count_buffer);
                w.current_constraint = Constraint::Max(w.line_count);
                max_height += w.line_count;
                last_widget = i;
                if max_height >= area.height as _ {
                    break
                }
            }
        }

        let area = Rect{ height: area.height.min(max_height as _), ..area };

        let widgets = &self.widgets[..=last_widget];
        let widgets = widgets.iter().map(|w| w.as_ref()).filter(|w| !w.hidden && w.line_count > 0);

        let layout = Layout::vertical(widgets.clone().map(|w| w.current_constraint));
        let layouts = layout.split(area);

        for (widget, layout) in widgets.zip(layouts.iter()) {
            widget.render(*layout, &mut self.new_buffer);
        }
    }

    fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.new_buffer, &mut self.old_buffer);
    }

    pub async fn draw(
        &mut self,
        stdout: &mut std::io::Stdout,
        (width, height): (u16, u16),
        shell: &crate::shell::Shell,
        prompt: &mut crate::prompt::Prompt,
        buffer: &mut crate::buffer::Buffer,
        mut clear: bool,
    ) -> Result<()> {

        if clear {
            self.new_buffer.reset();
            self.old_buffer.reset();
            queue!(stdout, Clear(ClearType::FromCursorDown))?;
            self.dirty = true;
            self.height = 0;
        }
        let old_height = self.height;
        let old_cursor_coord = buffer.cursor_coord;

        let max_height = (height * 2 / 3).max(1);
        if max_height != self.max_height || width != self.width {
            clear = true;
        }

        let new_buffer = clear || self.dirty || prompt.dirty || buffer.dirty;
        if new_buffer {
            self.swap_buffers();
            self.new_buffer.reset();
        }

        let area = Rect{x: 0, y: 0, width, height: max_height};
        self.old_buffer.resize(area);
        self.new_buffer.resize(area);

        if clear || prompt.dirty {
            // refresh the prompt
            prompt.dirty = true;
            prompt.refresh_prompt(&mut *shell.lock().await, area.width);
            // reset here
            self.new_buffer.content.iter_mut()
                .take((prompt.height * area.width + prompt.width) as usize)
                .for_each(|cell| {
                    cell.reset();
                });
        }

        if clear || buffer.dirty {
            // refresh the buffer
            buffer.render(area, &mut self.new_buffer, prompt);
        } else if new_buffer {
            // copy over from old buffer
            self.old_buffer.content.iter()
                .zip(self.new_buffer.content.iter_mut())
                .skip((prompt.height * area.width + prompt.width) as usize)
                .take((buffer.height * area.width + buffer.width) as usize)
                .for_each(|(old, new)| {
                    new.set_symbol(old.symbol());
                    new.set_style(old.style());
                });
        }

        let offset = prompt.height + buffer.height - 1;
        let area = Rect{ y: area.y + offset, height: area.height - offset, ..area };

        if clear || self.dirty {
            self.refresh(area);
        }

        self.height = buffer_nonempty_height(&self.new_buffer).max(buffer.cursor_coord.1 + 1);
        let updates = self.old_buffer.diff(&self.new_buffer);
        if !updates.is_empty() || prompt.dirty {
            queue!(stdout, crossterm::terminal::BeginSynchronizedUpdate)?;

            Ui::allocate_height(stdout, self.height.saturating_sub(old_cursor_coord.1 + 1))?;

            // move back to top of drawing area and redraw
            if self.height < old_height {
                let offset = self.height.saturating_sub(old_cursor_coord.1);
                queue!(
                    stdout,
                    // clear everything below
                    cursor::MoveToColumn(0),
                    crate::ui::MoveDown(offset),
                    Clear(ClearType::FromCursorDown),
                    crate::ui::MoveUp(offset),
                )?;
            }

            queue!(
                stdout,
                // move to top left
                cursor::MoveToColumn(0),
                crate::ui::MoveUp(old_cursor_coord.1),
                cursor::SavePosition,
            )?;

            let cursor_is_at_end = buffer.cursor_coord.1 == width && buffer.cursor_is_at_end();
            backend::draw(stdout, updates.into_iter())?;
            queue!(stdout, cursor::RestorePosition)?;

            if clear || prompt.dirty {
                stdout.write_all(prompt.as_bytes())?;
                queue!(
                    stdout,
                    // move to top left
                    cursor::MoveToColumn(0),
                    crate::ui::MoveUp(prompt.height - 1),
                )?;
            }

            // position the cursor
            if cursor_is_at_end {
                // cursor is at the very very end of line
                // only way i know of to get back there is to redraw that line
                let start = (buffer.cursor_coord.1 * width) as usize;
                for cell in self.old_buffer.content[start .. start + width as usize].iter_mut() {
                    cell.reset();
                }
                let updates = self.old_buffer.diff(&self.new_buffer);
                backend::draw(stdout, updates.into_iter().filter(|(_, y, _)| *y == buffer.cursor_coord.1))?;
            } else {
                // otherwise just position it normally
                queue!(
                    stdout,
                    crate::ui::MoveDown(buffer.cursor_coord.1),
                    cursor::MoveToColumn(buffer.cursor_coord.0),
                    crossterm::terminal::EndSynchronizedUpdate,
                )?;
            }
            execute!(stdout, crossterm::terminal::EndSynchronizedUpdate)?;
        }

        self.dirty = false;
        prompt.dirty = false;
        buffer.dirty = false;
        Ok(())
    }

}
