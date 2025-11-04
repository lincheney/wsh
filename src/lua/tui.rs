use std::default::Default;
use std::str::FromStr;
use serde::{Deserialize, Deserializer, de};
use anyhow::Result;
use ratatui::{
    text::*,
    layout::*,
    widgets::*,
    style::*,
};
use mlua::{prelude::*};
use crate::ui::{Ui, ThreadsafeUiInner};
use crate::tui;

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
struct CommonWidgetOptions {
    id: Option<usize>,
    pub persist: Option<bool>,
    pub hidden: Option<bool>,
    #[serde(flatten)]
    pub style: TextStyleOptions,
    pub border: Option<BorderOptions>,
    pub height: Option<SerdeConstraint>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct WidgetOptions {
    #[serde(flatten)]
    options: CommonWidgetOptions,
    pub text: Option<TextParts>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AnsiWidgetOptions {
    #[serde(flatten)]
    options: CommonWidgetOptions,
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

#[derive(Debug, Deserialize)]
pub struct UnderlineStyleOptions {
    color: SerdeWrap<Color>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum UnderlineOptions {
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
    pub underline: Option<UnderlineOptions>,
    pub strikethrough: Option<bool>,
    pub reversed: Option<bool>,
    pub blink: Option<bool>,
}

impl From<StyleOptions> for tui::StyleOptions {
    fn from(style: StyleOptions) -> Self {
        Self {
            fg: style.fg.map(|x| x.0),
            bg: style.bg.map(|x| x.0),
            bold: style.bold,
            dim: style.dim,
            italic: style.italic,
            underline: match style.underline {
                None | Some(UnderlineOptions::Bool(false)) => None,
                Some(UnderlineOptions::Bool(true)) => Some(None),
                Some(UnderlineOptions::Options(opts)) => Some(Some(opts.color.0)),
            },
            strikethrough: style.strikethrough,
            reversed: style.reversed,
            blink: style.blink,
       }
    }
}

fn set_widget_text(widget: &mut tui::Widget, text: Option<TextParts>) {
    if let Some(text) = text {

        // there's no way to set the text on an existing paragraph ...
        let mut lines: Vec<_> = match text {
            TextParts::Single(text) => {
                let text = tui::Widget::replace_tabs(text);
                text.split('\n').map(|l| l.to_owned()).map(Line::from).collect()
            },
            TextParts::Many(parts) => {
                let mut lines = vec![Line::default()];
                for part in parts {
                    let style: tui::StyleOptions = part.style.style.into();
                    let style = style.as_style();
                    let text = tui::Widget::replace_tabs(part.text);
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
        widget.inner = Text::default();
        widget.inner.lines = lines;
    }
}

fn set_widget_options(widget: &mut tui::Widget, options: CommonWidgetOptions) {
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
        widget.inner = std::mem::take(&mut widget.inner).alignment(align.0);
    }

    match options.border {
        // explicitly disabled
        Some(BorderOptions{enabled: Some(false), ..}) => {
            widget.block = None;
        },
        Some(options) => {
            let style: tui::StyleOptions = options.style.into();
            widget.border_style = widget.border_style.patch(style.as_style());
            widget.border_type = options.r#type.unwrap_or(SerdeWrap(widget.border_type)).0;

            let mut block = if let Some(title) = options.title {
                let style: tui::StyleOptions = title.style.into();
                widget.border_title_style = widget.border_title_style.patch(style.as_style());
                if let Some(text) = title.text {
                    Some(Block::new().title(text))
                } else {
                    widget.block.clone()
                }
            } else {
                widget.block.clone()
            }.unwrap_or_else(Block::new);

            block = block.borders(Borders::ALL);
            block = block.border_style(widget.border_style);
            block = block.border_type(widget.border_type);
            block = block.title_style(widget.border_title_style);

            widget.block = Some(block);
        },
        None => {},
    }

    widget.style = widget.style.merge(&options.style.style.into());
    widget.inner = std::mem::take(&mut widget.inner).style(widget.style.as_style());
    widget.block = std::mem::take(&mut widget.block).map(|b| b.style(widget.style.as_style()));

}

async fn set_message(mut ui: Ui, lua: Lua, val: LuaValue) -> Result<usize> {
    let options: Option<WidgetOptions> = lua.from_value(val)?;

    let tui = &mut ui.inner.borrow_mut().await.tui;
    let (id, widget) = match options.as_ref().and_then(|o| o.options.id).map(|id| (id, tui.get_mut(id))) {
        Some((id, Some(widget))) => (id, widget),
        None => {
            let widget = tui::Widget::default();
            tui.add(widget.into())
        },
        Some((id, None)) => return Err(anyhow::anyhow!("can't find widget with id {}", id)),
    };

    if let Some(options) = options {
        set_widget_text(widget.as_mut(), options.text);
        set_widget_options(widget.as_mut(), options.options);
    }
    tui.dirty = true;
    Ok(id)
}

async fn clear_messages(mut ui: Ui, _lua: Lua, all: bool) -> Result<()> {
    let tui = &mut ui.inner.borrow_mut().await.tui;
    if all {
        tui.clear_all();
    } else {
        tui.clear_non_persistent();
    }
    Ok(())
}

async fn check_message(ui: Ui, _lua: Lua, id: usize) -> Result<bool> {
    Ok(ui.inner.borrow().await.tui.get_index(id).is_some())
}

async fn remove_message(mut ui: Ui, _lua: Lua, id: usize) -> Result<()> {
    let tui = &mut ui.inner.borrow_mut().await.tui;
    if tui.remove(id).is_some() {
        tui.dirty = true;
        Ok(())
    } else {
        Err(anyhow::anyhow!("can't find widget with id {}", id))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Highlight {
    start: usize,
    finish: usize,
    #[serde(flatten)]
    style: StyleOptions,
    namespace: Option<usize>,
}

async fn add_buf_highlight(mut ui: Ui, lua: Lua, val: LuaValue) -> Result<()> {
    let hl: Highlight = lua.from_value(val)?;
    let style: tui::StyleOptions = hl.style.into();

    ui.inner.borrow_mut().await.buffer.highlights.push(crate::buffer::Highlight{
        start: hl.start,
        end: hl.finish,
        style: style.as_style(),
        namespace: hl.namespace.unwrap_or(0),
    });

    Ok(())
}

async fn clear_buf_highlights(mut ui: Ui, _lua: Lua, namespace: Option<usize>) -> Result<()> {
    let mut ui = ui.inner.borrow_mut().await;
    if let Some(namespace) = namespace {
        ui.buffer.highlights.retain(|h| h.namespace != namespace);
    } else {
        ui.buffer.highlights.clear();
    }
    Ok(())
}

async fn add_buf_highlight_namespace(mut ui: Ui, _lua: Lua, _val: ()) -> Result<usize> {
    let mut ui = ui.inner.borrow_mut().await;
    ui.buffer.highlight_counter += 1;
    Ok(ui.buffer.highlight_counter)
}

async fn set_ansi_message(mut ui: Ui, lua: Lua, val: LuaValue) -> Result<usize> {
    let options: Option<AnsiWidgetOptions> = lua.from_value(val)?;

    let tui = &mut ui.inner.borrow_mut().await.tui;
    let (id, widget) = match options.as_ref().and_then(|o| o.options.id).map(|id| (id, tui.get_mut(id))) {
        Some((id, Some(widget))) => (id, widget),
        None => {
            let widget = tui::ansi::Parser::default();
            tui.add(widget.into())
        },
        Some((id, None)) => return Err(anyhow::anyhow!("can't find widget with id {}", id)),
    };

    if let Some(options) = options {
        set_widget_options(widget.as_mut(), options.options);
    }
    tui.dirty = true;
    Ok(id)
}

async fn feed_ansi_message(mut ui: Ui, _lua: Lua, (id, value): (usize, LuaString)) -> Result<()> {
    let tui = &mut ui.inner.borrow_mut().await.tui;

    match tui.get_mut(id) {
        Some(tui::WidgetWrapper::Ansi(parser)) => {
            parser.feed((&*value.as_bytes()).into());
            tui.dirty = true;
            Ok(())
        },
        _ => Err(anyhow::anyhow!("can't find widget with id {}", id)),
    }
}

async fn clear_ansi_message(mut ui: Ui, _lua: Lua, id: usize) -> Result<()> {
    let tui = &mut ui.inner.borrow_mut().await.tui;

    match tui.get_mut(id) {
        Some(tui::WidgetWrapper::Ansi(parser)) => {
            parser.clear();
            tui.dirty = true;
            Ok(())
        },
        _ => Err(anyhow::anyhow!("can't find widget with id {}", id)),
    }
}

async fn set_status_bar(mut ui: Ui, lua: Lua, val: LuaValue) -> Result<()> {
    let options: Option<WidgetOptions> = lua.from_value(val)?;
    let mut ui = ui.inner.borrow_mut().await;
    let widget = ui.status_bar.inner.get_or_insert_default();
    if let Some(options) = options {
        set_widget_text(widget, options.text);
        set_widget_options(widget, options.options);
    }
    ui.status_bar.dirty = true;
    Ok(())
}

async fn allocate_height(_ui: Ui, _lua: Lua, height: u16) -> Result<()> {
    crate::tui::allocate_height(&mut std::io::stdout(), height)
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
    ui.set_lua_async_fn("set_ansi_message", set_ansi_message)?;
    ui.set_lua_async_fn("feed_ansi_message", feed_ansi_message)?;
    ui.set_lua_async_fn("clear_ansi_message", clear_ansi_message)?;
    ui.set_lua_async_fn("set_status_bar", set_status_bar)?;

    Ok(())
}

