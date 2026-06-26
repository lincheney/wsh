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

#[derive(Debug)]
pub enum Action {
    Done{exit: bool},
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
                let ui = self.try_borrow()?;
                buf == &[ui.termios_input_flags.eof] && ui.buffer.get_contents().is_empty()
            };
            if is_eof {
                self.shell.exit(0);
                // this should error as we exit
                if let Some(result) = self.shell.accept_line(None) {
                    let _ = result.await;
                }
                return Ok(Some(Action::Done{exit: true}));
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
                self.handle_default(event, buf).await
            },
            KeybindValue::Widget(widget) => {

                let mut new_buffer = None;
                let mut new_cursor = None;
                let mut accept_line = widget.is_accept_line();

                if !accept_line {
                    // execute the widget

                    let result = self.shell.trampoline_out_callback(Box::new(move |ui: Ui, token| {

                        let (old_buffer, old_cursor) = {
                            let ui = ui.try_borrow()?;
                            (ui.buffer.get_contents().clone(), ui.buffer.get_cursor())
                        };
                        ui.shell.set_zle_buffer(old_buffer.clone(), old_cursor as _);
                        ui.shell.set_lastchar(lastchar);

                        ui.exec_widget(&widget, token)?;

                        let (buffer, cursor) = ui.shell.get_zle_buffer();
                        let cursor = cursor.unwrap_or(buffer.len() as _) as _;

                        anyhow::Ok((
                            (old_buffer != buffer).then_some(buffer),
                            (old_cursor != cursor).then_some(cursor),
                            ui.shell.has_accepted_line(),
                        ))
                    })).await;

                    (new_buffer, new_cursor, accept_line) = result??;
                }

                {
                    if let Some(buffer) = &new_buffer {
                        self.insert_or_set_buffer(false, buffer, new_cursor.take()).await?;
                    }
                    if let Some(cursor) = new_cursor {
                        ui.try_borrow_mut()?.buffer.set_cursor(cursor);
                    }
                }

                if new_buffer.is_some() {
                    self.trigger_buffer_change_callbacks().await?;
                }
                // anything could have happened, so trigger a redraw
                self.queue_draw();

                // this widget may have called accept-line somewhere inside
                let exit = accept_line && !self.accept_line().await?;
                Ok(Some(Action::Done{exit}))
            },
        }
    }

    async fn handle_default(&mut self, event: &Event, _buf: &BStr) -> Result<Option<Action>> {
        match event {
            Event::Key(KeyEvent{ key: Key::Char(c), modifiers }) if modifiers.difference(Modifiers::SHIFT).is_empty() => {
                let mut buf = [0; 4];
                let c = c.encode_utf8(&mut buf).as_bytes();
                self.insert_or_set_buffer(true, c, None).await?;
                self.trigger_buffer_change_callbacks().await?;
                self.queue_draw();
                Ok(Some(Action::Done{exit: false}))
            },

            Event::Key(KeyEvent{ key: Key::Enter, modifiers }) if modifiers.difference(Modifiers::SHIFT).is_empty() => {
                self.accept_line().await.map(|success| Some(Action::Done{exit: !success}))
            },

            Event::BracketedPaste(data) => {
                self.trigger_paste_callbacks(data).await?;
                Ok(Some(Action::Done{exit: false}))
            },

            _ => Ok(None),
        }
    }

    async fn handle_mapping(&mut self, mut mapping: BString) -> Result<Option<Action>> {
        // shucks, gotta do recursion
        let mut exit = false;
        for _hop in 0..20 {
            let mut parser = crate::keybind::parser::Parser::default();
            parser.feed(mapping.as_ref());
            mapping.clear();

            for (event, buf) in parser.iter() {
                match self.handle_simple(&event, buf.as_ref()).await? {
                    Some(Action::Done{exit: x}) => {
                        exit = exit || x;
                    },
                    Some(Action::Mapping(mut string)) => {
                        mapping.append(&mut string);
                    },
                    None => (),
                }
            }

            if mapping.is_empty() {
                return Ok(Some(Action::Done{exit}))
            }
        }
        log::error!("exceeded recursion limit trying to execute mapping");

        // TODO we still have a mapping
        Ok(Some(Action::Done{exit: false}))
    }

}
