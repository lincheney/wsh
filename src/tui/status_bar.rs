#[derive(Default)]
pub struct StatusBar {
    pub inner: Option<super::Widget>,
    pub dirty: bool,
    pub buffer: ratatui::buffer::Buffer,
}

impl StatusBar {
    pub fn reset(&mut self) {
        if let Some(widget) = &mut self.inner {
            widget.line_count = 0;
        }
        self.dirty = true;
    }
}
