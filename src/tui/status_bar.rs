use ratatui::layout::Rect;

#[derive(Default)]
pub struct StatusBar {
    pub inner: Option<super::Widget>,
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
            widget.line_count = widget.get_height_for_width(area);
        }
    }

    pub fn get_height(&self) -> u16 {
        self.inner.as_ref().map_or(0, |w| w.line_count)
    }
}
