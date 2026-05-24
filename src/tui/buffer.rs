use std::ops::{Index, IndexMut};
use super::cell::Cell;
use super::rect::Rect;
use super::drawer::Canvas;

pub struct Buffer {
    pub area:    Rect,
    pub content: Vec<Cell>,
}

impl Default for Buffer {
    fn default() -> Self {
        Self { area: Rect::default(), content: vec![] }
    }
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
}

impl Index<(u16, u16)> for Buffer {
    type Output = Cell;
    fn index(&self, (x, y): (u16, u16)) -> &Cell {
        &self.content[(y * self.area.width + x) as usize]
    }
}

impl IndexMut<(u16, u16)> for Buffer {
    fn index_mut(&mut self, (x, y): (u16, u16)) -> &mut Cell {
        &mut self.content[(y * self.area.width + x) as usize]
    }
}

impl Canvas for Buffer {
    fn get_cell(&self, pos: (u16, u16)) -> &Cell {
        &self[pos]
    }

    fn get_cell_mut(&mut self, pos: (u16, u16)) -> &mut Cell {
        &mut self[pos]
    }

    fn set_cell(&mut self, pos: (u16, u16), cell: &Cell) {
        self[pos] = cell.clone();
    }

    fn get_cell_range(&self, start: (u16, u16), end: (u16, u16)) -> &[Cell] {
        let s = (start.1 * self.area.width + start.0) as usize;
        let e = (end.1   * self.area.width + end.0)   as usize;
        &self.content[s..e]
    }

    fn get_cell_range_mut(&mut self, start: (u16, u16), end: (u16, u16)) -> &mut [Cell] {
        let s = (start.1 * self.area.width + start.0) as usize;
        let e = (end.1   * self.area.width + end.0)   as usize;
        &mut self.content[s..e]
    }

    fn get_size(&self) -> (u16, u16) {
        (self.area.width, self.area.height)
    }
}
