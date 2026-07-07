use std::range::Range;
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use crate::utils::merge_sort_iter::SortedMergeable;
use super::widget::Widget;
use super::drawer::{Drawer, Canvas};
use super::text::{Renderer, TextRenderer, NoRendererCallback};
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
    pub children: Vec<NodeId>,
}

impl Layout {

    fn is_visible(&self, map: &HashMap<NodeId, Node>, tmp: bool) -> bool {
        self.children.iter().any(|c| map.get(c).is_some_and(|node| node.is_visible(map, tmp)))
    }

    fn refresh<'a>(
        &self,
        map: &HashMap<NodeId, Node>,
        max_width: u16,
        max_height: Option<u16>,
        tmp: bool,
        mut resized: Option<&'a mut Vec<usize>>,
    ) -> ((u16, u16), Option<&'a mut Vec<usize>>) {

        let visible: Vec<&Node> = self.children.iter()
            .filter_map(|cid| map.get(cid))
            .filter(|n| !n.is_hidden())
            .collect();

        if visible.is_empty() {
            return ((0, 0), resized);
        }

        let dim = match self.direction {
            Direction::Vertical => {
                let mut sizes: Vec<_> = visible.iter().map(|node| {
                    // given unlimited height, how much do you want?
                    let ((_, desired_height), _) = node.refresh(map, max_width, None, true, None);
                    node.height_spec.into_size(max_height, Some(desired_height))
                }).collect();
                let mut sizes = sizing::SizeArray(&mut sizes);
                sizes.allocate(max_height);

                let mut width = 0u16;
                let mut height = 0u16;
                for (child, size) in visible.iter().zip(sizes.0.iter()) {
                    let dim;
                    (dim, resized) = child.refresh(map, max_width, Some(size.size), false, resized);
                    width = width.max(dim.0);
                    height += dim.1;
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
                    let dim;
                    (dim, resized) = child.refresh(map, size.size, max_height, tmp, resized);
                    let child_height = child.height_spec.into_size(max_height, Some(dim.1)).size;
                    width += dim.0;
                    height = height.max(child_height);
                }
                (width, height.min(max_height.unwrap_or(u16::MAX)))
            },
        };
        (dim, resized)
    }

    pub fn iter_widgets<F: FnMut(&Node, &Widget)>(
        &self,
        map: &HashMap<NodeId, Node>,
        func: &mut F,
    ) {
        for id in &self.children {
            match map.get(id) {
                Some(Node{kind: NodeKind::Layout(layout), ..}) => {
                    layout.iter_widgets(map, func);
                },
                Some(node @ Node{kind: NodeKind::Widget(widget), ..}) => {
                    func(node, widget);
                },
                None => (),
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum NodeKind {
    Widget(Widget),
    Layout(Layout),
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum NodeId {
    Normal(usize),
    Special(usize),
}

impl From<NodeId> for usize {
    fn from(value: NodeId) -> Self {
        match value {
            NodeId::Normal(id) | NodeId::Special(id) => id
        }
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
         write!(f, "{}", usize::from(*self))
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: NodeId,
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

    fn is_visible(&self, map: &HashMap<NodeId, Node>, tmp: bool) -> bool {
        if self.is_hidden() || self.get_size(tmp).1 == 0 {
            false
        } else if let NodeKind::Layout(layout) = &self.kind && !layout.is_visible(map, tmp) {
            false
        } else {
            true
        }
    }

    pub fn get_draw_pos(&self, map: &HashMap<NodeId, Node>) -> Option<(u16, u16)> {
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

    pub(super) fn refresh<'a>(
        &self,
        map: &HashMap<NodeId, Node>,
        max_width: u16,
        max_height: Option<u16>,
        tmp: bool,
        mut resized: Option<&'a mut Vec<usize>>,
    ) -> ((u16, u16), Option<&'a mut Vec<usize>>) {

        if self.is_hidden() {
            if !tmp && let Some(resized) = &mut resized {
                self.collect_hidden_children(map, resized);
            }
            self.set_size((0, 0), tmp);
            return ((0, 0), resized);
        }

        let height = match &self.kind {
            NodeKind::Widget(widget) => widget.get_height_for_width(max_width, None),
            NodeKind::Layout(layout) => {
                let dim;
                (dim, resized) = layout.refresh(map, max_width, max_height, tmp, resized);
                dim.1
            },
        };
        let height = height.min(max_height.unwrap_or(height));
        let height = self.height_spec.into_size(max_height, Some(height)).size;
        let dim = (max_width, height);

        if dim != self.get_size(tmp) {
            self.set_size(dim, tmp);
            if !tmp && let NodeId::Normal(id) = self.id && let Some(resized) = &mut resized {
                resized.push(id);
            }
        }

        (dim, resized)
    }

    fn collect_hidden_children(&self, map: &HashMap<NodeId, Node>, vec: &mut Vec<usize>) {
        // assume self.is_hidden()
        debug_assert!(self.is_hidden());

        if self.get_size(false) != (0, 0) && let NodeKind::Layout(layout) = &self.kind {
            self.set_size((0, 0), false);
            if let NodeId::Normal(id) = self.id {
                vec.push(id);
            }
            for id in &layout.children {
                if let NodeId::Normal(id) = id {
                    vec.push(*id);
                }
            }
            for id in &layout.children {
                if let Some(node) = map.get(id) {
                    node.collect_hidden_children(map, vec);
                }
            }
        }
    }

}

#[derive(Default)]
pub struct Nodes {
    pub(super) map: HashMap<NodeId, Node>,
    root: Layout,
    counter: usize,
    pub size: Cell<(u16, u16)>,
}

impl Nodes {
    fn next_id(&mut self) -> usize {
        self.counter += 1;
        self.counter
    }

    pub fn add(&mut self, kind: NodeKind, special: bool) -> &mut Node {
        let id = self.next_id();
        let id = if special {
            NodeId::Special(id)
        } else {
            NodeId::Normal(id)
        };

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

    pub fn get_node(&self, id: NodeId) -> Option<&Node> {
        self.map.get(&id)
    }

    pub fn get_node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
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

    pub fn invalidate_ephemeral(&mut self, id: NodeId) -> bool {
        match self.map.get(&id) {
            Some(Node{kind: NodeKind::Layout(layout), ..}) => {
                let mut ids = vec![];
                layout.iter_widgets(&self.map, &mut |node, _widget| {
                    ids.push(node.id);
                });
                for id in ids {
                    if let Some(Node{kind: NodeKind::Widget(widget), ..}) = self.map.get_mut(&id) {
                        widget.ephemeral.clear();
                    }
                }
                true
            },
            Some(_) => {
                let Some(Node{kind: NodeKind::Widget(widget), ..}) = self.map.get_mut(&id)
                    else { unreachable!() };
                widget.ephemeral.clear();
                true
            },
            None => false,
        }
    }


    /// Remove a node
    pub fn remove(&mut self, id: NodeId) -> Option<Node> {
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
    pub fn remove_child_from_parent(&mut self, child_id: NodeId) {
        for layout in self.get_layouts_mut() {
            layout.children.retain(|&id| id != child_id);
        }
    }

    /// Add an existing node as a child of a layout. Removes it from any current parent first.
    pub fn add_child(&mut self, child_id: NodeId) {
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

    pub fn refresh(&mut self, max_width: u16, max_height: Option<u16>) -> Vec<usize> {
        // draw any cursors
        // this is such a hack
        for node in self.map.values_mut() {
            if !node.is_hidden() && let NodeKind::Widget(widget) = &mut node.kind {
                widget.make_cursor_space_hl();
            }
        }

        let mut resized = vec![];
        self.size.set(self.root.refresh(&self.map, max_width, max_height, false, Some(&mut resized)).0);
        resized
    }

    fn iter_widgets<F: FnMut(&Node, &Widget)>(&self, mut func: F) {
        self.root.iter_widgets(&self.map, &mut func);
    }

    pub fn trigger_ephemeral_callbacks<F: FnMut(NodeId, &mut Widget, usize)>(&mut self, tmp: bool, mut func: F) {
        let mut data = vec![];

        self.iter_widgets(|node, widget| {
            let size = node.get_size(tmp);
            let border_height = widget.border.inner_height();
            let height = size.1.saturating_sub(border_height);
            let (_, line_range) = widget.scroll.position.get_approx_line_range(Some(height as _), widget.inner.len());

            for lineno in line_range {
                if widget.ephemeral.index_for_lineno(lineno).is_err() {
                    data.push((node.id, lineno));
                }
            }
        });

        for (id, lineno) in data {
            if let Some(Node{kind: NodeKind::Widget(widget), ..}) = self.map.get_mut(&id) {
                func(id, widget, lineno);
            }
        }
    }

    pub fn render<W: Write, C: Canvas>(
        &self,
        drawer: &mut Drawer<W, C>,
        tmp: bool,
    ) -> std::io::Result<()> {
        let mut renderer = NodeRenderer::new_for_layout(&self.root, &self.map, tmp);
        renderer.render(drawer, false, true, NoRendererCallback::None)?;
        drawer.clear_to_end_of_line(None, false)
    }

    pub fn render_node<W: Write, C: Canvas>(
        &self,
        node: &Node,
        drawer: &mut Drawer<W, C>,
        tmp: bool,
    ) -> std::io::Result<()> {
        let mut renderer = NodeRenderer::new(node, &self.map, tmp);
        renderer.render(drawer, false, true, NoRendererCallback::None)?;
        drawer.clear_to_end_of_line(None, false)
    }
}

enum NodeRenderer<'a, I> {
    VerticalLayout {
        children: I,
        child: Option<Box<NodeRenderer<'a, I>>>,
        map: &'a HashMap<NodeId, Node>,
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

impl<'a> NodeRenderer<'a, std::slice::Iter<'a, NodeId>> {

    fn new_for_layout(
        layout: &'a Layout,
        map: &'a HashMap<NodeId, Node>,
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
        map: &'a HashMap<NodeId, Node>,
        tmp: bool,
    ) -> Self {

        let size = node.get_size(tmp);
        match &node.kind {
            NodeKind::Layout(layout) => Self::new_for_layout(layout, map, tmp),
            NodeKind::Widget(widget) => {
                let renderer = TextRenderer::new(
                    &widget.inner, // text
                    0, // initial_indent
                    Some(&widget.border), // border
                    size.0 as _, // width
                    Some(size.1 as _), // height
                    Some(widget.scroll), // scroll
                    |lineno| {
                        widget.inner.highlights.get_for_lineno(lineno).iter()
                            .sorted_merge_with(widget.ephemeral.get_for_lineno(lineno).iter())
                            .sorted_merge_with(widget.cursor_space_hl.iter().filter(move |hl| hl.lineno == lineno))
                    },
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

impl<'a> Renderer for NodeRenderer<'a, std::slice::Iter<'a, NodeId>> {
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
        F: FnMut(&mut Drawer<W, C>, usize, Range<usize>)
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
