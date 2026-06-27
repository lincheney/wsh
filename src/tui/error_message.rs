use super::layout::Nodes;
use super::{Style, Modifier};
use super::border;
use super::text::Alignment;
use crossterm::style::Color;

#[derive(Debug)]
pub struct ErrorMessage {
    pub id: super::layout::NodeId,
    pub count: usize,
}

impl ErrorMessage {
    pub const BORDER_STYLE: Style = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
    pub const TEXT_STYLE: Style = Style::new().fg(Color::AnsiValue(15));

    pub fn new(nodes: &mut Nodes) -> Self {
        let node = nodes.add(super::layout::NodeKind::Widget(super::widget::Widget::default()), true);
        node.persist = true;
        // node.constraint = Some(Constraint::Max(7));
        Self{ id: node.id, count: 0 }
    }

    pub fn add_error(&mut self, message: &str, nodes: &mut Nodes) {
        self.count += 1;
        if let Some(node) = nodes.get_node_mut(self.id) {
            node.set_hidden(false);
            if let super::layout::NodeKind::Widget(widget) = &mut node.kind {
                widget.inner.clear();
                widget.inner.push_str(message.into(), Some(Self::TEXT_STYLE.into()));
                widget.border = self.make_border();
            }
        }
    }

    fn make_border(&self) -> border::Border {
        let title_str = if self.count > 1 {
            format!(" Error (+{} more) ", self.count - 1)
        } else {
            " Error ".into()
        };

        let mut title_text = crate::tui::text::Text::default();
        title_text.push_str(title_str.as_str().into(), Some(Self::BORDER_STYLE.into()));

        border::Border{
            sides: border::Sides::ALL,
            kind: border::Kind::Plain,
            style: Self::BORDER_STYLE,
            title_top: Some(border::Title::new(title_text, Alignment::Left)),
            title_bottom: None,
        }
    }

    pub fn clear(&mut self, nodes: &mut Nodes) {
        self.count = 0;
        if let Some(node) = nodes.get_node_mut(self.id) {
            node.set_hidden(true);
        }
    }
}
