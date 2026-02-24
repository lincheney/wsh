use crate::tui::text::{TextRenderer, Renderer};
use crate::tui::{Drawer, Canvas};
use std::io::{Write};
use ratatui::layout::Rect;

#[derive(Default)]
pub struct StatusBar {
    pub inner: Option<super::widget::Widget>,
    pub dirty: bool,
}

impl StatusBar {
    pub fn reset(&mut self) {
        if let Some(widget) = &mut self.inner {
            widget.line_count = 0;
        }
        self.dirty = true;
    }

    pub fn refresh(&mut self, area: Rect) {
        if let Some(widget) = &mut self.inner {
            widget.line_count = widget.get_height_for_width(area.width, None);
        }
    }

    pub fn get_height(&self) -> u16 {
        self.inner.as_ref().map_or(0, |w| w.line_count)
    }

    pub fn is_visible(&self) -> bool {
        self.inner.is_some()
    }

    pub fn render<W :Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
    ) -> std::io::Result<()> {

        let Some(inner) = &self.inner
            else { return Ok(()) };

        let callback: Option<fn(&mut Drawer<W, C>, usize, usize, usize)> = None;
        TextRenderer::new(
            &inner.inner,
            0,
            None,
            drawer.term_width() as _,
            None,
            [].iter(),
        ).render(drawer, false, true, callback)?;
        drawer.clear_to_end_of_line(None)?;
        Ok(())
    }
}
