use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use ratatui::{
    layout::*,
};
use super::widget::Widget;
use super::drawer::{Drawer, Canvas};
use super::text::{Renderer, TextRenderer};

#[derive(Default, Debug)]
pub struct Layout {
    pub direction: Direction,
    pub children: Vec<usize>,
}

impl Layout {

    fn is_visible(&self, map: &HashMap<usize, Node>) -> bool {
        self.children.iter().any(|c| map.get(c).is_some_and(|node| node.is_visible(map)))
    }

    fn refresh(&self, map: &HashMap<usize, Node>, area: Rect) -> u16 {
        match self.direction {
            Direction::Vertical => {
                let mut total = 0;
                for child_id in &self.children {
                    if let Some(child) = map.get(child_id) && !child.hidden {
                        let area = Rect{ height: area.height - total, ..area};
                        child.refresh(map, area);
                        total += child.size.get().1;
                    }
                    if total >= area.height {
                        continue
                    }
                }
                total.min(area.height)
            },
            Direction::Horizontal => {
                let visible: Vec<&Node> = self.children.iter()
                    .filter_map(|cid| map.get(cid))
                    .filter(|n| !n.hidden)
                    .collect();
                if visible.is_empty() {
                    return 0
                }

                // just use the ratatui algorithm
                let constraints: Vec<_> = visible.iter()
                    .map(|n| n.constraint.unwrap_or(Constraint::Fill(1)))
                    .collect();

                let areas = ratatui::layout::Layout::horizontal(constraints).split(area);

                let mut max_height = 0;
                for (child, child_area) in visible.iter().zip(areas.iter()) {
                    child.refresh(map, *child_area);
                    max_height = max_height.max(child.size.get().1);
                }
                max_height.min(area.height)
            },
        }
    }
}

#[derive(Debug)]
pub enum NodeKind {
    Widget(Widget),
    Layout(Layout),
}

#[derive(Debug)]
pub struct Node {
    pub id: usize,
    pub has_parent: bool,
    pub kind: NodeKind,
    pub constraint: Option<Constraint>,
    pub persist: bool,
    pub hidden: bool,
    // cached width,height after refresh
    pub(super) size: Cell<(u16, u16)>,
}

impl Node {

    fn is_visible(&self, map: &HashMap<usize, Node>) -> bool {
        if self.hidden || self.size.get().1 == 0 {
            false
        } else if let NodeKind::Layout(layout) = &self.kind && !layout.is_visible(map) {
            false
        } else {
            true
        }
    }

    pub fn clear(&mut self) {
        match &mut self.kind {
            NodeKind::Widget(widget) => widget.clear(),
            NodeKind::Layout(layout) => layout.children.clear(),
        }
    }

    fn refresh(&self, map: &HashMap<usize, Node>, area: Rect) {
        if self.hidden {
            self.size.set((0, 0));
            return;
        }

        let height = match &self.kind {
            NodeKind::Widget(widget) => widget.get_height_for_width(area, self.constraint),
            NodeKind::Layout(layout) => layout.refresh(map, area),
        };
        self.size.set((area.width, height));
    }
}

#[derive(Default)]
pub struct Nodes {
    map: HashMap<usize, Node>,
    root: Layout,
    counter: usize,
    pub height: Cell<u16>,
}

impl Nodes {
    fn next_id(&mut self) -> usize {
        self.counter += 1;
        self.counter
    }

    pub fn add(&mut self, kind: NodeKind) -> &mut Node {
        let id = self.next_id();
        self.map.entry(id).insert_entry(Node {
            id,
            has_parent: false,
            kind,
            constraint: None,
            persist: false,
            hidden: false,
            size: Cell::new((0, 0)),
        }).into_mut()
    }

    pub fn get_node(&self, id: usize) -> Option<&Node> {
        self.map.get(&id)
    }

    pub fn get_node_mut(&mut self, id: usize) -> Option<&mut Node> {
        self.map.get_mut(&id)
    }

    pub fn get_layouts_mut(&mut self) -> impl Iterator<Item=&mut Layout> {
        self.map.values_mut()
            .filter_map(|node| {
                if let NodeKind::Layout(layout) = &mut node.kind {
                    Some(layout)
                } else {
                    None
                }
            }).chain(std::iter::once(&mut self.root))
    }

    /// Remove a node
    pub fn remove(&mut self, id: usize) -> Option<Node> {
        self.remove_child_from_parent(id);
        let node = self.map.remove(&id);
        if let Some(Node{ kind: NodeKind::Layout(layout), .. }) = &node {
            // orphan the children
            for child in &layout.children {
                if let Some(node) = self.map.get_mut(child) {
                    node.has_parent = false;
                    node.hidden = true;
                }
            }
        }
        node
    }

    /// Remove a child ID from all parents' children lists.
    pub fn remove_child_from_parent(&mut self, child_id: usize) {
        for layout in self.get_layouts_mut() {
            layout.children.retain(|&id| id != child_id);
        }
    }

    /// Add an existing node as a child of a layout. Removes it from any current parent first.
    pub fn add_child(&mut self, child_id: usize) {
        self.remove_child_from_parent(child_id);
        self.root.children.push(child_id);
    }

    /// Clear all top-level children (and their descendants).
    pub fn clear_all(&mut self) {
        self.map.clear();
        self.root.children.clear();
    }

    /// Remove non-persistent top-level nodes.
    pub fn clear_non_persistent(&mut self) {
        let cleared: HashSet<_> = self.map.extract_if(|_id, node| !node.persist).map(|(id, _node)| id).collect();
        for layout in self.get_layouts_mut() {
            layout.children.retain(|id| !cleared.contains(id));
        }
    }

