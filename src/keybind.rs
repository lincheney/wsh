use crate::lua::{HasEventCallbacks};
use crate::shell::{KeybindValue};
use crate::ui::Ui;
use anyhow::Result;
use bstr::{BStr, BString};
pub mod parser;
pub mod mouse;
pub mod event;
pub mod key;

pub use mouse::{MouseEvent, Mouse};
pub use key::{KeyEvent, Key};
pub use event::{Event, EventIndex};

pub const CONTROL_C_BYTE: u8 = KeyEvent{key: Key::Char('c'), modifiers: Modifiers::CONTROL}.try_into_byte().unwrap();

bitflags::bitflags! {
    #[derive(Debug, PartialEq, Eq, Copy, Clone, Hash)]
    pub struct Modifiers: u8 {
        const NONE    = 0;
        const SHIFT   = 1;
        const ALT     = 2;
        const CONTROL = 4;
    }
}

pub enum Action {
    Done{success: bool},
    Mapping(BString),
}

pub struct KeyHandler<'a>( pub &'a mut Ui );
crate::impl_deref_helper!(self: KeyHandler<'a>, &self.0 => Ui);
crate::impl_deref_helper!(mut self: KeyHandler<'a>, &mut self.0 => Ui);

impl KeyHandler<'_> {

    pub async fn handle(&mut self, event: &Event, buf: &BStr) -> Result<Option<Action>> {
        match self.handle_simple(event, buf).await? {
            Some(Action::Mapping(mapping)) => self.handle_mapping(mapping).await,
            action => Ok(action),
        }
    }

    async fn handle_simple(&mut self, event: &Event, buf: &BStr) -> Result<Option<Action>> {
        if let Some(result) = crate::lua::invoke_keybind_callback(self.0, event).await? {
            return Ok(Some(result));
        }

        if buf.len() == 1 {
            // zsh doesn't run widgets if eof
            let is_eof = {
                let ui = self.borrow();
                buf == &[ui.termios_input_flags.eof] && ui.buffer.get_contents().is_empty()
            };
            if is_eof {
                self.shell.exit(0);
                // this should error as we exit
                let _ = self.shell.accept_line(None).await;
                return Ok(Some(Action::Done{success: false}));
            }
        }

        let mut lastchar = [0; 4];
        let len = buf.len().min(lastchar.len());
        lastchar[..len].copy_from_slice(&buf[..len]);

        // look for a zle widget
        let ui = self.clone();

        let Some(keybind) = KeybindValue::find(crate::shell::MetaString::from(buf.to_owned()).as_ref())
            else { return self.handle_default(event, buf).await };

        match keybind {
            KeybindValue::String(string) => {
                // recurse
                Ok(Some(Action::Mapping(string)))
            },
            // skip not found or where we have our own impl
            KeybindValue::Widget(widget) if widget.is_self_insert() || widget.is_undefined_key() => {
                // continue to default
                Ok(None)
            },
            KeybindValue::Widget(mut widget) => {

                let mut new_buffer = None;
                let mut new_cursor = None;
                let mut output = None;
                let mut accept_line = widget.is_accept_line();

                if !accept_line {
                    // execute the widget
                    let lock = if widget.is_internal() {
                        // are all internal widgets safe to run without locking ui?
                        // they should all output only to shout which we are already capturing?
                        None
                    } else {
                        // a widget may run subprocesses so lock the ui
                        Some(ui.has_foreground_process.lock().await)
                    };
                    let (old_buffer, old_cursor) = {
                        let ui = self.borrow();
                        (ui.buffer.get_contents().clone(), ui.buffer.get_cursor())
                    };

                    self.shell.set_zle_buffer(old_buffer.clone(), old_cursor as _);
                    self.shell.set_lastchar(lastchar);

                    output = self.shell.trampoline_out_callback(Box::new(move |ui: Ui, token| {
                        Some(widget.exec_and_get_output(token, &ui.shell, None, [].into_iter()).0)
                    })).await?;

                    let (buffer, cursor) = self.shell.get_zle_buffer();
                    let cursor = cursor.unwrap_or(buffer.len() as _) as _;
                    new_buffer = (old_buffer != buffer).then_some(buffer);
                    new_cursor = (old_cursor != cursor).then_some(cursor);
                    accept_line = self.shell.has_accepted_line();
                    drop(lock);
                }

                {
                    if let Some(buffer) = &new_buffer {
                        self.insert_or_set_buffer(false, buffer, new_cursor.take()).await;
                    }

                    let mut ui = self.borrow_mut();

                    // check for any output e.g. zle -M
                    if let Some(output) = &output {
                        ui.tui.add_zle_message(output.as_ref());
                    }
                    ui.buffer.set(None, new_cursor);
                }

                if new_buffer.is_some() {
                    self.trigger_buffer_change_callbacks().await;
                }
                // anything could have happened, so trigger a redraw
                self.queue_draw();

                // this widget may have called accept-line somewhere inside
                let success = !accept_line || self.accept_line().await?;
                Ok(Some(Action::Done{success}))
            },
        }
    }

    async fn handle_default(&mut self, event: &Event, _buf: &BStr) -> Result<Option<Action>> {
        match event {
            Event::Key(KeyEvent{ key: Key::Char(c), modifiers }) if modifiers.difference(Modifiers::SHIFT).is_empty() => {
                let mut buf = [0; 4];
                let c = c.encode_utf8(&mut buf).as_bytes();
                self.insert_or_set_buffer(true, c, None).await;
                self.trigger_buffer_change_callbacks().await;
                self.queue_draw();
                Ok(Some(Action::Done{success: true}))
            },

            Event::Key(KeyEvent{ key: Key::Enter, modifiers }) if modifiers.difference(Modifiers::SHIFT).is_empty() => {
                self.accept_line().await.map(|success| Some(Action::Done{success}))
            },

            Event::BracketedPaste(data) => {
                self.trigger_paste_callbacks(data).await;
                Ok(Some(Action::Done{success: true}))
            },

            _ => Ok(None),
        }
    }

    async fn handle_mapping(&mut self, mut mapping: BString) -> Result<Option<Action>> {
        // shucks, gotta do recursion
        let mut success = true;
        for _hop in 0..20 {
            let mut parser = crate::keybind::parser::Parser::default();
            parser.feed(mapping.as_ref());
            mapping.clear();

            for (event, buf) in parser.iter() {
                match self.handle_simple(&event, buf.as_ref()).await? {
                    Some(Action::Done{success: x}) => {
                        success = success && x;
                    },
                    Some(Action::Mapping(mut string)) => {
                        mapping.append(&mut string);
                    },
                    None => (),
                }
            }

            if mapping.is_empty() {
                return Ok(Some(Action::Done{success}))
            }
        }

        // TODO we still have a mapping
        Ok(Some(Action::Done{success: false}))
    }

}
