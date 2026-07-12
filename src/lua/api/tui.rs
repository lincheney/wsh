use crate::lua::{LuaWrapper, auto_from_lua, Array, FromLuaStr, FromLuaSerde, api::number::PossiblyMaxUsize};
use bstr::{BString};
use std::default::Default;
use serde::{Serialize};
use anyhow::Result;
use crossterm::style::Color;
use mlua::{prelude::*};
use crate::ui::{Ui};
use crate::tui::{
    self,
    layout::{self, Node, NodeKind, Layout, NodeId},
    Style,
    Modifier,
    Hyperlink,
    Underline,
    text::Alignment,
    Cell,
    sizing,
};
use crate::tui::border;

#[derive(Debug, Clone, Copy)]
struct LuaColor(Color);

impl FromLua for LuaColor {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        if let Some(n) = value.as_u32() && n <= u8::MAX as _ {
            // try numeric ANSI index
            return Ok(LuaColor(Color::AnsiValue(n as _)));

        } else if let Some(string) = value.as_string() {
            if let Ok(string) = std::str::from_utf8(&string.as_bytes()) {
                if let Ok(color) = Color::try_from(string) {
                    return Ok(LuaColor(color))
                } else if string.starts_with('#') && string.len() == 7 {
                    let rgb = u32::from_str_radix(&string[1..7], 16)
                        .map_err(|e| crate::lua::lua_error(e.to_string()))?;
                    let r = (rgb >> 16) as _;
                    let g = ((rgb >> 8) & 0xff) as _;
                    let b = (rgb & 0xff) as _;
                    return Ok(LuaColor(Color::Rgb{r, g, b}));
                }
            }

            Err(crate::lua::lua_error(format!("unknown color: {string:?}")))

        } else {
            Err(crate::lua::lua_error("expected string or 0 <= number <= 255"))
        }
    }
}

impl serde::Serialize for LuaColor {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Color::Rgb{r, g, b} => format!("#{r:02x}{g:02x}{b:02x}").serialize(serializer),
            Color::AnsiValue(n) => n.serialize(serializer),
            color => color.serialize(serializer),
        }
    }
}

#[derive(Debug, Clone)]
struct LuaSizeMetric(sizing::Metric);

impl FromLua for LuaSizeMetric {
    fn from_lua(value: LuaValue, lua: &Lua) -> LuaResult<Self> {
        match mlua::Either::<u16, String>::from_lua(value, lua)? {
            mlua::Either::Left(x) => Ok(Self(sizing::Metric::Fixed(x))),
            mlua::Either::Right(x) => {
                if x.ends_with('%') && let Ok(x) = x.parse::<u16>() {
                    Ok(Self(sizing::Metric::Percent(x)))
                } else {
                    Err(crate::lua::lua_error(format!("invalid metric: {x}")))
                }
            },
        }
    }
}

auto_from_lua! {
    #[derive(Debug, Default, Clone)]
    struct Text {
        text: BString,
        #[flatten]
        style: StyleOptions,
    }
}

auto_from_lua! {
    #[derive(Debug, Clone)]
    enum TextOptions {
        Single(BString),
        Many(Array<Text>),
    }
}

auto_from_lua! {
    #[derive(Debug, Clone)]
    enum LineOptions {
        Unstyled(BString),
        Styled{
            text: Option<TextOptions>,
            #[flatten]
            style: StyleOptions,
        },
    }
}

auto_from_lua! {
    #[derive(Debug, Clone)]
    enum Contents {
        Single(LineOptions),
        Lines(Array<LineOptions>),
    }
}

auto_from_lua! {
    #[derive(Debug, Default)]
    struct MessageStyleOptions {
        align: Option<FromLuaStr<Alignment>>,
        #[flatten]
        style: StyleOptions,
        border: Option<BorderOptions>,
        // ansi options
        show_cursor: Option<bool>,
    }
}

impl MessageStyleOptions {
    fn is_none(&self) -> bool {
        self.align.is_none()
            && self.style.is_none()
            && self.border.is_none()
            && self.show_cursor.is_none()
    }
}

auto_from_lua! {
    #[derive(Debug)]
    #[allow(nonstandard_style)]
    enum Direction {
        vertical,
        horizontal,
    }
}

