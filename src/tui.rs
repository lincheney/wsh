use std::default::Default;
use std::str::FromStr;
use serde::{Deserialize, Deserializer, de};
use anyhow::Result;
use crossterm::{
    cursor,
    style,
    queue,
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
}

macro_rules! make_serde {
    ($inner:ident) => (
        paste::paste! {
            #[derive(Debug, Copy, Clone)]
            struct [<Serde $inner>]($inner);
            impl<'de> Deserialize<'de> for [<Serde $inner>] {
                fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                    let data = String::deserialize(deserializer)?;
                    Ok([<Serde $inner>]($inner::from_str(&data).map_err(de::Error::custom)?))
                }
            }
        }
    )
}

make_serde!(Color);
make_serde!(Alignment);
make_serde!(BorderType);

#[derive(Debug, Copy, Clone)]
struct SerdeConstraint(Constraint);
impl<'de> Deserialize<'de> for SerdeConstraint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let data = String::deserialize(deserializer)?;
        let constraint = if data.starts_with("min:") {
            Constraint::Min(data[4..].parse::<u16>().map_err(de::Error::custom)?)
        } else if data.starts_with("max:") {
            Constraint::Max(data[4..].parse::<u16>().map_err(de::Error::custom)?)
        } else if data.ends_with("%") {
            Constraint::Percentage(data[..data.len()-1].parse::<u16>().map_err(de::Error::custom)?)
        } else {
            Constraint::Length(data.parse::<u16>().map_err(de::Error::custom)?)
        };
        Ok(SerdeConstraint(constraint))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WidgetOptions {
    text: Option<String>,
    persist: Option<bool>,
    align: Option<SerdeAlignment>,
    #[serde(flatten)]
    style: StyleOptions,
    border: Option<BorderOptions>,
    height: Option<SerdeConstraint>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BorderOptions {
    enabled: Option<bool>,
    r#type: Option<SerdeBorderType>,
    title: Option<String>,
    #[serde(flatten)]
    style: StyleOptions,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct StyleOptions {
    fg: Option<SerdeColor>,
    bg: Option<SerdeColor>,
    bold: Option<bool>,
    dim: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strikethrough: Option<bool>,
    reversed: Option<bool>,
    blink: Option<bool>,
}

impl StyleOptions {
    fn apply_to_style(&self, mut style: Style) -> Style {
        if let Some(fg) = self.fg { style = style.fg(fg.0); }
        if let Some(bg) = self.bg { style = style.fg(bg.0); }

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

        if let Some(constraint) = options.height {
            self.constraint = constraint.0;
        }

        if let Some(text) = options.text {
            // there's no way to set the text on an existing paragraph ...
            self.inner = Paragraph::new(text);
        }

        if let Some(align) = options.align { self.align = align.0; }
        self.style = options.style.apply_to_style(self.style);

        match options.border {
            // explicitly disabled
            Some(BorderOptions{enabled: Some(false), ..}) => {
                self.block = Block::new();
            },
            Some(options) => {
                let mut block = std::mem::replace(&mut self.block, Block::new());
                block = block.borders(Borders::ALL);
                block = block.border_style(options.style.apply_to_style(self.border_style));
                if let Some(t) = options.r#type { block = block.border_type(t.0); }
                if let Some(t) = options.title { block = block.title(t); }
                self.block = block;
            },
            None => {},
        }

        self.inner = self.take_inner()
            .alignment(self.align)
            .style(self.style)
            .block(self.block.clone())
        ;
    }
}

pub struct Tui {
    terminal: ratatui::DefaultTerminal,
    counter: usize,
    widgets: Vec<Widget>,

    dirty: bool,
    width: u16,
    height: u16,

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

            old_buffer: Default::default(),
            new_buffer: Default::default(),
        }
    }
}

impl Tui {

    fn add(&mut self, options: WidgetOptions) -> usize {
        let id = self.counter;
        self.counter += 1;
        self.dirty = true;

        let mut widget = Widget::default();
        widget.id = id;
        widget.set_options(options);
        self.widgets.push(widget);
        id
    }

