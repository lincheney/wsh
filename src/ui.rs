use bstr::{BStr, BString};
use std::future::Future;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::default::Default;
use mlua::prelude::*;
use anyhow::Result;
use crate::keybind::parser::{Event, KeyEvent, Key, KeyModifiers, CONTROL_C_BYTE};
use crate::fork_lock::{ForkLock, RawForkLock, ForkLockReadGuard};
use crate::print_lock::{PrintLock, PrintLockGuard};
use nix::sys::termios;

use crossterm::{
    terminal::{Clear, ClearType, BeginSynchronizedUpdate, EndSynchronizedUpdate},
    cursor::MoveToColumn,
    event,
    style,
    execute,
    queue,
};
use crate::tui::{
    MoveDown,
};

use crate::timed_lock::{RwLock};
use crate::shell::{ShellClient, KeybindValue, process::PidMap};
use crate::lua::{EventCallbacks, HasEventCallbacks};
pub mod buffer;

fn lua_error<T>(msg: &str) -> Result<T, mlua::Error> {
    Err(mlua::Error::RuntimeError(msg.to_string()))
}

enum KeybindOutput {
    String(BString),
    Value(Result<bool>),
}

pub struct UiInner {
    pub tui: crate::tui::Tui,
    pub cmdline: crate::tui::command_line::CommandLineState,

    pub dirty: bool,
    pub keybinds: Vec<crate::lua::KeybindMapping>,
    pub keybind_layer_counter: usize,

    pub buffer: buffer::Buffer,
    pub status_bar: crate::tui::status_bar::StatusBar,

    pub stdout: std::io::Stdout,
    enhanced_keyboard: bool,
    pub size: (u32, u32),
    pub intr: u8,
}

pub struct UnlockedUi {
    pub inner: RwLock<UiInner>,
    pub event_callbacks: std::sync::Mutex<EventCallbacks> ,
}

impl UnlockedUi {
    pub fn borrow(&self) -> tokio::sync::RwLockReadGuard<'_, UiInner> {
        self.inner.blocking_read()
    }

    pub fn borrow_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, UiInner> {
        self.inner.blocking_write()
    }
}

crate::strong_weak_wrapper! {
    pub struct Ui {
        pub unlocked: ForkLock<'static, UnlockedUi>,
        pub shell: ShellClient,
        pub lua: Lua,
        pub events: ForkLock<'static, crate::event_stream::EventController>,
        pub has_foreground_process: tokio::sync::Mutex<()>,

        print_lock: PrintLock,
        is_drawing: AtomicBool,

        pub pid_map: ForkLock<'static, std::sync::Mutex<PidMap>>,
    }
}

impl Ui {

    pub fn new(
        lock: &'static RawForkLock,
        events: crate::event_stream::EventController,
        shell: ShellClient,
    ) -> Result<Self> {

        let lua = Lua::new();
        let lua_api = lua.create_table()?;
        lua.globals().set("wish", &lua_api)?;

        let mut ui = UiInner{
            dirty: true,
            tui: Default::default(),
            cmdline: Default::default(),
            buffer: buffer::Buffer::new(),
            status_bar: Default::default(),
            keybinds: Default::default(),
            keybind_layer_counter: Default::default(),
            stdout: std::io::stdout(),
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            size: (1, 1),
            intr: CONTROL_C_BYTE,
        };
        ui.keybinds.push(Default::default());

        ui.reset();

        let ui = UnlockedUi {
            inner: RwLock::new(ui),
            event_callbacks: Default::default(),
        };
        let ui = Self {
            unlocked: Arc::new(lock.wrap(ui)),
            lua: Arc::new(lua),
            events: Arc::new(lock.wrap(events)),
            shell: Arc::new(shell),
            has_foreground_process: Default::default(),
            print_lock: Default::default(),
            is_drawing: Arc::new(false.into()),
            pid_map: Arc::new(lock.wrap(Default::default())),
        };

        Ok(ui)
    }