auto_from_lua! {
    #[derive(Debug)]
    enum LayoutChild {
        Message(MessageOptions),
        WidgetRef(usize),
    }
}

auto_from_lua! {
    #[derive(Debug)]
    enum MessageInner {
        Layout{
            direction: Direction,
            children: Array<LayoutChild>,
        },
        Widget{
            #[flatten]
            style: MessageStyleOptions,
            contents: Option<Contents>,
        },
    }
}

auto_from_lua! {
    #[derive(Debug)]
    struct MessageOptions {
        id: Option<usize>,
        persist: Option<bool>,
        hidden: Option<bool>,
        min_width:  Option<LuaSizeMetric>,
        max_width:  Option<LuaSizeMetric>,
        flex_width: Option<FromLuaSerde<sizing::Flex>>,
        min_height: Option<LuaSizeMetric>,
        max_height: Option<LuaSizeMetric>,
        flex_height: Option<FromLuaSerde<sizing::Flex>>,
        #[flatten]
        inner: MessageInner,
    }
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

auto_from_lua! {
    #[derive(Debug)]
    enum BorderSides {
        Single(FromLuaStr<BorderSide>),
        Multiple(Array<FromLuaStr<BorderSide>>),
    }
}

auto_from_lua! {
    #[derive(Debug)]
    struct BorderTitleOptions {
        align: Option<FromLuaStr<Alignment>>,
        #[flatten]
        contents: LineOptions,
    }
}

auto_from_lua! {
    #[derive(Debug)]
    enum BorderTitle {
        Plain(BString),
        Detailed(BorderTitleOptions),
    }
}

impl From<BorderTitle> for BorderTitleOptions {
    fn from(val: BorderTitle) -> Self {
        match val {
            BorderTitle::Detailed(val) => val,
            BorderTitle::Plain(string) => Self { align: None, contents: LineOptions::Unstyled(string) },
        }
    }
}

auto_from_lua! {
    #[derive(Debug, Default)]
    struct BorderOptions {
        enabled: Option<bool>,
        sides: Option<BorderSides>,
        r#type: Option<FromLuaStr<border::Kind>>,
        title_top: Option<BorderTitle>,
        title_bottom: Option<BorderTitle>,
        show_empty: Option<bool>,
        #[flatten]
        style: StyleOptions,
    }
}

auto_from_lua! {
    #[allow(non_camel_case_types)]
    #[derive(Debug, Clone, Serialize)]
    enum UnderlineStyle {
        single,
        double,
        curly,
        dashed,
        dotted,
    }
}

impl From<Underline> for Option<UnderlineStyle> {
    fn from(hl: Underline) -> Self {
        match hl {
            Underline::None => None,
            Underline::Single => Some(UnderlineStyle::single),
            Underline::Double => Some(UnderlineStyle::double),
            Underline::Curly => Some(UnderlineStyle::curly),
            Underline::Dashed => Some(UnderlineStyle::dashed),
            Underline::Dotted => Some(UnderlineStyle::dotted),
        }
    }
}

impl From<UnderlineStyle> for Underline {
    fn from(hl: UnderlineStyle) -> Self {
        match hl {
            UnderlineStyle::single => Underline::Single,
            UnderlineStyle::double => Underline::Double,
            UnderlineStyle::curly => Underline::Curly,
            UnderlineStyle::dashed => Underline::Dashed,
            UnderlineStyle::dotted => Underline::Dotted,
        }
    }
}

auto_from_lua! {
    #[derive(Debug, Clone, Serialize)]
    enum UnderlineOption {
        Colored{style: UnderlineStyle, color: LuaColor,},
        Styled(UnderlineStyle),
        Bool(bool),
    }
}

auto_from_lua! {
    #[derive(Debug, Clone, Serialize)]
    enum HyperlinkOption {
        Url(String),
        Detailed{ url: String, id: Option<String>, },
    }
}

auto_from_lua! {
    #[derive(Debug, Default, Clone, Serialize)]
    pub struct StyleOptions {
        fg: Option<LuaColor>,
        bg: Option<LuaColor>,
        bold: Option<bool>,
        dim: Option<bool>,
        italic: Option<bool>,
        underline: Option<UnderlineOption>,
        strikethrough: Option<bool>,
        reversed: Option<bool>,
        blink: Option<bool>,
        hyperlink: Option<HyperlinkOption>,
    }
}

impl From<StyleOptions> for Style {
    fn from(style: StyleOptions) -> Self {

        let underline_color = if let Some(UnderlineOption::Colored{color, ..}) = style.underline {
            Some(color.0)
        } else {
            None
        };

        let underline = style.underline.map(|ul| {
            match ul {
                UnderlineOption::Bool(false) => Underline::None,
                UnderlineOption::Bool(true) => Underline::Single,
                UnderlineOption::Colored{style, ..} | UnderlineOption::Styled(style) => style.into(),
            }
        });

        let mut modifier = Modifier::empty();
        let mut modifier_mask = Modifier::empty();
        macro_rules! set_modifier {
            ($field:ident, $flag:ident) => {
                if let Some(v) = style.$field {
                    modifier_mask |= Modifier::$flag;
                    if v { modifier |= Modifier::$flag; }
                }
            }
        }

        set_modifier!(bold,          BOLD);
        set_modifier!(dim,           DIM);
        set_modifier!(italic,        ITALIC);
        set_modifier!(strikethrough, CROSSED_OUT);
        set_modifier!(reversed,      REVERSED);
        set_modifier!(blink,         BLINK);

        let hyperlink = match style.hyperlink {
            None => None,
            Some(HyperlinkOption::Url(url)) => Some(Hyperlink{ url: url.into(), id: None }),
            Some(HyperlinkOption::Detailed { url, id }) => Some(Hyperlink{ url: url.into(), id: id.map(|id| id.into()) }),
        };

        Style {
            fg: style.fg.map(|x| x.0),
            bg: style.bg.map(|x| x.0),
            underline,
            underline_color,
            hyperlink: hyperlink.map(|h| h.into()),
            modifier,
            modifier_mask,
        }
    }
}

auto_from_lua! {
    #[derive(Debug, Default, Clone)]
    pub struct EphemeralStyleOptions {
        #[flatten]
        pub inner: StyleOptions,
        pub start_column: usize,
        pub end_column: PossiblyMaxUsize,
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
            self.blink.is_none() &&
            self.hyperlink.is_none()
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
            underline: match (style.underline, style.underline_color) {
                (None, _) => None,
                (Some(style), None) => match Option::<UnderlineStyle>::from(style) {
                    None => Some(UnderlineOption::Bool(false)),
                    Some(style) => Some(UnderlineOption::Styled(style)),
                },
                (Some(style), Some(color)) => match Option::<UnderlineStyle>::from(style) {
                    None => Some(UnderlineOption::Bool(false)),
                    Some(style) => Some(UnderlineOption::Colored{style, color: LuaColor(color)}),
                },
            },
            strikethrough: get_modifier!(Modifier::CROSSED_OUT),
            reversed:      get_modifier!(Modifier::REVERSED),
            blink:         get_modifier!(Modifier::BLINK),
            hyperlink: style.hyperlink.as_ref().map(|h| {
                HyperlinkOption::Detailed {
                    url: h.url.to_string(),
                    id: h.id.as_ref().map(|id| id.to_string()),
                }
            }),
        }
    }
}

