use anyhow::Result;
use crossterm::{
    cursor,
    style,
    queue,
};
use ratatui::{
    *,
    text::*,
    layout::*,
    widgets::*,
    style::*,
};

pub struct Tui {
    terminal: ratatui::DefaultTerminal,
}

impl std::default::Default for Tui {
    fn default() -> Self {
        let terminal = ratatui::init_with_options(TerminalOptions{ viewport: Viewport::Inline(0) });
        Self{
            terminal
        }
    }
}

impl Tui {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn draw(&mut self, stdout: &mut std::io::Stdout, width: u16, height: u16, cursory: u16) -> Result<()> {
        let max_height = height * 2 / 3;
        let cursory = cursory + 1;
        let rect = Rect{
            x: 0,
            y: cursory,
            width,
            height: max_height,
        };

        for _ in 0..2 {
            let buffer = self.terminal.current_buffer_mut();
            buffer.resize(rect);
            self.terminal.swap_buffers();
        }

        let mut frame = self.terminal.get_frame();

        let done = 1;
        let NUM_DOWNLOADS = 3;
        let progress = LineGauge::default()
            .filled_style(Style::default().fg(Color::Blue))
            .label(format!("{done}/{NUM_DOWNLOADS}"))
            .ratio(done as f64 / NUM_DOWNLOADS as f64);
        frame.render_widget(progress, rect);

        let new_height = {
            let buffer = self.terminal.current_buffer_mut();
            let width = buffer.area.width;
            let trailing_empty_lines = buffer.content()
                .chunks(width as _)
                .rev()
                .take_while(|line| line.iter().all(|c| *c == ratatui::buffer::Cell::EMPTY))
                .count();
            buffer.area.height - trailing_empty_lines as u16
        };

        for _ in 0 .. new_height as _ {
            queue!(stdout, style::Print("\n"))?;
        }
        queue!(
            stdout,
            cursor::MoveUp(new_height),
            cursor::SavePosition,
            cursor::MoveToNextLine(1),
        )?;
        self.terminal.flush()?;
        self.terminal.swap_buffers();
        queue!(stdout, cursor::RestorePosition)?;

        Ok(())
    }

}
