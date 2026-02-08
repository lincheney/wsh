use bstr::BString;
use std::default::Default;
use serde::{Deserialize, Deserializer, de};
use anyhow::Result;
use ratatui::{
    layout::*,
    widgets::*,
    style::*,
};
use mlua::{prelude::*};
use crate::ui::{Ui};
use crate::tui;
use super::SerdeWrap;

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
    pub text: Option<String>,
    #[serde(flatten)]
    pub style: TextStyleOptions,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum TextParts {
    Single(String),
    Detailed(TextOptions),
    Many(Vec<TextOptions>),
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct CommonWidgetOptions {
    id: Option<usize>,
    pub persist: Option<bool>,
    pub hidden: Option<bool>,
    #[serde(flatten)]
    pub style: TextStyleOptions,
    pub border: Option<BorderOptions>,
    pub height: Option<SerdeConstraint>,
    // ansi options
    pub show_cursor: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct WidgetOptions {
    #[serde(flatten)]
    options: CommonWidgetOptions,
    pub text: Option<TextParts>,
}

#[derive(Clone, Copy, Debug, Default, strum::EnumString)]
pub enum BorderSide {
    Top,
    Right,
    Bottom,
    Left,
    #[default]
    All,
}

impl From<BorderSide> for Borders {
    fn from(val: BorderSide) -> Self {
        match val {
            BorderSide::Top => Borders::TOP,
            BorderSide::Right => Borders::RIGHT,
            BorderSide::Bottom => Borders::BOTTOM,
            BorderSide::Left => Borders::LEFT,
            BorderSide::All => Borders::ALL,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum BorderSides {
    Single(SerdeWrap<BorderSide>),
    Multiple(Vec<SerdeWrap<BorderSide>>),
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct BorderOptions {
    pub enabled: Option<bool>,
    pub sides: Option<BorderSides>,
    pub r#type: Option<SerdeWrap<BorderType>>,
    pub title: Option<TextParts>,
    pub show_empty: Option<bool>,
    #[serde(flatten)]
    pub style: StyleOptions,
}

#[derive(Debug, Deserialize)]
pub struct UnderlineStyleOptions {
    color: SerdeWrap<Color>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum UnderlineOption {
    Bool(bool),
    Options(UnderlineStyleOptions),
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct StyleOptions {
    pub fg: Option<SerdeWrap<Color>>,
    pub bg: Option<SerdeWrap<Color>>,
    pub bold: Option<bool>,
    pub dim: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<UnderlineOption>,
    pub strikethrough: Option<bool>,
    pub reversed: Option<bool>,
    pub blink: Option<bool>,
}

impl StyleOptions {
    fn is_default(&self) -> bool {
        self.fg.is_none() &&
            self.bg.is_none() &&
            self.bold.is_none() &&
            self.dim.is_none() &&
            self.italic.is_none() &&
            self.underline.is_none() &&
            self.strikethrough.is_none() &&
            self.reversed.is_none() &&
            self.blink.is_none()
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BufferStyleOptions {
    #[serde(flatten)]
    inner: StyleOptions,
    no_blend: bool,
}

impl From<StyleOptions> for tui::widget::StyleOptions {
    fn from(style: StyleOptions) -> Self {
        Self {
            fg: style.fg.map(|x| x.0),
            bg: style.bg.map(|x| x.0),
            bold: style.bold,
            dim: style.dim,
            italic: style.italic,
            underline: match style.underline {
                None => None,
                Some(UnderlineOption::Bool(false)) => Some(tui::widget::UnderlineOption::None),
                Some(UnderlineOption::Bool(true)) => Some(tui::widget::UnderlineOption::Set),
                Some(UnderlineOption::Options(opts)) => Some(tui::widget::UnderlineOption::Color(opts.color.0)),
            },
            strikethrough: style.strikethrough,
            reversed: style.reversed,
            blink: style.blink,
       }
    }
}

fn parse_text_parts<T: Default+Clone>(parts: TextParts, text: &mut tui::text::Text<T>) {
    match parts {
        TextParts::Single(part) => {
            text.clear();
            text.push_lines(part.split('\n').map(|s| s.into()), None);
        },
        TextParts::Detailed(part) => {
            let hl = if part.style.style.is_default() {
                None
            } else {
                let style: tui::widget::StyleOptions = part.style.style.into();
                Some(style.as_style().into())
            };

            if let Some(string) = part.text {
                text.clear();
                text.push_lines(string.split('\n').map(|s| s.into()),hl);
            } else if let Some(hl) = hl {
                for lineno in 0 .. text.get().len() {
                    text.add_highlight(tui::text::HighlightedRange{
                        lineno,
                        start: 0,
                        end: text.get()[lineno].len(),
                        inner: hl.clone(),
                    });
                }
            }
        },
        TextParts::Many(parts) => {
            text.clear();
            text.push_line(b"".into(), None);
            for part in parts {
                if let Some(string) = part.text {
                    let hl = if part.style.style.is_default() {
                        None
                    } else {
                        let style: tui::widget::StyleOptions = part.style.style.into();
                        Some(style.as_style().into())
                    };

                    for (i, string) in string.split('\n').enumerate() {
                        if i > 0 {
                            text.push_line(b"".into(), None);
                        }
                        text.push_str(string.into(), hl.clone());
                    }
                }
            }
        },
    }
}

fn set_widget_options(widget: &mut tui::widget::Widget, options: CommonWidgetOptions) {
    if let Some(persist) = options.persist {
        widget.persist = persist;
    }

    if let Some(hidden) = options.hidden {
        widget.hidden = hidden;
    }

    if let Some(constraint) = options.height {
        widget.constraint = Some(constraint.0);
    }

    if let Some(align) = options.style.align {
        widget.inner.alignment = align.0;
    }

    if let Some(show_cursor) = options.show_cursor {
        widget.ansi_show_cursor = show_cursor;
    }

    match options.border {
        // explicitly disabled
        Some(BorderOptions{enabled: Some(false), ..}) => {
            widget.block = None;
        },
        Some(options) => {
            let style: tui::widget::StyleOptions = options.style.into();
            widget.border_style = widget.border_style.patch(style.as_style());
            widget.border_type = options.r#type.unwrap_or(SerdeWrap(widget.border_type)).0;
            widget.border_show_empty = options.show_empty.unwrap_or(widget.border_show_empty);

            let border_sides = match options.sides {
                Some(BorderSides::Single(b)) => b.0.into(),
                Some(BorderSides::Multiple(b)) => b.iter().map(|x| x.0.into()).reduce(|x: Borders, y| x.union(y)).unwrap_or(Borders::ALL),
                None => widget.border_sides.unwrap_or(Borders::ALL),
            };
            widget.border_sides = Some(border_sides);

            let mut block = options.title
                .map(|title| {
                    let text = widget.border_title.get_or_insert_default();
                    parse_text_parts(title, text);
                    Block::new().title(text as &tui::text::Text)
                })
                .or_else(|| widget.block.clone())
                .unwrap_or_else(Block::new);

            block = block.borders(border_sides);
            block = block.border_style(widget.border_style);
            block = block.border_type(widget.border_type);

            widget.block = Some(block);
        },
        None => {},
    }

    widget.style = widget.style.merge(&options.style.style.into());
    widget.inner.style = widget.style.as_style();
    widget.block = std::mem::take(&mut widget.block).map(|b| b.style(widget.style.as_style()));
}

async fn set_message(ui: Ui, lua: Lua, val: LuaValue) -> Result<usize> {
    let options: Option<WidgetOptions> = lua.from_value(val)?;

    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;
    let (id, widget) = match options.as_ref().and_then(|o| o.options.id).map(|id| (id, tui.get_mut(id))) {
        Some((id, Some(widget))) => (id, widget),
        None => {
            let widget = tui::widget::Widget::default();
            tui.add(widget)
        },
        Some((id, None)) => anyhow::bail!("can't find widget with id {}", id),
    };

    if let Some(options) = options {
        if let Some(text) = options.text {
            parse_text_parts(text, &mut widget.inner);
        }
        set_widget_options(widget, options.options);
    }
    tui.dirty = true;
    Ok(id)
}

async fn clear_messages(ui: Ui, _lua: Lua, all: bool) -> Result<()> {
    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;
    if all {
        tui.clear_all();
    } else {
        tui.clear_non_persistent();
    }
    Ok(())
}

async fn check_message(ui: Ui, _lua: Lua, id: usize) -> Result<bool> {
    Ok(ui.get().borrow().tui.get_index(id).is_some())
}

async fn remove_message(ui: Ui, _lua: Lua, id: usize) -> Result<()> {
    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;
    if tui.remove(id).is_some() {
        tui.dirty = true;
        Ok(())
    } else {
        anyhow::bail!("can't find widget with id {}", id)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BufferHighlight {
    start: usize,
    finish: usize,
    #[serde(flatten)]
    style: BufferStyleOptions,
    virtual_text: Option<BString>,
    conceal: Option<bool>,
    namespace: Option<usize>,
}

async fn add_buf_highlight(ui: Ui, lua: Lua, val: LuaValue) -> Result<()> {
    let ui = ui.get();
    let hl: BufferHighlight = lua.from_value(val)?;
    let blend = !hl.style.no_blend;
    let style: tui::widget::StyleOptions = hl.style.inner.into();

    ui.borrow_mut().buffer.add_highlight(tui::text::HighlightedRange{
        lineno: 0,
        start: hl.start.saturating_sub(1),
        end: hl.finish,
        inner: tui::text::Highlight{
            style: style.as_style(),
            namespace: hl.namespace.unwrap_or(0),
            virtual_text: hl.virtual_text,
            conceal: hl.conceal,
            blend,
        },
    });

    Ok(())
}

async fn clear_buf_highlights(ui: Ui, _lua: Lua, namespace: Option<usize>) -> Result<()> {
    let ui = ui.get();
    let mut ui = ui.borrow_mut();
    if let Some(namespace) = namespace {
        ui.buffer.clear_highlights_in_namespace(namespace);
    } else {
        ui.buffer.clear_highlights();
    }
    Ok(())
}

async fn add_buf_highlight_namespace(ui: Ui, _lua: Lua, _val: ()) -> Result<usize> {
    let ui = ui.get();
    let mut ui = ui.borrow_mut();
    ui.buffer.highlight_counter += 1;
    Ok(ui.buffer.highlight_counter)
}

async fn feed_ansi_message(ui: Ui, _lua: Lua, (id, value): (usize, LuaString)) -> Result<()> {
    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;

    match tui.get_mut(id) {
        Some(widget) => {
            widget.feed_ansi((&*value.as_bytes()).into());
            tui.dirty = true;
            Ok(())
        },
        _ => anyhow::bail!("can't find widget with id {}", id),
    }
}

async fn clear_message(ui: Ui, _lua: Lua, id: usize) -> Result<()> {
    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;

    match tui.get_mut(id) {
        Some(widget) => {
            widget.clear();
            tui.dirty = true;
            Ok(())
        },
        _ => anyhow::bail!("can't find widget with id {}", id),
    }
}

async fn get_message_text(ui: Ui, _lua: Lua, id: usize) -> Result<Vec<BString>> {
    let ui = ui.get();
    let tui = &ui.borrow().tui;

    match tui.get(id) {
        Some(msg) => Ok(msg.inner.get().into()),
        _ => anyhow::bail!("can't find widget with id {}", id),
    }
}

async fn message_to_ansi_string(ui: Ui, _lua: Lua, (id, width): (usize, Option<u16>)) -> Result<mlua::BString> {
    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;

    match tui.render_to_string(id, width) {
        None => anyhow::bail!("can't find widget with id {}", id),
        Some(x) => Ok(x),
    }
}

async fn set_status_bar(ui: Ui, lua: Lua, val: LuaValue) -> Result<()> {
    let ui = ui.get();
    let options: Option<WidgetOptions> = lua.from_value(val)?;
    let mut ui = ui.borrow_mut();
    if let Some(options) = options {
        let widget = ui.status_bar.inner.get_or_insert_default();
        if let Some(text) = options.text {
            widget.inner.clear();
            parse_text_parts(text, &mut widget.inner);
        }
        set_widget_options(widget, options.options);
    }
    ui.status_bar.dirty = true;
    Ok(())
}

async fn allocate_height(_ui: Ui, _lua: Lua, height: u16) -> Result<()> {
    tui::allocate_height(&mut std::io::stdout(), height)?;
    Ok(())
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    ui.set_lua_async_fn("allocate_height", allocate_height)?;
    ui.set_lua_async_fn("set_message", set_message)?;
    ui.set_lua_async_fn("check_message", check_message)?;
    ui.set_lua_async_fn("remove_message", remove_message)?;
    ui.set_lua_async_fn("clear_messages", clear_messages)?;
    ui.set_lua_async_fn("add_buf_highlight_namespace", add_buf_highlight_namespace)?;
    ui.set_lua_async_fn("add_buf_highlight", add_buf_highlight)?;
    ui.set_lua_async_fn("clear_buf_highlights", clear_buf_highlights)?;
    ui.set_lua_async_fn("feed_ansi_message", feed_ansi_message)?;
    ui.set_lua_async_fn("clear_message", clear_message)?;
    ui.set_lua_async_fn("get_message_text", get_message_text)?;
    ui.set_lua_async_fn("message_to_ansi_string", message_to_ansi_string)?;
    ui.set_lua_async_fn("set_status_bar", set_status_bar)?;

    Ok(())
}

