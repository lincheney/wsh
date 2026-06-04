use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use super::widget::Widget;
use super::drawer::{Drawer, Canvas};
use super::text::{Renderer, TextRenderer};
use super::sizing;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Direction {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Default, Debug, Clone)]
pub struct Layout {
    pub direction: Direction,
    pub children: Vec<usize>,
}

impl Layout {

    fn is_visible(&self, map: &HashMap<usize, Node>, tmp: bool) -> bool {
        self.children.iter().any(|c| map.get(c).is_some_and(|node| node.is_visible(map, tmp)))
    }

    fn refresh(
        &self,
        map: &HashMap<usize, Node>,
        max_width: u16,
        max_height: Option<u16>,
        tmp: bool,
    ) -> (u16, u16) {

        let visible: Vec<&Node> = self.children.iter()
            .filter_map(|cid| map.get(cid))
            .filter(|n| !n.is_hidden())
            .collect();

        if visible.is_empty() {
            return (0, 0);
        }

        match self.direction {
            Direction::Vertical => {
                let mut sizes: Vec<_> = visible.iter().map(|node| {
                    // given unlimited height, how much do you want?
                    let desired_height = node.refresh(map, max_width, None, true).1;
                    node.height_spec.into_size(max_height, Some(desired_height))
                }).collect();
                let mut sizes = sizing::SizeArray(&mut sizes);
                sizes.allocate(max_height);

                let mut width = 0u16;
                let mut height = 0u16;
                for (child, size) in visible.iter().zip(sizes.0.iter()) {
                    let size = child.refresh(map, max_width, Some(size.size), false);
                    width = width.max(size.0);
                    height += size.1;
                }
                (width, height.min(max_height.unwrap_or(height)))
            },
            Direction::Horizontal => {
                let mut sizes: Vec<_> = visible.iter()
                    .map(|node| node.width_spec.into_size(Some(max_width), None))
                    .collect();
                let mut sizes = sizing::SizeArray(&mut sizes);
                sizes.allocate(Some(max_width));

                let mut width = 0u16;
                let mut height = 0u16;
                for (child, size) in visible.iter().zip(sizes.0.iter()) {
                    let dim = child.refresh(map, size.size, max_height, tmp);
                    let child_height = child.height_spec.into_size(max_height, Some(dim.1)).size;
                    width += dim.0;
                    height = height.max(child_height);
                }
                (width, height.min(max_height.unwrap_or(u16::MAX)))
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Widget(Widget),
    Layout(Layout),
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: usize,
    pub has_parent: bool,
    pub kind: NodeKind,
    pub height_spec: sizing::Constraint,
    pub width_spec: sizing::Constraint,
    pub persist: bool,
    hidden: bool,
    // cached width,height after refresh
    pub(super) size: Cell<(u16, u16)>,
    pub(super) tmp_size: Cell<(u16, u16)>,
}

impl Node {

    pub fn set_hidden(&mut self, hidden: bool) {
        self.hidden = hidden;
        if hidden && let NodeKind::Widget(w) = &self.kind {
            w.draw_pos.set(None);
        }
    }

    pub fn is_hidden(&self) -> bool {
        self.hidden
    }

    fn set_size(&self, size: (u16, u16), tmp: bool) {
        if tmp {
            self.tmp_size.set(size);
        } else {
            self.size.set(size);
        }
    }

    pub fn get_size(&self, tmp: bool) -> (u16, u16) {
        if tmp {
            self.tmp_size.get()
        } else {
            self.size.get()
        }
    }

    fn is_visible(&self, map: &HashMap<usize, Node>, tmp: bool) -> bool {
        if self.is_hidden() || self.get_size(tmp).1 == 0 {
            false
        } else if let NodeKind::Layout(layout) = &self.kind && !layout.is_visible(map, tmp) {
            false
        } else {
            true
        }
    }

    pub fn get_draw_pos(&self, map: &HashMap<usize, Node>) -> Option<(u16, u16)> {
        match &self.kind {
            NodeKind::Widget(widget) => widget.draw_pos.get(),
            NodeKind::Layout(layout) => {
                layout.children
                    .iter()
                    .filter_map(|id| map.get(id))
                    .filter(|child| child.is_visible(map, false))
                    .find_map(|child| child.get_draw_pos(map))
            }
        }
    }

    pub fn clear(&mut self) {
        match &mut self.kind {
            NodeKind::Widget(widget) => widget.clear(),
            NodeKind::Layout(layout) => layout.children.clear(),
        }
    }

    pub(super) fn refresh(
        &self,
        map: &HashMap<usize, Node>,
        max_width: u16,
        max_height: Option<u16>,
        tmp: bool,
    ) -> (u16, u16) {

        let mut dim = (0, 0);
        if !self.is_hidden() {
            let height = match &self.kind {
                NodeKind::Widget(widget) => widget.get_height_for_width(max_width),
                NodeKind::Layout(layout) => layout.refresh(map, max_width, max_height, tmp).1,
            };
            let height = height.min(max_height.unwrap_or(height));
            let height = self.height_spec.into_size(max_height, Some(height)).size;
            dim = (max_width, height);
        }

        self.set_size(dim, tmp);
        dim
    }
}

#[derive(Default)]
pub struct Nodes {
    pub(super) map: HashMap<usize, Node>,
    root: Layout,
    counter: usize,
    pub size: Cell<(u16, u16)>,
}

impl Nodes {
    fn next_id(&mut self) -> usize {
        self.counter += 1;
        self.counter
    }

    pub fn add(&mut self, kind: NodeKind) -> &mut Node {
        let id = self.next_id();
        self.add_child(id);
        self.map.entry(id).insert_entry(Node {
            id,
            has_parent: false,
            kind,
            height_spec: sizing::Constraint::default(),
            width_spec: sizing::Constraint::default(),
            persist: false,
            hidden: false,
            size: Cell::new((0, 0)),
            tmp_size: Cell::new((0, 0)),
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
                    node.set_hidden(true);
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
        self.size.get().1
    }

    pub fn refresh(&mut self, max_width: u16, max_height: Option<u16>) {
        // draw any cursors
        // this is such a hack
        for node in self.map.values_mut() {
            if !node.is_hidden() && let NodeKind::Widget(widget) = &mut node.kind {
                widget.make_cursor_space_hl();
            }
        }

        self.size.set(self.root.refresh(&self.map, max_width, max_height, false));
    }

    pub fn render<W: Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
        tmp: bool,
    ) -> std::io::Result<()> {
        let mut renderer = NodeRenderer::new_for_layout(&self.root, &self.map, tmp);
        let callback: Option<fn(&mut Drawer<W, C>, usize, usize, usize)> = None;
        renderer.render(drawer, false, true, callback)?;
        drawer.clear_to_end_of_line(None)
    }

    pub fn render_node<W: Write, C: Canvas>(
        &self,
        node: &Node,
        drawer: &mut Drawer<W, C>,
        tmp: bool,
    ) -> std::io::Result<()> {
        let mut renderer = NodeRenderer::new(node, &self.map, tmp);
        let callback: Option<fn(&mut Drawer<W, C>, usize, usize, usize)> = None;
        renderer.render(drawer, false, true, callback)?;
        drawer.clear_to_end_of_line(None)
    }
}

enum NodeRenderer<'a, I> {
    VerticalLayout {
        children: I,
        child: Option<Box<NodeRenderer<'a, I>>>,
        map: &'a HashMap<usize, Node>,
        tmp: bool,
    },
    HorizontalLayout {
        children: Vec<(&'a Node, NodeRenderer<'a, I>, bool)>,
    },
    Widget {
        renderer: TextRenderer<'a>,
        widget: &'a Widget,
        tmp: bool,
        pos_recorded: bool,
    },
}

impl<'a> NodeRenderer<'a, std::slice::Iter<'a, usize>> {

    fn new_for_layout(
        layout: &'a Layout,
        map: &'a HashMap<usize, Node>,
        tmp: bool,
    ) -> Self {

        match layout {
            Layout{ direction: Direction::Vertical, children } => {
                NodeRenderer::VerticalLayout{
                    children: children.iter(),
                    child: None,
                    map,
                    tmp,
                }
            },
            Layout{ direction: Direction::Horizontal, children } => {
                let children = children.iter()
                    .filter_map(|id| map.get(id))
                    .filter(|node| node.is_visible(map, tmp))
                    .map(|node| (node, NodeRenderer::new(node, map, tmp), false))
                    .collect();
                NodeRenderer::HorizontalLayout{children}
            },
        }
    }

    fn new(
        node: &'a Node,
        map: &'a HashMap<usize, Node>,
        tmp: bool,
    ) -> Self {

        match &node.kind {
            NodeKind::Layout(layout) => Self::new_for_layout(layout, map, tmp),
            NodeKind::Widget(widget) => {
                let size = node.get_size(tmp);
                let renderer = TextRenderer::new(
                    &widget.inner, // text
                    0, // initial_indent
                    Some(&widget.border), // border
                    size.0 as _, // width
                    Some(size.1 as _), // height
                    Some(widget.scroll), // scroll
                    widget.cursor_space_hl.iter(), // extra_highlights
                );
                NodeRenderer::Widget {
                    renderer,
                    widget,
                    tmp,
                    pos_recorded: false,
                }
            },
        }
    }
}

impl<'a> Renderer for NodeRenderer<'a, std::slice::Iter<'a, usize>> {
    fn finished(&mut self) -> bool {
        match self {
            NodeRenderer::VerticalLayout{children, child, map, tmp} => {
                if child.as_mut().is_some_and(|child| !child.finished()) {
                    return false
                }
                for id in children {
                    if let Some(node) = map.get(id) && node.is_visible(map, *tmp) {
                        let mut renderer = NodeRenderer::new(node, map, *tmp);
                        if !renderer.finished() {
                            *child = Some(Box::new(renderer));
                            return false
                        }
                    }
                }
                *child = None;
                true
            },
            NodeRenderer::HorizontalLayout{children} => {
                let mut all_finished = true;
                for (_node, renderer, finished) in children.iter_mut() {
                    if !*finished && renderer.finished() {
                        *finished = true;
                    }
                    all_finished = all_finished && *finished;
                }
                all_finished
            },
            NodeRenderer::Widget{renderer, ..} => renderer.finished(),
        }
    }

    fn draw_one_line<W, C, F>(
        &mut self,
        drawer: &mut Drawer<W, C>,
        newline: bool,
        pad: bool,
        callback: &mut Option<F>,
    ) -> std::io::Result<bool>
    where
        W :Write,
        C: Canvas,
        F: FnMut(&mut Drawer<W, C>, usize, usize, usize)
    {

        if self.finished() {
            return Ok(false)
        }

        match self {
            NodeRenderer::VerticalLayout{child, ..} => {
                // draw one child at a time
                // child must exist otherwise we would have returned earlier when checking finished
                child.as_mut().unwrap().draw_one_line(drawer, newline, pad, callback)
            },
            NodeRenderer::HorizontalLayout{children} => {
                // draw one line from each child
                let mut all_finished = true;
                let len = children.len();
                for (i, (node, renderer, finished)) in children.iter_mut().enumerate() {
                    // newline only if rist
                    let first = i == 0;
                    // pad if not the last one
                    let last = i == len - 1;

                    if !*finished {
                        *finished = !renderer.draw_one_line(drawer, newline && first, !last, callback)?;
                        all_finished = all_finished && *finished;
                    } else if !last {
                        if newline && first {
                            drawer.goto_newline(None)?;
                        }
                        drawer.draw_cell_n_times(&crate::tui::Cell::EMPTY, false, node.size.get().0 as _)?;
                    }
                }
                Ok(!all_finished)
            },
            NodeRenderer::Widget{ renderer, widget, tmp, pos_recorded } => {
                if !*tmp && !*pos_recorded {
                    let mut pos = drawer.get_pos();
                    if newline {
                        pos.1 += 1;
                        pos.0 = 0;
                    }
                    widget.draw_pos.set(Some(pos));
                    *pos_recorded = true;
                }
                widget.ansi.render(drawer)?;
                renderer.draw_one_line(drawer, newline, pad, callback)
            },
        }
    }
}
