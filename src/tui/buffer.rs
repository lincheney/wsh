use super::cell::Cell;
use super::rect::Rect;
use super::drawer::Canvas;

#[derive(Default)]
pub struct Buffer {
    pub area:    Rect,
    pub content: Vec<Cell>,
}

impl Buffer {
    pub fn resize(&mut self, area: Rect) {
        let new_len = (area.width as usize) * (area.height as usize);
        self.area = area;
        self.content.resize_with(new_len, Cell::default);
    }

    pub fn reset(&mut self) {
        for cell in &mut self.content {
            cell.reset();
        }
    }

    fn coord_to_index(&self, (x, y): (u16, u16)) -> usize {
        (y * self.area.width + x) as _
    }

    fn get(&self, pos: (u16, u16)) -> Option<&Cell> {
        let index = self.coord_to_index(pos);
        if cfg!(debug_assertions) {
            if index >= self.content.len() {
                log::error!("Trying to draw outside of bounds: size={:?}, pos={pos:?}", self.get_size());
            }
        }
        self.content.get(index)
    }

    fn get_mut(&mut self, pos: (u16, u16)) -> Option<&mut Cell> {
        let index = self.coord_to_index(pos);
        if cfg!(debug_assertions) {
            if index >= self.content.len() {
                log::error!("Trying to draw outside of bounds: size={:?}, pos={pos:?}", self.get_size());
            }
        }
        self.content.get_mut(index)
    }
}

impl Canvas for Buffer {
    fn get_cell(&self, pos: (u16, u16)) -> Option<&Cell> {
        self.get(pos)
    }

    fn get_cell_mut(&mut self, pos: (u16, u16)) -> Option<&mut Cell> {
        self.get_mut(pos)
    }

    fn set_cell(&mut self, pos: (u16, u16), cell: &Cell) {
        if let Some(c) = self.get_cell_mut(pos) {
            *c = cell.clone();
        }
    }

    fn get_cell_range(&self, start: (u16, u16), end: (u16, u16)) -> &[Cell] {
        let start = self.coord_to_index(start);
        let end = self.coord_to_index(end);
        let len = self.content.len();
        if cfg!(debug_assertions) {
            if start >= len || end > len {
                // log::error!("Trying to draw outside of bounds: len={len}, start={start:?}, end={end:?}");
            }
        }
        &self.content[start.min(len)..end.min(len)]
    }

    fn get_cell_range_mut(&mut self, start: (u16, u16), end: (u16, u16)) -> &mut [Cell] {
        let start = self.coord_to_index(start);
        let end = self.coord_to_index(end);
        let len = self.content.len();
        if cfg!(debug_assertions) {
            if start >= len || end > len {
                // log::error!("Trying to draw outside of bounds: len={len}, start={start:?}, end={end:?}");
            }
        }
        &mut self.content[start..end]
    }

    fn get_size(&self) -> (u16, u16) {
        (self.area.width, self.area.height)
    }
}
