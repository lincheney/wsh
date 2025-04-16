use std::default::Default;
use anyhow::Result;
use crossterm::{
    cursor,
    queue,
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

    pub fn is_empty(&self) -> bool {
        self.fg.is_none()
        && self.bg.is_none()
        && self.bold.is_none()
        && self.dim.is_none()
        && self.italic.is_none()
        && self.underline.is_none()
        && self.strikethrough.is_none()
        && self.reversed.is_none()
        && self.blink.is_none()
    }
}

#[derive(Default, Debug)]
pub struct Widget{
    id: usize,
    pub constraint: Constraint,
    pub inner: Option<Text<'static>>,
    pub align: Alignment,
    pub style: StyleOptions,
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

    pub fn make_text(&mut self) {
        let p = self.inner.take().unwrap_or_else(|| Text::default());
        let p = p
            .alignment(self.align)
            .style(self.style.as_style())
            // .block(self.block.clone())
            // .wrap(Wrap{trim: false})
        ;
        self.inner = Some(p);
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

    pub fn flush(&mut self) {
        match self {
            Self::Widget(_) => (),
            Self::Ansi(p) => p.flush(),
        }
    }
}

impl Into<WidgetWrapper> for Widget {
    fn into(self) -> WidgetWrapper {
        WidgetWrapper::Widget(self)
    }
}

impl Into<WidgetWrapper> for ansi::Parser {
    fn into(self) -> WidgetWrapper {
        WidgetWrapper::Ansi(self)
    }
}

pub struct Tui {
    terminal: ratatui::DefaultTerminal,
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
            line_count_buffer: Default::default(),
        }
    }
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
        let mut text = Text::default().fg(Color::Red);
        text.lines = message;
        widget.inner = Some(text);
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

        // let mut frame = self.terminal.get_frame();
        // std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);

        let mut max_height = 0;
        let mut last_widget = 0;
        for (i, w) in self.widgets.iter_mut().enumerate() {
            let w = w.as_mut();
            match &w.inner {
                Some(inner) if !w.hidden => {
                    w.line_count = Self::line_count(&mut self.line_count_buffer, inner, &w.block, width, height).unwrap_or(height) as _;
                    // w.line_count = inner.line_count(width);
                    if let Constraint::Min(min) = w.constraint {
                        w.line_count = w.line_count.max(min as _);
                    }

                    max_height += w.line_count;
                    last_widget = i;
                    if max_height >= area.height as _ {
                        break
                    }
                },
                _ => (),
            }
        }

        let widgets = &self.widgets[..=last_widget];
        area.height = area.height.min(max_height as _);

        let filter = |w: &&Widget| !w.hidden && w.line_count > 0 && w.inner.is_some();

        let layout = Layout::vertical(widgets.iter().map(|w| w.as_ref()).filter(filter).map(|w| w.constraint));
        let layouts = layout.split(area);

        for (widget, layout) in widgets.iter().map(|w| w.as_ref()).filter(filter).zip(layouts.iter()) {
            Self::render(widget.inner.as_ref().unwrap(), &widget.block, *layout, &mut self.new_buffer, true);
            // frame.render_widget(&widget.inner, *layout);
        }

        // std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);
    }

    fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.new_buffer, &mut self.old_buffer);
    }

    fn render(text: &Text, block: &Block, area: Rect, buffer: &mut Buffer, style: bool) {
        use ratatui::widgets::Widget;
        buffer.set_style(area, text.style);
        block.render(area, buffer);

        let inner = block.inner(area);
        let mut y_offset = 0;

        for line in text.iter() {
            let mut x_offset = 0;
            for graph in line.styled_graphemes(Style::default()) {

                use unicode_width::UnicodeWidthStr;
                let width = graph.symbol.width();
                if width == 0 {
                    continue
                }
                let symbol = if graph.symbol.is_empty() { " " } else { graph.symbol };
                let cell = &mut buffer[(inner.left() + x_offset, inner.top() + y_offset)];
                cell.set_symbol(symbol);
                if style {
                    cell.set_style(graph.style);
                }
                x_offset += width as u16;

                if x_offset >= inner.width {
                    x_offset = 0;
                    y_offset += 1;
                    if y_offset >= inner.height {
                        break
                    }
                }
            }

            y_offset += 1;
            if y_offset >= inner.height {
                break
            }
        }
    }

    fn line_count(buffer: &mut Buffer, text: &Text, block: &Block, width: u16, max_height: u16) -> Option<u16> {
        if width < 1 {
            return Some(0);
        }

        let area = Rect{x: 0, y: 0, width, height: max_height};
        buffer.resize(area);
        Self::render(text, block, area, buffer, false);

        let height = buffer_nonempty_height(buffer);
        if height == max_height {
            None
        } else {
            Some(height)
        }
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

        self.height = buffer_nonempty_height(&self.new_buffer);
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
