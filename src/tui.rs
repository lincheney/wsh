use std::default::Default;
use std::ops::DerefMut;
use std::str::FromStr;
use serde::{Deserialize, Deserializer, de};
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
    backend::Backend,
    buffer::Buffer,
};
use mlua::{prelude::*, UserData, UserDataMethods};
use crate::ui::Ui;
use crate::shell::Shell;

pub struct WidgetId(Ui, usize);

#[derive(Default)]
struct Widget{
    id: usize,
    constraint: Constraint,
    inner: Paragraph<'static>,
    align: Alignment,
    style: Style,
    border_style: Style,
    block: Block<'static>,
    persist: bool,
    hidden: bool,

    line_count: usize,
}

#[derive(Debug, Copy, Clone)]
pub struct SerdeWrap<T>(T);
impl<'de, T: FromStr> Deserialize<'de> for SerdeWrap<T>
    where <T as FromStr>::Err: std::fmt::Display
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let data = String::deserialize(deserializer)?;
        Ok(Self(T::from_str(&data).map_err(de::Error::custom)?))
    }
}

#[derive(Debug, Copy, Clone)]
pub struct SerdeConstraint(Constraint);
impl<'de> Deserialize<'de> for SerdeConstraint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let data = String::deserialize(deserializer)?;
        let constraint = if let Some(end) = data.strip_prefix("min:") {
            Constraint::Min(end.parse::<u16>().map_err(de::Error::custom)?)
        } else if let Some(end) = data.strip_prefix("max:") {
            Constraint::Max(end.parse::<u16>().map_err(de::Error::custom)?)
        } else if let Some(start) = data.strip_suffix("%") {
            Constraint::Percentage(start.parse::<u16>().map_err(de::Error::custom)?)
        } else {
            Constraint::Length(data.parse::<u16>().map_err(de::Error::custom)?)
        };
        Ok(Self(constraint))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TextStyleOptions {
    pub align: Option<SerdeWrap<Alignment>>,
    #[serde(flatten)]
    pub style: StyleOptions,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct TextOptions {
    pub text: String,
    #[serde(flatten)]
    pub style: TextStyleOptions,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TextParts {
    Single(String),
    Many(Vec<TextOptions>),
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct WidgetOptions {
    pub persist: Option<bool>,
    pub hidden: Option<bool>,
    pub text: Option<TextParts>,
    #[serde(flatten)]
    pub style: TextStyleOptions,
    pub border: Option<BorderOptions>,
    pub height: Option<SerdeConstraint>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct BorderTitleOptions {
    pub text: Option<String>,
    #[serde(flatten)]
    pub style: StyleOptions,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct BorderOptions {
    pub enabled: Option<bool>,
    pub r#type: Option<SerdeWrap<BorderType>>,
    pub title: Option<BorderTitleOptions>,
    #[serde(flatten)]
    pub style: StyleOptions,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StyleOptions {
    pub fg: Option<SerdeWrap<Color>>,
    pub bg: Option<SerdeWrap<Color>>,
    pub bold: Option<bool>,
    pub dim: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub strikethrough: Option<bool>,
    pub reversed: Option<bool>,
    pub blink: Option<bool>,
}

impl StyleOptions {
    fn apply_to_style(&self, mut style: Style) -> Style {
        if let Some(fg) = self.fg { style = style.fg(fg.0); }
        if let Some(bg) = self.bg { style = style.bg(bg.0); }

        macro_rules! set_modifier {
            ($field:ident, $enum:ident) => (
                if let Some($field) = self.$field {
                    let value = Modifier::$enum;
                    style = if $field { style.add_modifier(value) } else { style.remove_modifier(value) };
                }
            )
        }

        set_modifier!(bold, BOLD);
        set_modifier!(dim, DIM);
        set_modifier!(italic, ITALIC);
        set_modifier!(underline, UNDERLINED);
        set_modifier!(strikethrough, CROSSED_OUT);
        set_modifier!(reversed, REVERSED);
        set_modifier!(blink, SLOW_BLINK);
        style
    }
}

impl Widget {
    fn take_inner(&mut self) -> Paragraph<'static> {
        std::mem::replace(&mut self.inner, Paragraph::new(""))
    }

    fn set_options(&mut self, options: WidgetOptions) {
        if let Some(persist) = options.persist {
            self.persist = persist;
        }

        if let Some(hidden) = options.hidden {
            self.hidden = hidden;
        }

        if let Some(constraint) = options.height {
            self.constraint = constraint.0;
        }

        if let Some(text) = options.text {

            fn replace_tabs(mut text: String) -> String {
                let tab = "    ";
                if text.contains('\t') {
                    text = text.replace('\t', tab)
                }
                text
            }

            // there's no way to set the text on an existing paragraph ...
            let mut lines: Vec<_> = match text {
                TextParts::Single(text) => {
                    let text = replace_tabs(text);
                    text.split('\n').map(|l| l.to_owned()).map(Line::from).collect()
                },
                TextParts::Many(parts) => {
                    let mut lines = vec![Line::default()];
                    for part in parts.into_iter() {
                        let style = part.style.style.apply_to_style(Style::default());
                        let text = replace_tabs(part.text);
                        for (i, text) in text.split('\n').enumerate() {
                            if i > 0 {
                                lines.push(Line::default());
                            }
                            let line = lines.last_mut().unwrap();
                            line.spans.push(Span::styled(text.to_owned(), style));
                            line.alignment = part.style.align.map(|a| a.0);
                        }
                    }
                    lines
                },
            };

            lines.truncate(100);
            self.inner = Paragraph::new(lines);
        }

        if let Some(align) = options.style.align { self.align = align.0; }
        self.style = options.style.style.apply_to_style(self.style);

        match options.border {
            // explicitly disabled
            Some(BorderOptions{enabled: Some(false), ..}) => {
                self.block = Block::new();
            },
            Some(options) => {
                let mut block = Block::new();
                block = block.borders(Borders::ALL);
                block = block.border_style(options.style.apply_to_style(self.border_style));
                if let Some(t) = options.r#type { block = block.border_type(t.0); }
                if let Some(title) = options.title {
                    if let Some(t) = title.text {
                        block = block.title(t);
                    }
                    block = block.title_style(title.style.apply_to_style(self.border_style));
                }
                self.block = block;
            },
            None => {},
        }

        self.inner = self.take_inner()
            .alignment(self.align)
            .style(self.style)
            .block(self.block.clone())
            .wrap(Wrap{trim: false})
        ;
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

    pub fn add(&mut self, options: WidgetOptions) -> usize {
        let id = self.counter;
        self.counter += 1;
        self.dirty = true;

        let mut widget = Widget{ id, ..Default::default() };
        widget.set_options(options);
        self.widgets.push(widget);
        id
    }

    pub fn add_error_message(&mut self, message: String, options: Option<WidgetOptions>) -> usize {
        let mut options = options.unwrap_or_default();
        options.text = Some(TextParts::Single(message));
        // options.border.get_or_insert(Default::default());
        // options.border.as_mut().unwrap().enabled = Some(true);
        // options.border.as_mut().unwrap().style.fg = Some(SerdeWrap(Color::Red));
        // options.style.bg.get_or_insert(SerdeWrap(Color::Rgb(0x30, 0x30, 0x30)));
        options.style.style.fg.get_or_insert(SerdeWrap(Color::Red));
        self.add(options)
    }

    fn get_index(&self, id: usize) -> Option<usize> {
        for (i, w) in self.widgets.iter().enumerate() {
            match w.id.cmp(&id) {
                std::cmp::Ordering::Equal => return Some(i),
                std::cmp::Ordering::Greater => break,
                std::cmp::Ordering::Less => (),
            }
        }
        None
    }

    fn get_mut(&mut self, id: usize) -> Option<&mut Widget> {
        self.get_index(id).map(|i| {
            self.dirty = true;
            &mut self.widgets[i]
        })
    }

    fn remove(&mut self, id: usize) -> Option<Widget> {
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
        cursory: u16,
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

        let cursory = cursory + 1;
        self.old_buffer.area.y = cursory;
        self.new_buffer.area.y = cursory;

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

            let allocate_more_space = (cursory + self.height) as isize - height as isize;
            if allocate_more_space > 0 {
                // adjust cursory if new lines will be added below
                let y = self.old_buffer.area.y.saturating_sub(allocate_more_space as _);
                self.old_buffer.area.y = y;
                self.new_buffer.area.y = y;
                self.old_buffer.reset();
                queue!(stdout, Clear(ClearType::FromCursorDown))?;
            }

            let updates = self.old_buffer.diff(&self.new_buffer);

            if !updates.is_empty() {
                if allocate_more_space > 0 {
                    Ui::allocate_height(stdout, self.height)?;
                }
                queue!(
                    stdout,
                    cursor::SavePosition,
                    cursor::MoveToNextLine(1),
                    cursor::MoveToColumn(0),
                )?;

                let limit = self.old_buffer.area.y + self.height + (-allocate_more_space.min(0) as u16);
                let updates = updates.into_iter().filter(|(_, y, _)| *y < limit);
                self.terminal.backend_mut().draw(updates)?;
                queue!(stdout, cursor::RestorePosition)?;
            }
        }

        self.dirty = false;
        Ok(())
    }

}

impl UserData for WidgetId {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_async_method("exists", |_lua, id, _val: LuaValue| async move {
            Ok(id.0.borrow().await.tui.get_index(id.1).is_some())
        });

        methods.add_async_method_mut("set_options", |lua, mut id, val: LuaValue| async move {
            let id = id.deref_mut();
            if let Some(widget) = id.0.borrow_mut().await.tui.get_mut(id.1) {
                let options: WidgetOptions = lua.from_value(val)?;
                widget.set_options(options);
                Ok(())
            } else {
                Err(LuaError::RuntimeError(format!("can't find widget with id {}", id.1)))
            }
        });

        methods.add_async_method_mut("remove", |_lua, mut id, _val: LuaValue| async move {
            let id = id.deref_mut();
            if id.0.borrow_mut().await.tui.remove(id.1).is_some() {
                Ok(())
            } else {
                Err(LuaError::RuntimeError(format!("can't find widget with id {}", id.1)))
            }
        });
    }
}

async fn show_message(mut ui: Ui, _shell: Shell, lua: Lua, val: LuaValue) -> Result<WidgetId> {
    let options: WidgetOptions = lua.from_value(val)?;
    let id = ui.borrow_mut().await.tui.add(options);
    Ok(WidgetId(ui, id))
}

async fn clear_messages(mut ui: Ui, _shell: Shell, _lua: Lua, all: bool) -> Result<()> {
    let tui = &mut ui.borrow_mut().await.tui;
    if all {
        tui.clear_all();
    } else {
        tui.clear_non_persistent();
    }
    Ok(())
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("show_message", shell, show_message).await?;
    ui.set_lua_async_fn("clear_messages", shell, clear_messages).await?;

    Ok(())
}
