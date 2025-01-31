#[derive(Debug, Default)]
pub struct Buffer {
    pub contents: String,
    pub cursor: usize,
}

impl Buffer {

    fn fix_cursor(&mut self) {
        if self.cursor > self.contents.len() {
            self.cursor = self.contents.len()
        }
    }

    pub fn mutate<F: FnMut(&mut String, &mut usize)->R, R>(&mut self, mut func: F) -> R {
        let value = func(&mut self.contents, &mut self.cursor);
        self.fix_cursor();
        value
    }

    pub fn get_contents(&self) -> &String {
        &self.contents
    }

    pub fn get_cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_contents(&mut self, contents: String) {
        self.contents = contents;
        self.fix_cursor();
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
        self.fix_cursor();
    }

    pub fn reset(&mut self) {
        self.contents.clear();
        self.cursor = 0;
    }

}
