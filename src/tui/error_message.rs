use super::layout::Nodes;
use ratatui::style::{Style, Color, Modifier};

#[derive(Debug, Clone, Copy)]
pub struct ErrorMessage {
    pub id: usize,    // Persistent widget node ID
    pub count: usize,      // Total errors encountered since last clear
}

impl ErrorMessage {
    pub const BORDER_STYLE: Style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
    pub const TEXT_STYLE: Style = Style::new().fg(Color::White);

    pub fn new(nodes: &mut Nodes) -> Self {
        let node = nodes.add(super::layout::NodeKind::Widget(super::widget::Widget::default()));
        node.persist = true;
        Self{ id: node.id, count: 0 }
    }

    pub fn add_error(&mut self, message: &str, nodes: &mut Nodes) {
        self.count += 1;
        if let Some(node) = nodes.get_node_mut(self.id) {
            node.hidden = false;
            if let super::layout::NodeKind::Widget(widget) = &mut node.kind {
                widget.inner.clear();
                widget.inner.push_str(message.into(), Some(Self::TEXT_STYLE.into()));
                widget.block = Some(self.make_border());
            }
        }
    }

    fn make_border(&self) -> ratatui::widgets::Block<'static> {
        let title = if self.count > 1 {
            format!(" Error (+{} more) ", self.count - 1)
        } else {
            " Error ".into()
        };

        ratatui::widgets::Block::new()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(Self::BORDER_STYLE)
            .title(title)
    }

    pub fn clear(&mut self, nodes: &mut Nodes) {
        self.count = 0;
        if let Some(node) = nodes.get_node_mut(self.id) {
            node.hidden = true;
        }
    }
}