    pub fn get_height(&self) -> u16 {
        self.height.get()
    }

    pub fn refresh(&mut self, area: Rect) {
        // draw any cursors
        // this is such a hack
        for node in self.map.values_mut() {
            if !node.hidden && let NodeKind::Widget(widget) = &mut node.kind {
                widget.make_cursor_space_hl();
            }
        }

        self.height.set(self.root.refresh(&self.map, area));
    }

    pub fn render<W: Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
        max_height: Option<usize>,
    ) -> std::io::Result<()> {
        if let Some(mut renderer) = NodeRenderer::new_for_layout(&self.root, &self.map, drawer, max_height) {
            let callback: Option<fn(&mut Drawer<W, C>, usize, usize, usize)> = None;
            renderer.render(drawer, true, (0, &ratatui::buffer::Cell::EMPTY), callback)
        } else {
            Ok(())
        }
    }

    pub fn render_node<W: Write, C: Canvas>(
        &self,
        node: &Node,
        drawer: &mut Drawer<W, C>,
        max_height: Option<usize>,
    ) -> std::io::Result<()> {
        if let Some(mut renderer) = NodeRenderer::new(node, &self.map, drawer, max_height) {
            let callback: Option<fn(&mut Drawer<W, C>, usize, usize, usize)> = None;
            renderer.render(drawer, true, (0, &ratatui::buffer::Cell::EMPTY), callback)
        } else {
            Ok(())
        }
    }
}

enum NodeRenderer<'a, I> {
    VerticalLayout {
        children: I,
        child: Option<Box<NodeRenderer<'a, I>>>,
        map: &'a HashMap<usize, Node>,
        max_height: Option<usize>,
    },
    HorizontalLayout {
        children: Vec<(&'a Node, NodeRenderer<'a, I>, bool)>,
    },
    Widget {
        renderer: TextRenderer<'a>,
    },
}

impl<'a> NodeRenderer<'a, std::slice::Iter<'a, usize>> {

    fn new_for_layout<W, C: Canvas>(
        layout: &'a Layout,
        map: &'a HashMap<usize, Node>,
        drawer: &mut Drawer<W, C>,
        max_height: Option<usize>,
    ) -> Option<Self> {

        if !layout.is_visible(map) {
            return None
        }

        Some(match layout {
            Layout{ direction: Direction::Vertical, children } => {
                NodeRenderer::VerticalLayout{children: children.iter(), child: None, map, max_height}
            },
            Layout{ direction: Direction::Horizontal, children } => {
                let children = children.iter()
                    .filter_map(|id| map.get(id))
                    .filter_map(|node| Some((node, NodeRenderer::new(node, map, drawer, max_height)?, false)))
                    .collect();
                NodeRenderer::HorizontalLayout{children}
            },
        })
    }

    fn new<W, C: Canvas>(
        node: &'a Node,
        map: &'a HashMap<usize, Node>,
        drawer: &mut Drawer<W, C>,
        max_height: Option<usize>,
    ) -> Option<Self> {

        if !node.is_visible(map) {
            return None
        }

        match &node.kind {
            NodeKind::Layout(layout) => Self::new_for_layout(layout, map, drawer, max_height),
            NodeKind::Widget(widget) => {
                let renderer = TextRenderer::new(
                    &widget.inner,
                    drawer,
                    widget.block.as_ref(),
                    Some(node.size.get().0 as usize),
                    max_height.map(|w| {
                        (
                            w,
                            super::text::Scroll{
                                show_scrollbar: true,
                                position: super::scroll::ScrollPosition::StickyBottom,
                            },
                        )
                    }),
                    widget.cursor_space_hl.iter(),
                );
                Some(NodeRenderer::Widget{renderer})
            },
        }
    }
}

impl<'a> Renderer for NodeRenderer<'a, std::slice::Iter<'a, usize>> {
    fn draw_one_line<W, C, F>(
        &mut self,
        drawer: &mut Drawer<W, C>,
        newlines: bool,
        pad_to: (u16, &ratatui::buffer::Cell),
        callback: &mut Option<F>,
    ) -> std::io::Result<bool>
    where
        W :Write,
        C: Canvas,
        F: FnMut(&mut Drawer<W, C>, usize, usize, usize)
    {

        let result = match self {
            NodeRenderer::VerticalLayout{children, child, map, max_height} => {
                // draw one child at a time
                loop {
                    if child.is_none() {
                        if let Some(id) = children.next() {
                            *child = map.get(id)
                                .and_then(|node| NodeRenderer::new(node, map, drawer, *max_height))
                                .map(Box::new);
                        } else {
                            // no more children
                            break Ok(false)
                        }
                    }

                    if let Some(child) = child && child.draw_one_line(drawer, false, pad_to, callback)? {
                        break Ok(true)
                    }
                    *child = None;
                }
            },
            NodeRenderer::HorizontalLayout{children} => {
                // draw lines from each child
                if children.iter().all(|(_, _, done)| *done) {
                    return Ok(false)
                }

                let mut all_done = true;
                let mut startx: u16 = pad_to.0;
                for (node, renderer, done) in children.iter_mut() {
                    // need to add padding
                    let endx = drawer.get_pos().0 + node.size.get().0;
                    if !*done {
                        *done = !renderer.draw_one_line(drawer, false, (startx, pad_to.1), callback)?;
                        all_done = all_done && *done;
                    }
                    startx = endx;
                }
                Ok(!all_done)
            },
            NodeRenderer::Widget{renderer} => {
                renderer.draw_one_line(drawer, false, pad_to, callback)
            },
        }?;
        if newlines {
            drawer.goto_newline(None)?;
        }
        Ok(result)
    }
}
