use bstr::BString;
use std::default::Default;
use serde::{Deserialize, Serialize};
use anyhow::Result;
use crossterm::style::Color;
use mlua::{prelude::*};
use crate::ui::{Ui};
use crate::tui::{
    self,
    layout::{self, Node, NodeKind, Layout},
    Style,
    Modifier,
    text::Alignment,
    Cell,
    sizing,
};
use crate::tui::border;
use super::SerdeWrap;

#[derive(Debug, Clone, Copy)]
struct LuaColor(Color);

impl<'de> serde::Deserialize<'de> for LuaColor {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let string = String::deserialize(deserializer)?.to_lowercase();
        let color = match string.as_str() {
            "reset"             => Color::Reset,
            "default"           => Color::Reset,
            "black"             => Color::Black,
            "red"               => Color::DarkRed,
            "green"             => Color::DarkGreen,
            "yellow"            => Color::DarkYellow,
            "blue"              => Color::DarkBlue,
            "magenta"           => Color::DarkMagenta,
            "cyan"              => Color::DarkCyan,
            "gray"              => Color::Grey,
            "grey"              => Color::Grey,
            "darkgrey"          => Color::DarkGrey,
            "darkgray"          => Color::DarkGrey,
            "lightred"          => Color::Red,
            "lightgreen"        => Color::Green,
            "lightyellow"       => Color::Yellow,
            "lightblue"         => Color::Blue,
            "lightmagenta"      => Color::Magenta,
            "lightcyan"         => Color::Cyan,
            "white"             => Color::White,
            _ if string.starts_with('#') && string.len() == 7 => {
                let rgb = u32::from_str_radix(&string[1..7], 16).map_err(|e| serde::de::Error::custom(e.to_string()))?;
                let r = (rgb >> 16) as _;
                let g = ((rgb >> 8) & 0xff) as _;
                let b = (rgb & 0xff) as _;
                Color::Rgb{r, g, b}
            },
            _ => {
                // try numeric ANSI index
                if let Ok(n) = string.parse::<u8>() {
                    Color::AnsiValue(n)
                } else {
                    return Err(serde::de::Error::custom(format!("unknown color: {string}")))
                }
            },
        };
        Ok(LuaColor(color))
    }
}

impl serde::Serialize for LuaColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let string = match self.0 {
            Color::Reset              => "reset",
            Color::Black              => "black",
            Color::DarkRed            => "red",
            Color::DarkGreen          => "green",
            Color::DarkYellow         => "yellow",
            Color::DarkBlue           => "blue",
            Color::DarkMagenta        => "magenta",
            Color::DarkCyan           => "cyan",
            Color::Grey               => "gray",
            Color::DarkGrey           => "darkgray",
            Color::Red                => "lightred",
            Color::Green              => "lightgreen",
            Color::Yellow             => "lightyellow",
            Color::Blue               => "lightblue",
            Color::Magenta            => "lightmagenta",
            Color::Cyan               => "lightcyan",
            Color::White              => "white",
            Color::AnsiValue(n)       => &n.to_string(),
            Color::Rgb{r, g, b}       => &format!("#{r:02x}{g:02x}{b:02x}"),
        };
        serializer.serialize_str(&string)
    }
}

#[derive(Debug, Clone)]
struct LuaSizeMetric(sizing::Metric);