auto_from_lua! {
    #[derive(Debug, Default)]
    struct BufferStyleOptions {
        #[flatten]
        inner: StyleOptions,
        no_blend: Option<bool>,
    }
}

fn parse_line<T: Default+Clone>(line: LineOptions, text: &mut tui::text::Text<T>) {
    match line {
        LineOptions::Unstyled(string) => {
            text.push_line(string, None);
        },
        LineOptions::Styled{text: string, style} => {

            match string {
                None => (),
                Some(TextOptions::Single(string)) => {
                    text.push_line(string, None);
                },
                Some(TextOptions::Many(parts)) => {
                    for part in parts.0 {
                        let hl = if part.style.is_none() {
                            None
                        } else {
                            Some(Style::from(part.style).into())
                        };
                        text.push_str(part.text.as_ref(), hl);
                    }
                },
            }

            if !style.is_none() && text.len() > 0 {
                let inner = Style::from(style).into();
                text.add_highlight(tui::text::HighlightedRange{
                    parano: text.len() - 1,
                    start: 0,
                    end: usize::MAX,
                    inner,
                });
            }

        },
    }
}

fn parse_text_parts<T: Default+Clone>(parts: Contents, text: &mut tui::text::Text<T>) {
    text.clear();
    match parts {
        Contents::Single(line) => {
            parse_line(line, text);
        },
        Contents::Lines(lines) => {
            for line in lines.0 {
                parse_line(line, text);
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
                Some(BorderSides::Multiple(b)) => b.0.iter().map(|x| border::Sides::from(x.0)).reduce(|x, y| x.union(y)),
                None => Some(border::Sides::ALL),
            };

            if let Some(sides) = sides {
                let style: Style = options.style.clone().into();
                widget.border.sides = sides;
                widget.border.kind = options.r#type.unwrap_or(FromLuaStr(widget.border.kind)).0;
                widget.border.style = widget.border.style.clone().patch(style);

                if let Some(text) = options.title_top {
                    let text: BorderTitleOptions = text.into();
                    let title = widget.border.title_top.get_or_insert_default();
                    title.text.style = widget.border.style.clone();
                    parse_text_parts(Contents::Single(text.contents), &mut title.text);
                    if let Some(align) = text.align {
                        title.alignment = align.0;
                    }
                }

                if let Some(text) = options.title_bottom {
                    let text: BorderTitleOptions = text.into();
                    let title = widget.border.title_bottom.get_or_insert_default();
                    title.text.style = widget.border.style.clone();
                    parse_text_parts(Contents::Single(text.contents), &mut title.text);
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

    widget.style = widget.style.clone().patch(style.style.into());
    widget.inner.style = widget.style.clone();
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

            for child in children.0 {
                match child {
                    LayoutChild::Message(child_options) => {
                        let child = process_message(tui, child_options)?;
                        child.has_parent = true;
                        layout.children.push(child.id);
                    },
                    LayoutChild::WidgetRef(id) => {
                        let id = NodeId::Normal(id);
                        tui.nodes.remove_child_from_parent(id);
                        let Some(node) = tui.get_node_mut(id)
                            else { anyhow::bail!("can't find widget with id {id}") };
                        node.has_parent = true;
                        layout.children.push(id);
                    },
                }
            }

            if let Some(id) = options.id {
                let id = NodeId::Normal(id);
                match tui.get_node_mut(id) {
                    Some(node) => {
                        node.kind = NodeKind::Layout(layout);
                        node
                    },
                    None => anyhow::bail!("can't find node with id {id}"),
                }
            } else {
                tui.nodes.add(NodeKind::Layout(layout), false)
            }
        },

        MessageInner::Widget { style, contents, .. } if contents.is_none() && style.is_none() => {
            if let Some(id) = options.id {
                let id = NodeId::Normal(id);
                match tui.get_node_mut(id) {
                    Some(node) => node,
                    None => anyhow::bail!("can't find node with id {id}"),
                }
            } else {
                tui.nodes.add(NodeKind::Widget(tui::widget::Widget::default()), false)
            }
        },

        MessageInner::Widget { style, contents } => {
            let node = if let Some(id) = options.id {
                let id = NodeId::Normal(id);
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
                tui.nodes.add(NodeKind::Widget(tui::widget::Widget::default()), false)
            };

            let NodeKind::Widget(widget) = &mut node.kind
                else { unreachable!() };

            if let Some(contents) = contents {
                widget.ephemeral.clear();
                parse_text_parts(contents, &mut widget.inner);
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
    node.height_spec.flex = options.flex_height.map(|x| x.0).or(node.height_spec.flex);
    node.width_spec.min = options.min_width.map(|x| x.0).or(node.width_spec.min);
    node.width_spec.max = options.max_width.map(|x| x.0).or(node.width_spec.max);
    node.width_spec.flex = options.flex_width.map(|x| x.0).or(node.width_spec.flex);

    Ok(node)
}

fn set_message(ui: &Ui, _lua: &Lua, val: MessageOptions) -> Result<usize> {
    let tui = &mut ui.try_borrow_mut()?.tui;
    tui.dirty = true;

    let node = process_message(tui, val)?;
    let id = node.id;
    // Only add newly created top-level nodes to root; existing nodes keep their position
    if !node.has_parent {
        node.has_parent = true;
        tui.nodes.add_child(id);
    }
    ui.queue_draw();
    Ok(id.into())
}

fn redraw_message(ui: &Ui, _lua: &Lua, id: usize) -> Result<()> {
    let id = NodeId::Normal(id);
    if ui.try_borrow_mut()?.tui.nodes.invalidate_ephemeral(id) {
        ui.queue_draw();
    } else {
        anyhow::bail!("can't find widget with id {}", id)
    }
    Ok(())
}

fn clear_messages(ui: &Ui, _lua: &Lua, all: bool) -> Result<()> {
    let tui = &mut ui.try_borrow_mut()?.tui;
    if all {
        tui.clear_all();
    } else {
        tui.clear_non_persistent();
    }
    ui.queue_draw();
    Ok(())
}

fn check_message(ui: &Ui, _lua: &Lua, id: usize) -> Result<bool> {
    let id = NodeId::Normal(id);
    Ok(ui.try_borrow()?.tui.get_node(id).is_some())
}

fn remove_message(ui: &Ui, _lua: &Lua, id: usize) -> Result<()> {
    let tui = &mut ui.try_borrow_mut()?.tui;
    let id = NodeId::Normal(id);
    if tui.remove(id).is_some() {
        tui.dirty = true;
        ui.queue_draw();
        Ok(())
    } else {
        anyhow::bail!("can't find widget with id {}", id)
    }
}

auto_from_lua! {
    #[derive(Debug, Default)]
    struct BufferHighlight {
        start: super::number::PossiblyMaxUsize,
        finish: super::number::PossiblyMaxUsize,
        #[flatten]
        style: BufferStyleOptions,
        virtual_text: Option<BString>,
        conceal: Option<bool>,
        namespace: Option<usize>,
        priority: Option<f64>,
    }
}

fn add_buf_highlight(ui: &Ui, _lua: &Lua, val: BufferHighlight) -> Result<()> {
    let blend = !val.style.no_blend.unwrap_or_default();
    let priority = val.priority.unwrap_or_default();
    let style: Style = val.style.inner.into();

    ui.try_borrow_mut()?.buffer.add_highlight(tui::text::HighlightedRange{
        parano: 0,
        start: usize::from(val.start).saturating_sub(1),
        end: val.finish.into(),
        inner: tui::text::Highlight{
            style,
            namespace: val.namespace.unwrap_or(0),
            virtual_text: val.virtual_text.map(std::borrow::Cow::Owned),
            conceal: val.conceal,
            blend,
            priority,
        },
    });
    ui.queue_draw();

    Ok(())
}

fn clear_buf_highlights(ui: &Ui, _lua: &Lua, namespace: Option<usize>) -> Result<()> {
    ui.queue_draw();
    let mut ui = ui.try_borrow_mut()?;
    if let Some(namespace) = namespace {
        ui.buffer.clear_highlights_in_namespace(namespace);
    } else {
        ui.buffer.clear_highlights();
    }
    Ok(())
}

fn add_buf_highlight_namespace(ui: &Ui, _lua: &Lua, _val: ()) -> Result<usize> {
    let mut ui = ui.try_borrow_mut()?;
    ui.buffer.highlight_counter += 1;
    Ok(ui.buffer.highlight_counter)
}

fn scroll_message(ui: &Ui, _lua: &Lua, (id, delta): (usize, isize)) -> Result<()> {
    let tui = &mut ui.try_borrow_mut()?.tui;
    let id = NodeId::Normal(id);
    match tui.get_node_mut(id) {
        Some(Node{ kind: NodeKind::Widget(widget), .. }) => {
            if widget.scroll(delta, true) {
                tui.dirty = true;
                ui.queue_draw();
            }
            Ok(())
        },
        Some(_) => anyhow::bail!("can't scroll layout with id {id}"),
        _ => anyhow::bail!("can't find widget with id {id}"),
    }
}

fn scroll_message_to(ui: &Ui, _lua: &Lua, (id, line): (usize, usize)) -> Result<()> {
    let tui = &mut ui.try_borrow_mut()?.tui;
    let id = NodeId::Normal(id);
    match tui.get_node_mut(id) {
        Some(Node{ kind: NodeKind::Widget(widget), .. }) => {
            if widget.scroll(line as isize, false) {
                tui.dirty = true;
                ui.queue_draw();
            }
            Ok(())
        },
        Some(_) => anyhow::bail!("can't scroll layout with id {id}"),
        _ => anyhow::bail!("can't find widget with id {id}"),
    }
}

fn feed_ansi_message(ui: &Ui, _lua: &Lua, (id, value): (usize, LuaString)) -> Result<()> {
    let tui = &mut ui.try_borrow_mut()?.tui;

    let id = NodeId::Normal(id);
    match tui.get_node_mut(id) {
        Some(Node{ kind: NodeKind::Widget(widget), .. }) => {
            widget.feed_ansi((&*value.as_bytes()).into());
            ui.queue_draw();
            tui.dirty = true;
            Ok(())
        },
        Some(_) => anyhow::bail!("can't add text to layout with id {id}"),
        _ => anyhow::bail!("can't find widget with id {id}"),
    }
}

fn clear_message(ui: &Ui, _lua: &Lua, id: usize) -> Result<()> {
    let tui = &mut ui.try_borrow_mut()?.tui;

    let id = NodeId::Normal(id);
    match tui.get_node_mut(id) {
        Some(node) => node.clear(),
        _ => anyhow::bail!("can't find widget with id {id}"),
    }
    ui.queue_draw();
    Ok(())
}

fn get_message_text(ui: &Ui, _lua: &Lua, id: usize) -> Result<Vec<BString>> {
    let tui = &ui.try_borrow()?.tui;

    let id = NodeId::Normal(id);
    match tui.get_node(id) {
        Some(Node{ kind: NodeKind::Widget(widget), .. }) => Ok(widget.inner.get().into()),
        Some(_) => anyhow::bail!("can't get text from layout with id {id}"),
        _ => anyhow::bail!("can't find widget with id {id}"),
    }
}

fn message_to_ansi_string(ui: &Ui, _lua: &Lua, (id, width): (usize, Option<u16>)) -> Result<mlua::BString> {
    let tui = &mut ui.try_borrow_mut()?.tui;

    let id = NodeId::Normal(id);
    match tui.render_to_string(id, width) {
        None => anyhow::bail!("can't find widget with id {id}"),
        Some(x) => Ok(x),
    }
}

fn set_status_bar(ui: &Ui, _lua: &Lua, val: Option<MessageOptions>) -> Result<()> {
    ui.queue_draw();
    let mut ui = ui.try_borrow_mut()?;
    if let Some(options) = val {
        match options.inner {
            MessageInner::Widget { style, contents } => {
                let widget = ui.status_bar.inner.get_or_insert_default();
                if let Some(contents) = contents {
                    widget.inner.clear();
                    parse_text_parts(contents, &mut widget.inner);
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

fn set_prompt(ui: &Ui, __lua: &Lua, val: Option<MessageOptions>) -> Result<()> {
    ui.queue_draw();
    let mut ui = ui.try_borrow_mut()?;
    if let Some(options) = val {
        match options.inner {
            MessageInner::Widget { style, contents } => {
                let mut widget = tui::widget::Widget::default();
                if let Some(contents) = contents {
                    parse_text_parts(contents, &mut widget.inner);
                }
                set_widget_options(&mut widget, style);
                ui.cmdline.prompt_mode = tui::command_line::PromptMode::Custom(widget);
            },
            MessageInner::Layout { .. } => anyhow::bail!("prompt only accepts widget options"),
        }
    } else {
        ui.cmdline.prompt_mode = tui::command_line::PromptMode::ShellVars(Default::default());
    }
    ui.cmdline.prompt_dirty = true;
    Ok(())
}

async fn enable_mouse_mode(ui: Ui, _lua: Lua, enable: Option<bool>) -> Result<()> {
    let locks = (
        ui.has_foreground_process.lock().await,
        ui.print_lock.lock_exclusive().await,
    );

    let mut ui = ui.try_borrow_mut()?;
    let mouse_mode = enable.unwrap_or(true);
    if mouse_mode != ui.mouse_mode {
        ui.mouse_mode = mouse_mode;
        ui.apply_mouse_mode()?;
    }

    drop(locks);
    Ok(())
}

fn get_message_geometry(ui: &Ui, lua: &Lua, id: usize) -> Result<Option<LuaTable>> {
    let tui = &ui.try_borrow()?.tui;

    let id = NodeId::Normal(id);
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
    let ui = ui.try_borrow()?;
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

fn get_cursor_pos(ui: &Ui, _lua: &Lua, (): ()) -> Result<(u16, u16)> {
    Ok(ui.try_borrow()?.cmdline.cursor_coord)
}

fn sgr_to_style(lua: &Lua, sgr: String) -> LuaResult<LuaValue> {
    let sgr = if sgr.starts_with("\x1b[") && sgr.ends_with('m') {
        &sgr[2..sgr.len()-1]
    } else {
        &sgr
    };
    let style = tui::widget::parse_ansi_col(Style::default(), sgr.into());
    let style = StyleOptions::from(style);
    lua.to_value_with(&style, mlua::serde::ser::Options::new().serialize_none_to_null(false))
}

fn style_to_sgr(_lua: &Lua, options: StyleOptions) -> LuaResult<Option<BString>> {
    let style: Style = options.into();
    if style == Style::default() {
        return Ok(None);
    }
    let mut cell = Cell::default();
    cell.style = style;

    let mut buf = vec![];
    let mut canvas = tui::DummyCanvas::default();
    let mut drawer = tui::Drawer::new(&mut canvas, &mut buf, (0, 0));
    drawer.print_style_of_cell(&cell)?;

    Ok(Some(buf.into()))
}

async fn allocate_height(ui: Ui, _lua: Lua, height: u16) -> Result<()> {
    ui.queue_draw();
    ui.allocate_height(height).await
}

fn add_render_callback(ui: &Ui, _lua: &Lua, func: LuaFunction) -> Result<usize> {
    Ok(ui.try_borrow_mut()?.tui.add_render_callback(func))
}

fn remove_render_callback(ui: &Ui, _lua: &Lua, id: usize) -> Result<()> {
    ui.try_borrow_mut()?.tui.remove_render_callback(id);
    Ok(())
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    lua.api.set("sgr_to_style", lua.create_function(sgr_to_style)?)?;
    lua.api.set("style_to_sgr", lua.create_function(style_to_sgr)?)?;
    lua.set_async_fn("allocate_height", allocate_height)?;
    lua.set_fn("set_message", set_message)?;
    lua.set_fn("redraw_message", redraw_message)?;
    lua.set_fn("check_message", check_message)?;
    lua.set_fn("remove_message", remove_message)?;
    lua.set_fn("clear_messages", clear_messages)?;
    lua.set_fn("scroll_message", scroll_message)?;
    lua.set_fn("scroll_message_to", scroll_message_to)?;
    lua.set_fn("add_buf_highlight_namespace", add_buf_highlight_namespace)?;
    lua.set_fn("add_buf_highlight", add_buf_highlight)?;
    lua.set_fn("clear_buf_highlights", clear_buf_highlights)?;
    lua.set_fn("feed_ansi_message", feed_ansi_message)?;
    lua.set_fn("clear_message", clear_message)?;
    lua.set_fn("get_message_text", get_message_text)?;
    lua.set_fn("message_to_ansi_string", message_to_ansi_string)?;
    lua.set_fn("set_status_bar", set_status_bar)?;
    lua.set_fn("set_prompt", set_prompt)?;
    lua.set_async_fn("enable_mouse_mode", enable_mouse_mode)?;
    lua.set_fn("get_message_geometry", get_message_geometry)?;
    lua.set_fn("get_status_bar_geometry", get_status_bar_geometry)?;
    lua.set_fn("get_cursor_pos", get_cursor_pos)?;
    lua.set_fn("add_render_callback", add_render_callback)?;
    lua.set_fn("remove_render_callback", remove_render_callback)?;

    Ok(())
}

