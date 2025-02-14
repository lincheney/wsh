use std::default::Default;
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
    backend::Backend,
    buffer::Buffer,
};
use mlua::{prelude::*, UserData, UserDataMethods, MetaMethod};
use crate::ui::Ui;
use crate::shell::Shell;

#[derive(Copy, Clone, PartialEq)]
pub struct WidgetId(usize);

struct Widget{
    id: WidgetId,
    constraint: Constraint,
    inner: Paragraph<'static>,
}

pub struct Tui {
    terminal: ratatui::DefaultTerminal,
    counter: usize,
    widgets: Vec<Widget>,

    old_buffer: Buffer,
    new_buffer: Buffer,
}

impl std::default::Default for Tui {
    fn default() -> Self {
        Self{
            terminal: ratatui::init_with_options(TerminalOptions{ viewport: Viewport::Inline(0) }),
            counter: 0,
            widgets: vec![],

            old_buffer: Default::default(),
            new_buffer: Default::default(),
        }
    }
}

impl Tui {

    pub fn add(&mut self, string: String, persist: bool) -> WidgetId {
        let id = WidgetId(self.counter);
        self.counter += 1;
        let inner = Paragraph::new(string);
        self.widgets.push(Widget{
            id,
            inner,
            constraint: Constraint::Max(1),
        });
        id
    }

    pub fn draw(&mut self, stdout: &mut std::io::Stdout, width: u16, height: u16, cursory: u16) -> Result<()> {
        let max_height = height * 2 / 3;
        let cursory = cursory + 1;
        let mut area = Rect{
            x: 0,
            y: cursory,
            width,
            height: max_height,
        };

        self.old_buffer.resize(area);
        self.new_buffer.resize(area);

        let mut frame = self.terminal.get_frame();
        std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);

        let layout = Layout::vertical(self.widgets.iter().map(|w| w.constraint));
        let layouts = layout.split(area);

        for (widget, layout) in self.widgets.iter().zip(layouts.iter()) {
            frame.render_widget(&widget.inner, *layout);
        }
        std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);

        let new_height = {
            let trailing_empty_lines = self.new_buffer.content()
                .chunks(self.new_buffer.area.width as _)
                .rev()
                .take_while(|line| line.iter().all(|c| *c == ratatui::buffer::Cell::EMPTY))
                .count();
            self.new_buffer.area.height - trailing_empty_lines as u16
        };

        if new_height > 0 {

            let allocate_more_space = (cursory + new_height + 1).saturating_sub(height);
            if allocate_more_space > 0 {
                area.y = area.y.saturating_sub(allocate_more_space - 1);
                self.old_buffer.resize(area);
                self.new_buffer.resize(area);
                self.old_buffer.reset();
            }

            let updates = self.old_buffer.diff(&self.new_buffer);
            if !updates.is_empty() {
                queue!(stdout, crossterm::terminal::BeginSynchronizedUpdate)?;
                if allocate_more_space > 0 {
                    for _ in 0 .. new_height as _ {
                        queue!(stdout, style::Print("\n"))?;
                    }
                    queue!(stdout, cursor::MoveUp(new_height), cursor::SavePosition)?;
                }
                queue!(stdout, cursor::MoveToNextLine(1))?;

                self.terminal.backend_mut().draw(updates.into_iter())?;
                std::mem::swap(&mut self.new_buffer, &mut self.old_buffer);

                queue!(stdout, cursor::RestorePosition, crossterm::terminal::EndSynchronizedUpdate)?;
            }
        }

        Ok(())
    }

}

impl UserData for WidgetId {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
    }
}

async fn show_message(
    ui: Ui,
    _shell: Shell,
    _lua: Lua,
    (val, persist): (String, Option<bool>),
) -> Result<WidgetId> {
    let persist = persist.unwrap_or(false);
    Ok(ui.borrow_mut().await.tui.add(val, persist))
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("show_message", shell, show_message).await?;

    Ok(())
}
