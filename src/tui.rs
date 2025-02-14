use std::default::Default;
use serde::{Deserialize};
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
use mlua::{prelude::*, UserData, UserDataMethods};
use crate::ui::Ui;
use crate::shell::Shell;

#[derive(Copy, Clone, PartialEq)]
pub struct WidgetId(usize);

struct Widget{
    id: WidgetId,
    constraint: Constraint,
    inner: Paragraph<'static>,
}

#[derive(Debug, Deserialize)]
struct WidgetOptions {
    #[serde(default)]
    text: String,
    #[serde(default)]
    persist: bool,
}

pub struct Tui {
    terminal: ratatui::DefaultTerminal,
    counter: usize,
    widgets: Vec<Widget>,

    dirty: bool,
    width: u16,
    height: u16,

    old_buffer: Buffer,
    new_buffer: Buffer,
}

impl std::default::Default for Tui {
    fn default() -> Self {
        Self{
            terminal: ratatui::init_with_options(TerminalOptions{ viewport: Viewport::Inline(0) }),
            counter: 0,
            widgets: vec![],
            dirty: false,
            width: 0,
            height: 0,

            old_buffer: Default::default(),
            new_buffer: Default::default(),
        }
    }
}

impl Tui {

    pub fn add(&mut self, options: WidgetOptions) -> WidgetId {
        let id = WidgetId(self.counter);
        self.counter += 1;
        self.dirty = true;
        let inner = Paragraph::new(options.text);
        self.widgets.push(Widget{
            id,
            inner,
            constraint: Constraint::Max(1),
        });
        id
    }

    fn refresh(&mut self, width: u16, height: u16) {
        self.dirty = false;
        self.width = width;
        self.height = height;

        let area = Rect{
            x: 0,
            y: 0,
            width,
            height,
        };

        self.old_buffer.resize(area);
        self.new_buffer.resize(area);

        let mut frame = self.terminal.get_frame();
        std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);

        // assume each widget needs at least 1 line
        let widgets = &self.widgets[..area.height as _];
        let layout = Layout::vertical(widgets.iter().map(|w| w.constraint));
        let layouts = layout.split(area);

        for (widget, layout) in widgets.iter().zip(layouts.iter()) {
            frame.render_widget(&widget.inner, *layout);
        }
        std::mem::swap(frame.buffer_mut(), &mut self.new_buffer);
    }

    fn swap_buffers(&mut self) {
        std::mem::swap(&mut self.new_buffer, &mut self.old_buffer);
    }

    pub fn draw(&mut self, stdout: &mut std::io::Stdout, width: u16, height: u16, cursory: u16) -> Result<()> {
        if self.widgets.is_empty() {
            return Ok(())
        }

        let max_height = height * 2 / 3;

        if self.dirty || max_height != self.height || width != self.width {
            self.swap_buffers();
            self.new_buffer.reset();
            self.refresh(width, max_height);
        }

        let cursory = cursory + 1;
        self.old_buffer.area.y = cursory;
        self.new_buffer.area.y = cursory;

        let actual_height = {
            let trailing_empty_lines = self.new_buffer.content()
                .chunks(self.new_buffer.area.width as _)
                .rev()
                .take_while(|line| line.iter().all(|c| *c == ratatui::buffer::Cell::EMPTY))
                .count();
            self.new_buffer.area.height - trailing_empty_lines as u16
        };

        if actual_height > 0 {

            let allocate_more_space = (cursory + actual_height + 1).saturating_sub(height);
            if allocate_more_space > 0 {
                let y = self.old_buffer.area.y.saturating_sub(allocate_more_space - 1);
                self.old_buffer.area.y = y;
                self.new_buffer.area.y = y;
                self.old_buffer.reset();
            }

            let updates = self.old_buffer.diff(&self.new_buffer);
            if !updates.is_empty() {
                queue!(stdout, crossterm::terminal::BeginSynchronizedUpdate)?;
                if allocate_more_space > 0 {
                    for _ in 0 .. actual_height as _ {
                        queue!(stdout, style::Print("\n"))?;
                    }
                    queue!(stdout, cursor::MoveUp(actual_height), cursor::SavePosition)?;
                }
                queue!(stdout, cursor::MoveToNextLine(1))?;

                self.terminal.backend_mut().draw(updates.into_iter())?;

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
    lua: Lua,
    val: LuaValue,
) -> Result<WidgetId> {
    let options: WidgetOptions = lua.from_value(val)?;
    Ok(ui.borrow_mut().await.tui.add(options))
}

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    ui.set_lua_async_fn("show_message", shell, show_message).await?;

    Ok(())
}
