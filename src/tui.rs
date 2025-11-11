use std::default::Default;
use std::io::Write;
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
    text::*,
    layout::*,
    widgets::*,
    style::*,
    buffer::Buffer,
};
mod backend;
pub use backend::{DrawInstruction, Drawer};
pub mod status_bar;
pub mod ansi;

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
    pub constraint: Option<Constraint>,
    pub inner: Text<'static>,
    pub style: StyleOptions,
    pub border_style: Style,
    pub border_title_style: Style,
    pub border_type: BorderType,
    pub block: Option<Block<'static>>,
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
            text = text.replace('\t', tab);
        }
        text
    }

    fn render(&self, area: Rect, buffer: &mut Buffer) {
        buffer.set_style(area, self.inner.style);
        let area = if let Some(ref block) = self.block {
            block.render_ref(area, buffer);
            block.inner(area)
        } else {
            area
        };
        render_text(
            area,
            buffer,
            (0, 0),
            &self.inner,
            true,
            if self.text_overrides_style { Some(self.inner.style) } else { None },
        );
    }

    fn measure(&self, mut area: Rect, buffer: &mut Buffer) {
        if let Some(ref block) = self.block {
            let inner = block.inner(area);
            area = Rect{
                y: area.y + area.height - inner.height,
                ..inner
            };
        }

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
        if let Some(Constraint::Min(min)) = self.constraint {
            height = height.max(min);
        }

        if area.width >= 1 {
            buffer.resize(area);
            buffer.reset();
            self.measure(area, buffer);

            height = height.max(buffer_nonempty_height(buffer));
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
            Self::Widget(w) => w,
            Self::Ansi(p) => &p.widget,
        }
    }

    pub fn as_mut(&mut self) -> &mut Widget {
        match self {
            Self::Widget(w) => w,
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
struct Widgets {
    inner: Vec<WidgetWrapper>,
    width: u16,
    pub height: u16,
    max_height: u16,
    buffer: Buffer,
    line_count_buffer: Buffer,
}

impl Widgets {
    fn refresh(&mut self, area: Rect, max_height: u16) -> bool {
        self.width = area.width;
        self.max_height = max_height;

        if self.inner.is_empty() {
            return false
        }

        let mut max_height = 0;
        let mut last_widget = 0;
        for (i, w) in self.inner.iter_mut().enumerate() {
            let w = w.as_mut();
            if !w.hidden {
                w.line_count = w.line_count(area, &mut self.line_count_buffer);
                max_height += w.line_count;
                last_widget = i;
                if max_height >= area.height as _ {
                    break
                }
            }
        }

        let area = Rect{ height: area.height.min(max_height as _), ..area };

        let widgets = &self.inner[..=last_widget];
        let widgets = widgets.iter().map(|w| w.as_ref()).filter(|w| !w.hidden && w.line_count > 0);

        let layout = Layout::vertical(widgets.clone().map(|w| w.constraint.unwrap_or(Constraint::Max(w.line_count))));
        let layouts = layout.split(area);

        let mut found = false;
        for (widget, layout) in widgets.zip(layouts.iter()) {
            widget.render(*layout, &mut self.buffer);
            found = true;
        }
        found
    }
}

#[derive(Default)]
pub struct Tui {
    counter: usize,
    widgets: Widgets,
    buffer: Buffer,
    pub dirty: bool,
    // TODO move this into status bar struct
    status_bar_buffer: Buffer,
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
        let mut widget = Widget::default();
        let message = Widget::replace_tabs(message);
        let message: Vec<_> = message.split('\n').map(|l| Line::from(l.to_owned())).collect();
        widget.inner.lines = message;
        self.add(widget.into())
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

    pub fn clear_all(&mut self) {
        self.widgets.inner.clear();
        self.dirty = true;
    }

    pub fn clear_non_persistent(&mut self) {
        self.widgets.inner.retain(|w| w.as_ref().persist);
        self.dirty = true;
    }

    pub fn reset(&mut self) {
        self.buffer.reset();
        self.widgets.buffer.reset();
        self.widgets.height = 0;
        self.dirty = true;
    }

    pub async fn draw<W: Write>(
        &mut self,
        writer: &mut W,
        (width, height): (u16, u16),
        shell: &crate::shell::Shell,
        prompt: &mut crate::prompt::Prompt,
        buffer: &mut crate::buffer::Buffer,
        status_bar: &mut status_bar::StatusBar,
        mut clear: bool,
    ) -> Result<()> {

        if clear {
            self.buffer.reset();
            queue!(writer, Clear(ClearType::FromCursorDown))?;
            self.dirty = true;
            self.widgets.height = 0;
        }

        // take up at most 2/3 of the screen
        let max_height = (height * 2 / 3).max(1);
        // reset all if dimensions have changed
        if max_height != self.widgets.max_height || width != self.widgets.width {
            clear = true;
        }

        // resize buffers
        let area = Rect{x: 0, y: 0, width, height: max_height};
        self.buffer.resize(area);
        self.widgets.buffer.resize(area);
        self.status_bar_buffer.resize(area);

        // quit early if nothing is dirty
        if !clear && !prompt.dirty && !buffer.dirty && !self.dirty && !status_bar.dirty {
            return Ok(())
        }

        queue!(writer, crossterm::terminal::BeginSynchronizedUpdate)?;
        let mut drawer = backend::Drawer::new(&mut self.buffer, writer, buffer.cursor_coord);
        drawer.reset_colours()?;

        // redraw the prompt
        if clear || prompt.dirty {
            prompt.refresh_prompt(&mut shell.lock().await, area.width);
            // move back to top of drawing area and redraw
            drawer.cur_pos = (0, 0);
            drawer.writer.write_all(prompt.as_bytes())?;
            drawer.set_pos((prompt.width, prompt.height - 1));
        } else {
            // if not redrawn, just move to where it should end
            drawer.cur_pos = (prompt.width, prompt.height - 1);
        }

        // redraw the buffer
        if clear || buffer.dirty {
            buffer.render(&mut drawer, false)?;
        } else {
            // if not dirty, must at least figure out where the cursor position is
            buffer.render(&mut drawer, true)?;
        }
        // move to end of buffer
        drawer.cur_pos = buffer.draw_end_pos;

        // restrict widgets to after the buffer
        let area = Rect{ y: drawer.cur_pos.1, height: area.height - drawer.cur_pos.1, ..area };
        let old_height = (self.widgets.height, status_bar.inner.as_ref().map_or(0, |w| w.line_count));

        // refresh status bar
        // need to refresh this FIRST
        // to get the bar height
        // as it in turn restricts the height available for other widgets
        let status_bar_height = if let Some(ref mut widget) = status_bar.inner {
            if status_bar.dirty {
                widget.line_count = widget.line_count(area, &mut self.widgets.line_count_buffer);
                self.status_bar_buffer.reset();
                widget.render(area, &mut self.status_bar_buffer);
            }
            widget.line_count
        } else {
            0
        };

        // refresh widgets
        if clear || self.dirty {
            let area = Rect{ height: area.height - status_bar_height, ..area };
            self.widgets.buffer.reset();
            self.widgets.refresh(area, max_height);
            self.widgets.height = buffer_nonempty_height(&self.widgets.buffer).saturating_sub(drawer.cur_pos.1);
        }
        let new_height = (self.widgets.height, status_bar.inner.as_ref().map_or(0, |w| w.line_count));


        // allocate enough height for the widgets
        let resize = new_height.0 + new_height.1 > old_height.0 + old_height.1;
        if resize {
            // go back to the cursor, this is important since adding more screen lines can change where the cursor is
            queue!(drawer.writer, cursor::RestorePosition)?;
            // allocate height
            allocate_height(drawer.writer, new_height.0 + new_height.1 + buffer.draw_end_pos.1 - buffer.cursor_coord.1)?;
            // save the new position
            queue!(drawer.writer, cursor::SavePosition)?;
            // clear the old status bar
            if old_height.1 > 0 {
                drawer.move_to_pos((0, buffer.draw_end_pos.1 + old_height.0 + 1))?;
                queue!(drawer.writer, Clear(ClearType::FromCursorDown))?;
            }
            // clear the extra lines
            for y in buffer.draw_end_pos.1 + old_height.0 .. area.height {
                drawer.clear_cells((0, y), area.width);
            }
        }

        if clear || self.dirty {
            drawer.cur_pos = buffer.draw_end_pos;
            // redraw widgets
            for line in self.widgets.buffer.content.chunks(area.width as _).take(self.widgets.height as _).skip(drawer.cur_pos.1 as usize) {
                drawer.draw(DrawInstruction::Newline)?;
                for cell in line {
                    drawer.draw_cell(cell, false)?;
                }
                drawer.draw(DrawInstruction::ClearRestOfLine)?;
            }
        } else {
            // if not redrawn, just move to where it should end
            drawer.cur_pos = (area.width, self.widgets.height);
        }

        if status_bar_height > 0 {
            if clear || status_bar.dirty || resize {
                // redraw status bar
                // go to the bottom of the screen
                queue!(
                    drawer.writer,
                    Clear(ClearType::FromCursorDown),
                    MoveDown(area.height * 10),
                    MoveUp(status_bar_height - 1),
                    cursor::MoveToColumn(0),
                )?;
                drawer.set_pos((0, area.height - status_bar_height));

                for (i, line) in self.status_bar_buffer.content.chunks(area.width as _).take(status_bar_height as _).enumerate() {
                    if i > 0 {
                        drawer.draw(DrawInstruction::Newline)?;
                    }
                    for cell in line {
                        // always redraw in case we've scrolled
                        drawer.draw_cell(cell, true)?;
                    }
                    drawer.draw(DrawInstruction::ClearRestOfLine)?;
                }
                if status_bar_height < old_height.1 {
                    for _ in status_bar_height .. old_height.1 {
                        drawer.draw(DrawInstruction::Newline)?;
                    }
                }
            } else {
                // if not redrawn, just move to where it should end
                drawer.cur_pos = (area.width, area.height - 1);
            }
        }

        // clear everything else below unless we are already at the end
        if drawer.cur_pos != (area.width, area.height - 1) {
            queue!(drawer.writer, Clear(ClearType::FromCursorDown))?;
        }

        drawer.reset_colours()?;
        queue!(writer, cursor::RestorePosition)?;
        execute!(writer, crossterm::terminal::EndSynchronizedUpdate)?;

        self.dirty = false;
        prompt.dirty = false;
        buffer.dirty = false;
        status_bar.dirty = false;
        Ok(())
    }

}
