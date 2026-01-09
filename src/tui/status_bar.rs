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
            widget.line_count = widget.get_height_for_width(area);
            ::log::debug!("DEBUG(swift) \t{}\t= {:?}", stringify!(widget.line_count), widget.line_count);
        }
    }

    pub fn get_height(&self) -> u16 {
        self.inner.as_ref().map_or(0, |w| w.line_count)
    }
}
