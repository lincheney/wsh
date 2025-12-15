use std::default::Default;
use std::io::{Write};
use ratatui::{
    layout::*,
    widgets::*,
    style::*,
    buffer::Buffer,
};

#[derive(Default, Debug, Clone, Copy)]
pub enum UnderlineOption {
    #[default]
    None,
    Set,
    Color(Color),
}

#[derive(Debug, Default)]
pub struct StyleOptions {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: Option<bool>,
    pub dim: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<UnderlineOption>,
    pub strikethrough: Option<bool>,
    pub reversed: Option<bool>,
    pub blink: Option<bool>,
}

impl StyleOptions {
    pub fn as_style(&self) -> Style {
        let mut add_modifier = Modifier::empty();
        let mut sub_modifier = Modifier::empty();
        let mut underline_color = None;
        match self.underline {
            None => (),
            Some(UnderlineOption::None) => { sub_modifier |= Modifier::UNDERLINED; },
            Some(UnderlineOption::Set) => { add_modifier |= Modifier::UNDERLINED; },
            Some(UnderlineOption::Color(color)) => {
                underline_color = Some(color);
                add_modifier |= Modifier::UNDERLINED;
            },
        }

        let mut style = Style {
            fg: self.fg,
            bg: self.bg,
            underline_color,
            add_modifier,
            sub_modifier,
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
    pub(super) id: usize,
    pub constraint: Option<Constraint>,
    pub inner: super::text::Text,
    pub style: StyleOptions,
    pub border_sides: Option<Borders>,
    pub border_style: Style,
    pub border_type: BorderType,
    pub border_show_empty: bool,
    pub border_title: Option<super::text::Text>,
    pub block: Option<Block<'static>>,
    pub persist: bool,
    pub hidden: bool,
    pub(super) line_count: u16,
    // text_overrides_style: bool,
}

impl Widget {

    pub(super) fn render<W :Write>(
        &self,
        drawer: &mut super::Drawer<W>,
        buffer: &mut Buffer,
        max_height: Option<usize>,
    ) -> std::io::Result<()> {

        let area = if let Some(block) = &self.block {
            block.render_ref(buffer.area, buffer);
            block.inner(buffer.area)
        } else {
            buffer.area
        };

        let border_top = 0 .. (area.y * buffer.area.width) as usize;
        let border_bottom = (buffer.area.width * (area.y + area.height)) as usize .. (buffer.area.width * buffer.area.height) as usize;
        let border_left = border_top.end .. border_top.end + area.x as usize;
        let border_right = border_top.end + (area.x + area.width) as usize .. border_top.end + buffer.area.width as usize;

        let border_top_height = area.y;
        let border_bottom_height = buffer.area.height - area.height - area.y;

        let mut state = super::text::RenderState::default();
        let mut first_line = true;
        let mut max_height = max_height.unwrap_or(usize::MAX).min((drawer.term_height() - drawer.cur_pos.1) as _);

        if self.border_show_empty || !self.inner.get().is_empty() {
            // draw top border
            let height = max_height.min(border_top_height as _);
            if height > 0 {
                drawer.draw_lines(buffer.content[border_top].chunks(buffer.area.width as _).take(height))?;
                max_height -= height;
            }
        }

        let line_iter = self.inner.get().iter()
            .enumerate()
            .flat_map(|(lineno, line)| {
                super::text::wrap(line.as_ref(), area.width as usize, 0)
                .map(move |(range, _width)| (lineno, line, range))
            });

        let clear_cell = self.inner.make_default_style_cell();
        for (lineno, line, range) in line_iter {
            // leave room for the bottom border
            if max_height <= border_bottom_height as _ {
                break
            }
            max_height -= 1;

            if !first_line {
                drawer.goto_newline(clear_cell.as_ref())?;
            }
            first_line = false;

            // draw left border
            for cell in &buffer.content[border_left.clone()] {
                drawer.draw_cell(cell, false)?;
            }

            // draw line
            self.inner.render_line(&mut state, lineno, line.as_ref(), range, drawer, None)?;

            // draw right border
            if !border_right.is_empty()  {
                drawer.clear_to_end_of_line(clear_cell.as_ref())?;
                drawer.cur_pos.0 = buffer.area.width - border_right.len() as u16;
                for cell in &buffer.content[border_right.clone()] {
                    drawer.draw_cell(cell, false)?;
                }
            }
        }

        if !self.inner.get().is_empty() {
            drawer.clear_to_end_of_line(clear_cell.as_ref())?;
        }

        // draw bottom border
        if self.border_show_empty || !self.inner.get().is_empty() {
            let height = max_height.min(border_bottom_height as _);
            if height > 0 {
                drawer.draw_lines(buffer.content[border_bottom].chunks(buffer.area.width as _).take(height))?;
                // max_height -= border_height;
            }
        }

        Ok(())
    }

    pub(super) fn get_height_for_width(&self, mut area: Rect) -> u16 {
        let mut height = 0;
        let mut min_height = None;
        let mut max_height = None;
        match self.constraint {
            Some(Constraint::Min(min)) => min_height = Some(min),
            Some(Constraint::Max(max)) => max_height = Some(max),
            _ => (),
        }

        let mut border_height = 0;
        if let Some(ref block) = self.block {
            let inner = block.inner(area);
            border_height = area.height - inner.height;
            area = inner;
        }

        height = height.max(self.inner.get_height_for_width(area.width as _, 0) as _);

        if self.border_show_empty || height > 0 {
            height += border_height;
        }
        if let Some(min_height) = min_height {
            height = height.max(min_height);
        }
        if let Some(max_height) = max_height {
            height = height.min(max_height);
        }
        height
    }

}