    pub fn get(&self) -> ForkLockReadGuard<'_, UnlockedUi> {
        self.unlocked.read()
    }

    pub fn get_lua_api(&self) -> LuaResult<LuaTable> {
        self.lua.globals().get("wish")
    }

    pub async fn start_cmd(&self, buffer: Option<&BString>) -> Result<()> {
        self.trigger_precmd_callbacks(buffer).await;
        self.draw().await
    }

    pub fn queue_draw(&self) {
        if !crate::is_forked() && !self.is_drawing.swap(true, Ordering::AcqRel) {
            self.events.read().queue_draw();
        }
    }

    pub async fn draw(&self) -> Result<()> {
        self.is_drawing.store(false, Ordering::Release);
        if let Ok(mut lock) = self.print_lock.try_lock() && lock.get_value() == 0 {
            self.draw_with_lock(&mut lock).await
        } else {
            // the shell will draw it later
            Ok(())
        }
    }

    async fn draw_with_lock(&self, _lock: &mut PrintLockGuard<'_>) -> Result<()> {
        let mut size = None;
        let mut shell_vars = None;
        let mut cursor_y = None;

        loop {
            let need_shell_vars;
            let mut need_cursor_y = false;

            let size = {
                let this = self.unlocked.read();
                let ui = &mut *this.borrow_mut();

                if size == Some(ui.size) {
                    return ui.draw(shell_vars, cursor_y);
                }
                // if the size has changed, recompute everything

                size = Some(ui.size);
                let (width, height) = ui.size;
                // redraw all if dimensions have changed
                if height != ui.tui.max_height || width != ui.tui.get_size().0 as _ {
                    ui.tui.max_height = height;
                    ui.dirty = true;
                    need_cursor_y = true;
                }

                if !(ui.dirty || ui.buffer.dirty || ui.tui.dirty || ui.status_bar.dirty) {
                    return Ok(())
                }

                need_shell_vars = ui.dirty || ui.cmdline.is_dirty();

                if !need_cursor_y && !need_shell_vars {
                    // don't need to refresh anything, draw immediately
                    return ui.draw(shell_vars, cursor_y)
                }
                ui.size
            };

            // get the shell vars and cursor y then reacquire the ui
            if need_shell_vars {
                shell_vars = Some(crate::tui::command_line::CommandLineState::get_shell_vars(&self.shell, size.0).await?);
            }
            if need_cursor_y {
                let cursor = self.events.read().get_cursor_position();
                cursor_y = Some(tokio::time::timeout(crate::timed_lock::DEFAULT_DURATION, cursor).await.unwrap()?.1 as _);
            }
        }
    }

    pub async fn call_lua_fn<T: IntoLuaMulti + mlua::MaybeSend + 'static>(&self, draw: bool, callback: mlua::Function, arg: T) {
        let result = callback.call_async::<LuaValue>(arg).await;
        let mut ui = self.clone();
        if !ui.report_error(result) && draw {
            ui.queue_draw();
        }
    }

    pub fn report_error<T, E: std::fmt::Display>(&mut self, result: std::result::Result<T, E>) -> bool {
        if let Err(err) = result {
            log::error!("{}", err);
            self.show_error_message(&format!("ERROR: {err}"));
            true
        } else {
            false
        }
    }

    pub fn show_error_message(&mut self, msg: &str) {
        let this = self.get();
        let mut ui = this.borrow_mut();
        ui.tui.add_error_message(msg);
        self.queue_draw();
    }

    pub async fn handle_event(&mut self, event: Event, event_buffer: BString) -> Result<bool> {
        if let Event::Key(event) = event {
            self.trigger_key_callbacks(&event.into(), &event_buffer).await;
            if let Some(result) = self.handle_key(event, event_buffer.as_ref()).await {
                self.cancel_completion_suffix();
                return result
            }

        }
        self.handle_key_default(event, event_buffer.as_ref()).await?;
        self.cancel_completion_suffix();
        Ok(true)
    }

    pub async fn handle_window_resize(&self, width: u32, height: u32) -> Result<bool> {
        self.get().borrow_mut().size = (width, height);
        self.queue_draw();
        self.trigger_window_resize_callbacks(width, height).await;
        Ok(true)
    }

    pub async fn set_vintr(&self, intr: u8) -> Result<()> {
        let _fg_lock = self.has_foreground_process.lock().await;
        let _print_lock = self.print_lock.lock_exclusive().await;

        let this = self.get();
        let mut ui = this.borrow_mut();
        ui.intr = intr;
        ui.apply_intr(intr)?;
        Ok(())
    }

    pub async fn handle_interrupt(&self) -> Result<bool> {
        // sigint
        // cancel the current command line?

        self.insert_or_set_buffer(false, b"", None).await;
        if self.shell.accept_line_trampoline(Some("".into())).await.is_err() {
            return Ok(false)
        }
        {
            let this = &*self.unlocked.read();
            let mut ui = this.borrow_mut();
            ui.reset();
        }
        self.trigger_buffer_change_callbacks().await;
        self.start_cmd(Some(&"".into())).await?;
        Ok(true)
    }

    fn pre_accept_line<'a>(&'a self, lock: &mut PrintLockGuard<'a>) -> Result<()> {
        {
            let this = &*self.unlocked.read();
            let mut ui = this.borrow_mut();

            ui.tui.clear_non_persistent();

            // TODO handle errors here properly
        }
        self.events.read().pause();
        self.prepare_for_unhandled_output_blocking(Some(lock))?;
        Ok(())
    }

    pub fn zle_cmd_trash(&self) -> Result<bool> {
        if self.print_lock.zle_cmd_trash() {
            self.prepare_for_unhandled_output_blocking(None)
        } else {
            Ok(false)
        }
    }

    fn prepare_for_unhandled_output_blocking<'a>(&'a self, lock: Option<&mut PrintLockGuard<'a>>) -> Result<bool> {
        // TODO if forked and trashed, zsh will NOT recover
        // we're going go to end up with janky output
        // how do we solve this?
        if !crate::is_forked() {

            let mut print_lock;
            let print_lock = if let Some(lock) = lock {
                lock
            } else {
                print_lock = self.print_lock.blocking_lock();
                &mut print_lock
            };
            self.unlocked.read().borrow_mut().prepare_for_unhandled_output()?;
            print_lock.acquire();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn prepare_for_unhandled_output(&self) -> Result<bool> {
        // TODO if forked and trashed, zsh will NOT recover
        // we're going go to end up with janky output
        // how do we solve this?
        if !crate::is_forked() {
            let mut print_lock = self.print_lock.lock().await;
            self.unlocked.read().borrow_mut().prepare_for_unhandled_output()?;
            print_lock.acquire();
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn zle_cmd_refresh(&self) -> Result<bool> {
        if self.print_lock.zle_cmd_refresh() {
            self.recover_from_unhandled_output(None).await
        } else {
            Ok(false)
        }
    }

    pub async fn recover_from_unhandled_output<'a>(&'a self, lock: Option<&mut PrintLockGuard<'a>>) -> Result<bool> {
        let mut print_lock;
        let print_lock = if let Some(lock) = lock {
            lock
        } else {
            print_lock = self.print_lock.lock().await;
            &mut print_lock
        };

        assert_ne!(print_lock.get_value(), 0);
        if print_lock.get_value() == 1 {

            {
                let this = self.unlocked.read();
                let ui = this.borrow();
                ui.activate()?;
            }

            // move down one line if not at start of line
            let cursor = self.events.read().get_cursor_position();
            let cursor = tokio::time::timeout(crate::timed_lock::DEFAULT_DURATION, cursor).await.unwrap().unwrap_or((0, 0));

            let this = self.unlocked.read();
            let ui = &mut *this.borrow_mut();
            if cursor.0 != 0 {
                queue!(ui.stdout, style::Print("\r\n"))?;
            }
            execute!(ui.stdout, style::ResetColor)?;
            ui.cmdline.make_command_line(&mut ui.buffer).hard_reset();
            ui.dirty = true;
        }

        print_lock.release();
        Ok(print_lock.get_value() == 0)
    }

    async fn post_accept_line<'a>(&'a self, lock: &mut PrintLockGuard<'a>) -> Result<()> {
        {
            let this = &*self.unlocked.read();
            let mut ui = this.borrow_mut();
            ui.reset();
        }
        self.events.read().unpause();
        self.recover_from_unhandled_output(Some(lock)).await?;
        Ok(())
    }

    pub async fn accept_line(&mut self) -> Result<bool> {
        if crate::is_forked() {
            return Ok(false)
        }

        let buffer = {
            let buffer = {
                let this = self.get();
                let ui = this.borrow();
                ui.buffer.get_contents().clone()
            };
            let (complete, _tokens) = self.shell.parse(buffer.clone(), Default::default()).await?;
            if complete {
                Some(buffer)
            } else {
                None
            }
        };

        // time to execute
        if let Some(buffer) = buffer {
            self.trigger_accept_line_callbacks(&buffer).await;

            {
                let fg_lock = self.has_foreground_process.lock().await;
                let mut print_lock = self.print_lock.lock_exclusive().await;

                // last draw
                crate::log_if_err(self.draw_with_lock(&mut print_lock).await);
                self.pre_accept_line(&mut print_lock)?;
                // acceptline doesn't actually accept the line right now
                // only when we return control to zle using the trampoline
                if self.shell.accept_line_trampoline(Some(buffer.clone())).await.is_err() {
                    return Ok(false)
                }
                self.post_accept_line(&mut print_lock).await?;
                drop(print_lock);
                drop(fg_lock);
            }

            self.trigger_buffer_change_callbacks().await;
            self.start_cmd(Some(&buffer)).await?;

        } else {
            self.insert_or_set_buffer(true, b"\n", None).await;
            self.trigger_buffer_change_callbacks().await;
            self.draw().await?;
        }

        Ok(true)
    }

    pub fn set_lua_fn<F, A, R>(&self, name: &str, func: F) -> LuaResult<()>
    where
        F: Fn(&Self, &Lua, A) -> Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {

        let func = self.make_lua_fn(func)?;
        self.get_lua_api()?.set(name, func)
    }

    pub fn make_lua_fn<F, A, R>(&self, func: F) -> LuaResult<LuaFunction>
    where
        F: Fn(&Self, &Lua, A) -> Result<R> + mlua::MaybeSend + 'static,
        A: FromLuaMulti,
        R: IntoLuaMulti,
    {
        let weak = self.downgrade();
        self.lua.create_function(move |lua, value| {
            let ui = weak.try_upgrade()?;
            func(&ui, lua, value)
                .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
        })
    }

    pub fn set_lua_async_fn<F, A, R, T>(&self, name: &str, func: F) -> LuaResult<()>
    where
        F: Fn(Self, Lua, A) -> T + mlua::MaybeSend + 'static + Clone,
        A: FromLuaMulti + Send + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + mlua::MaybeSend + 'static,
    {
        let func = self.make_lua_async_fn(func)?;
        self.get_lua_api()?.set(name, func)
    }

    pub fn make_lua_async_fn<F, A, R, T>(&self, func: F) -> LuaResult<LuaFunction>
    where
        F: Fn(Self, Lua, A) -> T + mlua::MaybeSend + 'static + Clone,
        A: FromLuaMulti + Send + 'static,
        R: IntoLuaMulti,
        T: Future<Output=Result<R>> + mlua::MaybeSend + 'static,
    {
        let weak = self.downgrade();
        self.lua.create_async_function(move |lua, value| {
            let weak = weak.clone();
            let func = func.clone();
            async move {
                let ui = weak.try_upgrade()?;
                func(ui, lua, value).await
                    .map_err(|e| mlua::Error::RuntimeError(e.to_string()))
            }
        })
    }

    pub fn init_lua(&self) -> Result<()> {
        crate::lua::init_lua(self)?;

        self.lua.load(/*lua*/ r#"
            local xdg_data = os.getenv('XDG_DATA_HOME')
            local home = os.getenv('HOME')
            local base = xdg_data or (home and home .. '/.local/share')
            local wish_path = base and (base .. '/wish/lua/?.lua;') or ''
            local p = (';' .. package.path .. ';'):gsub(';%./%?%.lua;', ''):gsub('^;', ''):gsub(';$', '')
            package.path = wish_path .. p
        "#).exec()?;
        self.lua.load("require('wish')").exec()?;
        Ok(())
    }

    async fn handle_key(&mut self, event: KeyEvent, buf: &BStr) -> Option<Result<bool>> {
        let mut mapping = match self.handle_key_simple(event, buf).await? {
            KeybindOutput::Value(x) => return Some(x),
            KeybindOutput::String(string) => string,
        };

        // shucks, gotta do recursion
        let mut success = true;
        for _hop in 0..20 {
            let mut parser = crate::keybind::parser::Parser::default();
            parser.feed(mapping.as_ref());
            mapping.clear();
            for (event, buf) in parser.iter() {
                if let Event::Key(event) = event {
                    match self.handle_key_simple(event, buf.as_ref()).await {
                        Some(KeybindOutput::Value(x)) => {
                            if let Ok(x) = x {
                                success = success && x;
                                continue
                            }
                            return Some(x) // error
                        },
                        Some(KeybindOutput::String(mut string)) => {
                            mapping.append(&mut string);
                            continue
                        },
                        None => (),
                    }
                }

                let x = self.handle_key_default(event, buf.as_ref()).await;
                if let Ok(x) = x {
                    success = success && x;
                } else {
                    return Some(x) // error
                }
            }

            if mapping.is_empty() {
                return Some(Ok(success))
            }
        }

        // TODO we still have a mapping
        // let mut ui = self.inner.borrow_mut();
        // ui.buffer.insert_at_cursor(string.as_ref());
        // return Some(Ok(true))

        Some(Ok(true))
    }

    async fn handle_key_simple(&mut self, event: KeyEvent, buf: &BStr) -> Option<KeybindOutput> {
        enum Value {
            String(BString),
            Widget{buffer: Option<BString>, cursor: Option<usize>, output: Option<BString>, accept_line: bool},
        }

        // look for a lua callback
        let result = self.get().borrow().keybinds
            .iter()
            .rev()
            .find_map(|k| {
                if let Some(callback) = k.inner.get(&(event.key, event.modifiers)) {
                    Some(Some(callback.clone()))
                } else if k.no_fallthrough {
                    Some(None)
                } else {
                    None
                }
            });

        match result {
            Some(Some(callback)) => {
                self.call_lua_fn(true, callback, ()).await;
                return Some(KeybindOutput::Value(Ok(true)))
            },
            Some(None) => {
                // no fallthrough
                return Some(KeybindOutput::Value(Ok(true)))
            },
            None => (), // fallthrough
        }

        let mut lastchar = [0; 4];
        let len = buf.len().min(lastchar.len());
        lastchar[..len].copy_from_slice(&buf[..len]);

        // look for a zle widget
        let ui = self.clone();
        let buf: crate::shell::MetaString = buf.to_owned().into();

        let result = self.shell.run(move |shell| {
            match KeybindValue::find(shell, buf.as_ref()) {
                Some(KeybindValue::String(string)) => {
                    // recurse
                    Some(Value::String(string))
                },
                // skip not found or where we have our own impl
                Some(KeybindValue::Widget(widget)) if widget.is_self_insert() || widget.is_undefined_key() => None,
                Some(KeybindValue::Widget(widget)) if widget.is_accept_line() => {
                    Some(Value::Widget{accept_line: true, buffer: None, cursor: None, output: None})
                },
                Some(KeybindValue::Widget(mut widget)) => {
                    // execute the widget
                    // a widget may run subprocesses so lock the ui
                    let lock = ui.has_foreground_process.blocking_lock();
                    let (buffer, cursor) = {
                        let this = ui.get();
                        let ui = this.inner.blocking_write();
                        (ui.buffer.get_contents().clone(), ui.buffer.get_cursor())
                    };

                    widget.shell.set_zle_buffer(buffer.clone(), cursor as _);

                    widget.shell.set_lastchar(lastchar);
                    // executing a widget may block
                    let (output, _) = tokio::task::block_in_place(|| widget.exec_and_get_output(None, [].into_iter()));
                    let (new_buffer, new_cursor) = shell.get_zle_buffer();
                    let new_cursor = new_cursor.unwrap_or(new_buffer.len() as _) as _;
                    let new_buffer = (new_buffer != buffer).then_some(new_buffer);
                    let new_cursor = (new_cursor != cursor).then_some(new_cursor);
                    let accept_line = shell.has_accepted_line();
                    drop(lock);

                    Some(Value::Widget{
                        buffer: new_buffer,
                        cursor: new_cursor,
                        output: if output.is_empty() { None } else { Some(output) },
                        accept_line,
                    })
                },
                None => None,
            }
        }).await;
        let result = crate::log_if_err(result)??;

        match result {
            Value::String(string) => Some(KeybindOutput::String(string)),
            Value::Widget{buffer, mut cursor, output, accept_line} => {
                {
                    if let Some(buffer) = &buffer {
                        self.insert_or_set_buffer(false, buffer, cursor.take()).await;
                    }

                    let this = self.get();
                    let mut ui = this.borrow_mut();

                    // check for any output e.g. zle -M
                    if let Some(output) = &output {
                        ui.tui.add_zle_message(output.as_ref());
                    }
                    ui.buffer.set(None, cursor);
                    // anything could have happened, so trigger a redraw
                    ui.dirty = true;
                }

                if buffer.is_some() {
                    self.trigger_buffer_change_callbacks().await;
                }
                // anything could have happened, so trigger a redraw
                self.queue_draw();

                // this widget may have called accept-line somewhere inside
                if accept_line {
                    Some(KeybindOutput::Value(self.accept_line().await))
                } else {
                    Some(KeybindOutput::Value(Ok(true)))
                }
            },
        }
    }

    async fn handle_key_default(&mut self, event: Event, _buf: &BStr) -> Result<bool> {
        match event {

            Event::Key(KeyEvent{ key: Key::Char(c), modifiers }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                let mut buf = [0; 4];
                let c = c.encode_utf8(&mut buf).as_bytes();
                self.insert_or_set_buffer(true, c, None).await;
                self.trigger_buffer_change_callbacks().await;
                self.queue_draw();
            },

            Event::Key(KeyEvent{ key: Key::Enter, modifiers }) if modifiers.difference(KeyModifiers::SHIFT).is_empty() => {
                return self.accept_line().await;
            },

            Event::BracketedPaste(data) => {
                self.trigger_paste_callbacks(&data).await;
            },

            _ => (),
        }
        Ok(true)
    }

    fn cancel_completion_suffix(&self) {
        self.get().borrow_mut().buffer.replace_completion_suffix(None);
    }

    pub async fn insert_or_set_buffer(&self, insert: bool, data: &[u8], cursor: Option<usize>) {
        // if we need to invoke a shfunc, need to trampline out
        let (func, num_chars, old_buffer, old_cursor) = {
            let ui = self.get();
            let buffer = &mut ui.borrow_mut().buffer;

            let insert = if insert {
                Some(data)
            } else {
                buffer.convert_to_insert(data)
            };

            if let Some(insert) = insert {
                // check suffix auto removal
                if let Some((pos, suffix)) = buffer.replace_completion_suffix(None)
                    && pos == buffer.get_cursor()
                    && buffer.cursor_byte_pos() >= suffix.byte_len
                    && suffix.matches(Some(insert.into()))
                {

                    match suffix.try_into_func() {
                        Err(suffix) => {
                            // easy, but no longer a plain insert
                            buffer.splice_at(buffer.cursor_byte_pos() - suffix.byte_len, insert, suffix.byte_len, true);
                            buffer.set(None, cursor);
                            return
                        },
                        Ok((func, num_chars)) => {
                            // pita = trampoline
                            (func, num_chars, buffer.get_contents().clone(), buffer.get_cursor())
                        },
                    }

                } else {
                    buffer.insert_at_cursor(insert);
                    buffer.set(None, cursor);
                    return
                }

            } else {
                buffer.set(Some(data), cursor);
                return
            }
        };

        // invoke the func, then reacquire the buf

        // execute the func
        // a func may run subprocesses so lock the ui
        let lock = self.has_foreground_process.lock().await;
        let zle = self.shell.run(move |shell| {
            shell.set_zle_buffer(old_buffer, old_cursor as _);
            shell.exec_function_by_name(func, vec![num_chars.to_string().into()]);
            shell.get_zle_buffer()
        }).await;
        drop(lock);

        let ui = self.get();
        let buffer = &mut ui.borrow_mut().buffer;
        if let Some(zle) = crate::log_if_err(zle) {
            buffer.set(Some(&zle.0), Some(zle.1.unwrap_or(zle.0.len() as _) as usize));
        }
        // finally add the data we wanted originally
        buffer.set(Some(data), cursor);
    }

    pub async fn freeze_if<T, F: Future<Output = T>>(
        &self,
        condition: bool,
        freeze_events: bool,
        f: F,
    ) -> Result<T> {

        let mut lock = if condition && !crate::is_forked() {
            // this essentially locks ui
            if freeze_events {
                self.events.read().pause();
            }
            self.prepare_for_unhandled_output().await?;
            Some(self.has_foreground_process.lock().await)
        } else {
            None
        };

        let result = f.await;

        if let Some(lock) = lock.take() {
            if freeze_events {
                self.events.read().unpause();
            }
            let recovered = self.recover_from_unhandled_output(None).await;
            drop(lock);
            if crate::log_if_err(recovered) == Some(true) {
                self.queue_draw();
            }
        }

        Ok(result)
    }

}

impl UiInner {
    pub fn activate(&self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;

        // onlcr in case bg processes are outputting things
        let mut attrs = termios::tcgetattr(&self.stdout)?;
        attrs.output_flags.insert(termios::OutputFlags::OPOST | termios::OutputFlags::ONLCR);
        attrs.local_flags.insert(termios::LocalFlags::ISIG);
        attrs.control_chars[termios::SpecialCharacterIndices::VINTR as usize] = self.intr;
        nix::sys::termios::tcsetattr(&self.stdout, termios::SetArg::TCSADRAIN, &attrs)?;

        if self.enhanced_keyboard {
            // queue!(
                // self.stdout.lock(),
                // event::PushKeyboardEnhancementFlags(
                    // event::KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    // | event::KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                    // | event::KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                    // | event::KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                // )
            // )?;
        }

        execute!(
            self.stdout.lock(),
            event::EnableBracketedPaste,
            event::EnableFocusChange,
            // event::EnableMouseCapture,
        )?;
        Ok(())
    }

    pub fn apply_intr(&self, intr: u8) -> Result<()> {
        let mut attrs = termios::tcgetattr(&self.stdout)?;
        attrs.control_chars[termios::SpecialCharacterIndices::VINTR as usize] = intr;
        nix::sys::termios::tcsetattr(&self.stdout, termios::SetArg::TCSADRAIN, &attrs)?;
        Ok(())
    }

    pub fn deactivate(&mut self) -> Result<()> {
        if self.enhanced_keyboard {
            // queue!(self.stdout, event::PopKeyboardEnhancementFlags)?;
        }

        self.apply_intr(CONTROL_C_BYTE)?; // control c

        execute!(
            self.stdout,
            event::DisableBracketedPaste,
            event::DisableFocusChange,
            // event::DisableMouseCapture
        )?;

        crossterm::terminal::disable_raw_mode()?;
        Ok(())
    }

    fn reset(&mut self) {
        self.buffer.reset();
        self.tui.reset();
        self.status_bar.reset();
        self.dirty = true;
    }

    fn prepare_for_unhandled_output(&mut self) -> Result<()> {
        self.deactivate()?;
        let y_offset = self.cmdline.y_offset_to_end();
        self.dirty = true;

        // move to last line of buffer
        queue!(
            self.stdout,
            BeginSynchronizedUpdate,
            MoveDown(y_offset),
        )?;

        if self.cmdline.cursor_coord.0 != 0 {
            queue!(
                self.stdout,
                style::Print('\n'),
                MoveToColumn(0),
            )?;
        }

        execute!(
            self.stdout,
            Clear(ClearType::FromCursorDown),
            EndSynchronizedUpdate,
        )?;
        Ok(())
    }


    fn draw(&mut self, shell_vars: Option<crate::tui::command_line::ShellVars>, cursor_y: Option<u32>) -> Result<()> {
        if let Some(shell_vars) = shell_vars {
            self.cmdline.shell_vars = shell_vars;
        }
        let cmdline = self.cmdline.make_command_line(&mut self.buffer);
        self.tui.draw(
            &mut self.stdout,
            self.size,
            cursor_y,
            cmdline,
            &mut self.status_bar,
            self.dirty,
        )?;
        self.dirty = false;
        Ok(())
    }

}

impl Drop for UiInner {
    fn drop(&mut self) {
        if let Err(err) = self.deactivate() {
            log::error!("{:?}", err);
        }
    }
}

impl WeakUi {
    pub fn try_upgrade(&self) -> LuaResult<Ui> {
        if let Some(ui) = self.upgrade() {
            Ok(ui)
        } else {
            lua_error("ui not running")
        }
    }
}