    fn get_index(&self, id: usize) -> Option<usize> {
        for (i, w) in self.widgets.iter().enumerate() {
            if w.id == id {
                return Some(i)
            } else if w.id > id {
                break
            }
        }
        return None
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

    pub fn clear_non_persistent(&mut self) {
        self.widgets.retain(|w| w.persist);
    }

    fn refresh(&mut self, width: u16, height: u16) {
        self.dirty = false;
        self.width = width;
        self.height = height;

        let mut area = Rect{
            x: 0,
            y: 0,
            width,
            height,
        };

        self.old_buffer.resize(area);
        self.new_buffer.resize(area);

        let mut frame = self.terminal.get_frame();
        std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);

        // assume each widget needs at least 1 line
        let widgets = &self.widgets[..self.widgets.len().min(area.height as _)];

        let mut max_height = 0;
        for w in widgets.iter() {
            max_height += w.inner.line_count(width);
            if max_height >= area.height as _ {
                break
            }
        }
        area.height = area.height.min(max_height as _);

        let layout = Layout::vertical(widgets.iter().map(|w| w.constraint));
        let layouts = layout.split(area);

        for (widget, layout) in widgets.iter().zip(layouts.iter()) {
            frame.render_widget(&widget.inner, *layout);
        }
        std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);
    }

    fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.new_buffer, &mut self.old_buffer);
    }

    pub fn draw(&mut self, stdout: &mut std::io::Stdout, width: u16, height: u16, cursory: u16) -> Result<()> {
        if self.widgets.is_empty() {
            return Ok(())
        }

        let max_height = height * 2 / 3;

        if self.dirty || max_height != self.height || width != self.width {
            self.swap_buffers();
            self.new_buffer.reset();
            self.refresh(width, max_height);
        }

        let cursory = cursory + 1;
        self.old_buffer.area.y = cursory;
        self.new_buffer.area.y = cursory;

        let actual_height = {
            let trailing_empty_lines = self.new_buffer.content()
                .chunks(self.new_buffer.area.width as _)
                .rev()
                .take_while(|line| line.iter().all(|c| *c == ratatui::buffer::Cell::EMPTY))
                .count();
            self.new_buffer.area.height - trailing_empty_lines as u16
        };

        if actual_height > 0 {

            let allocate_more_space = (cursory + actual_height + 1).saturating_sub(height);
            if allocate_more_space > 0 {
                let y = self.old_buffer.area.y.saturating_sub(allocate_more_space - 1);
                self.old_buffer.area.y = y;
                self.new_buffer.area.y = y;
                self.old_buffer.reset();
            }

            let updates = self.old_buffer.diff(&self.new_buffer);
            if !updates.is_empty() {
                if allocate_more_space > 0 {
                    for _ in 0 .. actual_height as _ {
                        queue!(stdout, style::Print("\n"))?;
                    }
                    queue!(stdout, cursor::MoveUp(actual_height))?;
                }
                queue!(stdout, cursor::SavePosition, cursor::MoveToNextLine(1))?;
                self.terminal.backend_mut().draw(updates.into_iter())?;
                queue!(stdout, cursor::RestorePosition)?;
            }
        }

        Ok(())
    }

}

impl UserData for WidgetId {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {

        methods.add_async_method("set_options", |lua, id, val: LuaValue| async move {
            if let Some(widget) = id.0.borrow_mut().await.tui.get_mut(id.1) {
                let options: WidgetOptions = lua.from_value(val)?;
                widget.set_options(options);
                Ok(())
            } else {
                Err(LuaError::RuntimeError(format!("can't find widget with id {}", id.1)))
            }
        });

        methods.add_async_method("remove", |_lua, id, _val: LuaValue| async move {
            if let Some(_) = id.0.borrow_mut().await.tui.remove(id.1) {
                Ok(())
            } else {
                Err(LuaError::RuntimeError(format!("can't find widget with id {}", id.1)))
            }
        });
    }
}

async fn show_message(
    ui: Ui,
    _shell: Shell,
    lua: Lua,
    val: LuaValue,
) -> Result<WidgetId> {
    let options: WidgetOptions = lua.from_value(val)?;
    let id = ui.borrow_mut().await.tui.add(options);
    Ok(WidgetId(ui, id))
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("show_message", shell, show_message).await?;

    Ok(())
}