impl<'de> serde::Deserialize<'de> for LuaSizeMetric {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Debug, Clone, Deserialize)]
        #[serde(untagged)]
        enum Metric {
            Fixed(u16),
            Percent(String),
        }

        match Metric::deserialize(deserializer)? {
            Metric::Fixed(x) => Ok(LuaSizeMetric(sizing::Metric::Fixed(x))),
            Metric::Percent(x) => {
                if x.ends_with('%') && let Ok(x) = x.parse::<u16>() {
                    Ok(Self(sizing::Metric::Percent(x)))
                } else {
                    Err(serde::de::Error::custom(format!("invalid metric: {x}")))
                }
            },
        }
    }
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
struct TextStyleOptions {
    #[serde(flatten)]
    style: StyleOptions,
}

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
struct TextOptions {
    text: Option<String>,
    #[serde(flatten)]
    style: TextStyleOptions,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum TextParts {
    Single(String),
    Detailed(TextOptions),
    Many(Vec<TextOptions>),
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct MessageStyleOptions {
    align: Option<SerdeWrap<Alignment>>,
    #[serde(flatten)]
    style: TextStyleOptions,
    border: Option<BorderOptions>,
    // ansi options
    show_cursor: Option<bool>,
}

impl MessageStyleOptions {
    fn is_none(&self) -> bool {
        self.align.is_none()
            && self.style.style.is_none()
            && self.border.is_none()
            && self.show_cursor.is_none()
    }
}

#[derive(Debug, Deserialize)]
#[allow(nonstandard_style)]
enum Direction {
    vertical,
    horizontal,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum LayoutChild {
    Message(MessageOptions),
    WidgetRef(usize),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MessageInner {
    Layout{
        direction: Direction,
        children: Vec<LayoutChild>,
    },
    Widget{
        #[serde(flatten)]
        style: MessageStyleOptions,
        text: Option<TextParts>,
    },
}

#[derive(Debug, Deserialize)]
struct MessageOptions {
    id: Option<usize>,
    persist: Option<bool>,
    hidden: Option<bool>,
    min_width:  Option<LuaSizeMetric>,
    max_width:  Option<LuaSizeMetric>,
    flex_width: Option<sizing::Flex>,
    min_height: Option<LuaSizeMetric>,
    max_height: Option<LuaSizeMetric>,
    flex_height: Option<sizing::Flex>,
    #[serde(flatten)]
    inner: MessageInner,
}

#[derive(Clone, Copy, Debug, Default, strum::EnumString)]
enum BorderSide {
    Top,
    Right,
    Bottom,
    Left,
    #[default]
    All,
}

impl From<BorderSide> for border::Sides {
    fn from(val: BorderSide) -> Self {
        match val {
            BorderSide::Top    => border::Sides::TOP,
            BorderSide::Right  => border::Sides::RIGHT,
            BorderSide::Bottom => border::Sides::BOTTOM,
            BorderSide::Left   => border::Sides::LEFT,
            BorderSide::All    => border::Sides::ALL,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BorderSides {
    Single(SerdeWrap<BorderSide>),
    Multiple(Vec<SerdeWrap<BorderSide>>),
}

#[derive(Debug, Deserialize)]
struct BorderTitleOptions {
    align: Option<SerdeWrap<Alignment>>,
    #[serde(flatten)]
    text: TextParts,
}


#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BorderOptions {
    enabled: Option<bool>,
    sides: Option<BorderSides>,
    r#type: Option<SerdeWrap<border::Kind>>,
    title_top: Option<BorderTitleOptions>,
    title_bottom: Option<BorderTitleOptions>,
    show_empty: Option<bool>,
    #[serde(flatten)]
    style: StyleOptions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct UnderlineStyleOptions {
    color: LuaColor,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
enum UnderlineOption {
    Bool(bool),
    Options(UnderlineStyleOptions),
}

#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
struct StyleOptions {
    fg: Option<LuaColor>,
    bg: Option<LuaColor>,
    bold: Option<bool>,
    dim: Option<bool>,
    italic: Option<bool>,
    underline: Option<UnderlineOption>,
    strikethrough: Option<bool>,
    reversed: Option<bool>,
    blink: Option<bool>,
}

impl FromLua for StyleOptions {
    fn from_lua(value: LuaValue, lua: &Lua) -> LuaResult<Self> {
        lua.from_value(value)
    }
}

impl StyleOptions {
    fn is_none(&self) -> bool {
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

impl From<Style> for StyleOptions {
    fn from(style: Style) -> Self {
        macro_rules! get_modifier {
            ($value:expr) => {
                if style.modifier_mask.contains($value) {
                    Some(style.modifier.contains($value))
                } else {
                    None
                }
            }
        }

        Self {
            fg: style.fg.map(LuaColor),
            bg: style.bg.map(LuaColor),
            bold:          get_modifier!(Modifier::BOLD),
            dim:           get_modifier!(Modifier::DIM),
            italic:        get_modifier!(Modifier::ITALIC),
            underline: if let Some(color) = style.underline_color {
                Some(UnderlineOption::Options(UnderlineStyleOptions { color: LuaColor(color) }))
            } else {
                get_modifier!(Modifier::UNDERLINED).map(UnderlineOption::Bool)
            },
            strikethrough: get_modifier!(Modifier::CROSSED_OUT),
            reversed:      get_modifier!(Modifier::REVERSED),
            blink:         get_modifier!(Modifier::SLOW_BLINK),
        }
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
            let hl = if part.style.style.is_none() {
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
                    let hl = if part.style.style.is_none() {
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

fn set_widget_options(
    widget: &mut tui::widget::Widget,
    style: MessageStyleOptions,
) {
    if let Some(align) = style.align {
        widget.inner.alignment = align.0;
    }

    if let Some(show_cursor) = style.show_cursor {
        widget.ansi_show_cursor = show_cursor;
    }

    match style.border {
        // explicitly disabled
        Some(BorderOptions{enabled: Some(false), ..}) => {
            widget.border.clear();
        },
        Some(options) => {

            let sides = match &options.sides {
                Some(BorderSides::Single(b)) => Some(b.0.into()),
                Some(BorderSides::Multiple(b)) => b.iter().map(|x| border::Sides::from(x.0)).reduce(|x, y| x.union(y)),
                None => Some(border::Sides::ALL),
            };

            if let Some(sides) = sides {
                let style: tui::widget::StyleOptions = options.style.clone().into();
                widget.border.sides = sides;
                widget.border.kind = options.r#type.unwrap_or(SerdeWrap(widget.border.kind)).0;
                widget.border.style = widget.border.style.patch(style.as_style());

                if let Some(text) = options.title_top {
                    let title = widget.border.title_top.get_or_insert_default();
                    title.text.style = widget.border.style;
                    parse_text_parts(text.text, &mut title.text);
                    if let Some(align) = text.align {
                        title.alignment = align.0;
                    }
                }

                if let Some(text) = options.title_bottom {
                    let title = widget.border.title_bottom.get_or_insert_default();
                    title.text.style = widget.border.style;
                    parse_text_parts(text.text, &mut title.text);
                    if let Some(align) = text.align {
                        title.alignment = align.0;
                    }
                }

            } else {
                widget.border.clear();
            }

            widget.border_show_empty = options.show_empty.unwrap_or(widget.border_show_empty);

        },
        None => {},
    }

    widget.style = widget.style.merge(&style.style.style.clone().into());
    widget.inner.style = widget.style.as_style();
}

fn process_message(tui: &mut tui::Tui, options: MessageOptions) -> Result<&mut Node> {
    let node = match options.inner {

        MessageInner::Layout { direction, children } => {

            let mut layout = Layout {
                direction: match direction {
                    Direction::vertical   => layout::Direction::Vertical,
                    Direction::horizontal => layout::Direction::Horizontal,
                },
                children: vec![],
            };

            for child in children {
                match child {
                    LayoutChild::Message(child_options) => {
                        let child = process_message(tui, child_options)?;
                        child.has_parent = true;
                        layout.children.push(child.id);
                    },
                    LayoutChild::WidgetRef(id) => {
                        tui.nodes.remove_child_from_parent(id);
                        let Some(node) = tui.get_node_mut(id)
                            else { anyhow::bail!("can't find widget with id {id}") };
                        node.has_parent = true;
                        layout.children.push(id);
                    },
                }
            }

            if let Some(id) = options.id {
                match tui.get_node_mut(id) {
                    Some(node) => {
                        node.kind = NodeKind::Layout(layout);
                        node
                    },
                    None => anyhow::bail!("can't find node with id {id}"),
                }
            } else {
                tui.nodes.add(NodeKind::Layout(layout))
            }
        },

        MessageInner::Widget { style, text } if text.is_none() && style.is_none() => {
            if let Some(id) = options.id {
                match tui.get_node_mut(id) {
                    Some(node) => node,
                    None => anyhow::bail!("can't find node with id {id}"),
                }
            } else {
                tui.nodes.add(NodeKind::Widget(tui::widget::Widget::default()))
            }
        },

        MessageInner::Widget { style, text } => {
            let node = if let Some(id) = options.id {
                match tui.get_node_mut(id) {
                    Some(node) => {
                        if !matches!(node.kind, NodeKind::Widget(_)) {
                            node.kind = NodeKind::Widget(tui::widget::Widget::default());
                        }
                        node
                    },
                    None => anyhow::bail!("can't find node with id {id}"),
                }
            } else {
                tui.nodes.add(NodeKind::Widget(tui::widget::Widget::default()))
            };

            let NodeKind::Widget(widget) = &mut node.kind
                else { unreachable!() };

            if let Some(text) = text {
                parse_text_parts(text, &mut widget.inner);
            }
            set_widget_options(widget, style);

            node
        },
    };

    // Apply node options (persist/hidden/constraints)
    if let Some(persist) = options.persist {
        node.persist = persist;
    }
    if let Some(hidden) = options.hidden {
        node.set_hidden(hidden);
    }

    node.height_spec.min = options.min_height.map(|x| x.0).or(node.height_spec.min);
    node.height_spec.max = options.max_height.map(|x| x.0).or(node.height_spec.max);
    node.height_spec.flex = options.flex_height.or(node.height_spec.flex);
    node.width_spec.min = options.min_width.map(|x| x.0).or(node.width_spec.min);
    node.width_spec.max = options.max_width.map(|x| x.0).or(node.width_spec.max);
    node.width_spec.flex = options.flex_width.or(node.width_spec.flex);

    Ok(node)
}

async fn set_message(ui: Ui, lua: Lua, val: LuaValue) -> Result<usize> {
    let options: MessageOptions = lua.from_value(val)?;

    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;
    tui.dirty = true;

    let node = process_message(tui, options)?;
    let id = node.id;
    // Only add newly created top-level nodes to root; existing nodes keep their position
    if !node.has_parent {
        node.has_parent = true;
        tui.nodes.add_child(id);
    }
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
    Ok(ui.get().borrow().tui.get_node(id).is_some())
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

async fn scroll_message(ui: Ui, _lua: Lua, (id, delta): (usize, isize)) -> Result<()> {
    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;
    match tui.get_node_mut(id) {
        Some(Node{ kind: NodeKind::Widget(widget), .. }) => {
            if widget.scroll(delta, true) {
                tui.dirty = true;
            }
            Ok(())
        },
        Some(_) => anyhow::bail!("can't scroll layout with id {id}"),
        _ => anyhow::bail!("can't find widget with id {id}"),
    }
}

async fn scroll_message_to(ui: Ui, _lua: Lua, (id, line): (usize, usize)) -> Result<()> {
    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;
    match tui.get_node_mut(id) {
        Some(Node{ kind: NodeKind::Widget(widget), .. }) => {
            if widget.scroll(line as isize, false) {
                tui.dirty = true;
            }
            Ok(())
        },
        Some(_) => anyhow::bail!("can't scroll layout with id {id}"),
        _ => anyhow::bail!("can't find widget with id {id}"),
    }
}

async fn feed_ansi_message(ui: Ui, _lua: Lua, (id, value): (usize, LuaString)) -> Result<()> {
    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;

    match tui.get_node_mut(id) {
        Some(Node{ kind: NodeKind::Widget(widget), .. }) => {
            widget.feed_ansi((&*value.as_bytes()).into());
            tui.dirty = true;
            Ok(())
        },
        Some(_) => anyhow::bail!("can't add text to layout with id {id}"),
        _ => anyhow::bail!("can't find widget with id {id}"),
    }
}

async fn clear_message(ui: Ui, _lua: Lua, id: usize) -> Result<()> {
    let ui = ui.get();
    let tui = &mut ui.borrow_mut().tui;

    match tui.get_node_mut(id) {
        Some(node) => node.clear(),
        _ => anyhow::bail!("can't find widget with id {id}"),
    }
    Ok(())
}

async fn get_message_text(ui: Ui, _lua: Lua, id: usize) -> Result<Vec<BString>> {
    let ui = ui.get();
    let tui = &ui.borrow().tui;

    match tui.get_node(id) {
        Some(Node{ kind: NodeKind::Widget(widget), .. }) => Ok(widget.inner.get().into()),
        Some(_) => anyhow::bail!("can't get text from layout with id {id}"),
        _ => anyhow::bail!("can't find widget with id {id}"),
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
    let options: Option<MessageOptions> = lua.from_value(val)?;
    let mut ui = ui.borrow_mut();
    if let Some(options) = options {
        match options.inner {
            MessageInner::Widget { style, text } => {
                let widget = ui.status_bar.inner.get_or_insert_default();
                if let Some(text) = text {
                    widget.inner.clear();
                    parse_text_parts(text, &mut widget.inner);
                }
                // StatusBar is standalone, node options (persist/hidden/constraint) are ignored
                set_widget_options(widget, style);
            },
            MessageInner::Layout { .. } => anyhow::bail!("status bar only accepts widget options"),
        }
    }
    ui.status_bar.dirty = true;
    Ok(())
}

async fn enable_mouse_mode(ui: Ui, _lua: Lua, enable: Option<bool>) -> Result<()> {
    let locks = (
        ui.has_foreground_process.lock().await,
        ui.print_lock.lock_exclusive().await,
    );

    let ui = ui.get();
    let mut ui = ui.borrow_mut();
    let mouse_mode = enable.unwrap_or(true);
    if mouse_mode != mouse_mode {
        ui.mouse_mode = mouse_mode;
        ui.apply_mouse_mode()?;
    }

    drop(locks);
    Ok(())
}

fn get_message_geometry(ui: &Ui, lua: &Lua, id: usize) -> Result<Option<LuaTable>> {
    let ui = ui.get();
    let tui = &ui.borrow().tui;

    if let Some(geom) = tui.get_node_geometry(id) {
        let table = lua.create_table_from([
            ("x", geom.x),
            ("y", geom.y),
            ("width", geom.width),
            ("height", geom.height),
        ])?;
        Ok(Some(table))
    } else {
        Ok(None)
    }
}

fn get_status_bar_geometry(ui: &Ui, lua: &Lua, (): ()) -> Result<Option<LuaTable>> {
    let ui = ui.get();
    let ui = ui.borrow();
    if let Some(geom) = ui.tui.get_status_bar_geometry(&ui.status_bar) {
        let table = lua.create_table_from([
            ("x", geom.x),
            ("y", geom.y),
            ("width", geom.width),
            ("height", geom.height),
        ])?;
        Ok(Some(table))
    } else {
        Ok(None)
    }
}

fn sgr_to_style(lua: &Lua, sgr: String) -> LuaResult<LuaValue> {
    let sgr = if sgr.starts_with("\x1b[") && sgr.ends_with('m') {
        &sgr[2..sgr.len()-1]
    } else {
        &sgr
    };
    let style = tui::widget::parse_ansi_col(Style::default(), sgr.into());
    let options = StyleOptions::from(style);
    Ok(lua.to_value(&options)?)
}

fn style_to_sgr(_lua: &Lua, options: StyleOptions) -> LuaResult<BString> {
    let style: tui::widget::StyleOptions = options.into();
    let style = style.as_style();
    let mut cell = Cell::default();
    cell.style = style;

    let mut buf = vec![];
    let mut canvas = tui::DummyCanvas::default();
    let mut drawer = tui::Drawer::new(&mut canvas, &mut buf, (0, 0));
    drawer.print_style_of_cell(&cell)?;

    Ok(buf.into())
}

async fn allocate_height(ui: Ui, _lua: Lua, height: u16) -> Result<()> {
    ui.allocate_height(height).await
}

pub fn init_lua(ui: &Ui) -> Result<()> {

    let lua_api = ui.get_lua_api()?;

    lua_api.set("sgr_to_style", ui.lua.create_function(sgr_to_style)?)?;
    lua_api.set("style_to_sgr", ui.lua.create_function(style_to_sgr)?)?;
    ui.set_lua_async_fn("allocate_height", allocate_height)?;
    ui.set_lua_async_fn("set_message", set_message)?;
    ui.set_lua_async_fn("check_message", check_message)?;
    ui.set_lua_async_fn("remove_message", remove_message)?;
    ui.set_lua_async_fn("clear_messages", clear_messages)?;
    ui.set_lua_async_fn("scroll_message", scroll_message)?;
    ui.set_lua_async_fn("scroll_message_to", scroll_message_to)?;
    ui.set_lua_async_fn("add_buf_highlight_namespace", add_buf_highlight_namespace)?;
    ui.set_lua_async_fn("add_buf_highlight", add_buf_highlight)?;
    ui.set_lua_async_fn("clear_buf_highlights", clear_buf_highlights)?;
    ui.set_lua_async_fn("feed_ansi_message", feed_ansi_message)?;
    ui.set_lua_async_fn("clear_message", clear_message)?;
    ui.set_lua_async_fn("get_message_text", get_message_text)?;
    ui.set_lua_async_fn("message_to_ansi_string", message_to_ansi_string)?;
    ui.set_lua_async_fn("set_status_bar", set_status_bar)?;
    ui.set_lua_async_fn("enable_mouse_mode", enable_mouse_mode)?;
    ui.set_lua_fn("get_message_geometry", get_message_geometry)?;
    ui.set_lua_fn("get_status_bar_geometry", get_status_bar_geometry)?;

    Ok(())
}

