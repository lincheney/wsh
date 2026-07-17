use std::ops::ControlFlow;
use std::cell::{Cell, RefCell, BorrowError, BorrowMutError};
use std::rc::Rc;
use bstr::{BString};
use std::future::Future;
use std::default::Default;
use mlua::prelude::*;
use anyhow::Result;
use crate::keybind::{Event};
use crate::print_lock::{PrintLock, PrintLockGuard};
use nix::sys::termios;
use crate::shell::{Shell, signals::sigchld::PidMap, ParserOptions};
use crate::lua::{LuaWrapper, EventCallbacks, HasEventCallbacks};
pub mod buffer;

use crossterm::{
    terminal::{Clear, ClearType, BeginSynchronizedUpdate, EndSynchronizedUpdate},
    cursor::{MoveToColumn, SetCursorStyle},
    event,
    style,
    execute,
    queue,
};
use crate::tui::{
    MoveDown,
};

const ENABLE_SGR_MOUSE: style::Print<&str> = style::Print("\x1b[?1000;1006h");
const DISABLE_SGR_MOUSE: style::Print<&str> = style::Print("\x1b[?1000;1006l");

pub struct TermiosInputFlags {
    pub intr: u8,
    pub eof: u8,
}

#[derive(Clone)]
pub struct Ui(pub Rc<_Ui>);
crate::impl_deref_helper!(self: Ui, &self.0 => Rc<_Ui>);

pub struct _Ui {
    pub inner: RefCell<UiInner>,
    pub shell: Shell,
    pub lua: crate::lua::LuaWrapper,
    pub events: crate::event_stream::EventController,
    pub has_foreground_process: tokio::sync::Mutex<()>,
    pub print_lock: PrintLock,
    pub is_drawing: Cell<bool>,
    pub runtime: crate::async_runtime::Runtime,
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

    pub termios_input_flags: TermiosInputFlags,
    pub mouse_mode: bool,
    pub cursor_style: Option<SetCursorStyle>,

    pub pid_map: PidMap,
    pub event_callbacks: EventCallbacks ,
}

pub type WeakUi = std::rc::Weak<_Ui>;

impl Ui {

    pub fn new(
        events: crate::event_stream::EventController,
        shell: Shell,
        runtime: crate::async_runtime::Runtime,
    ) -> Result<Self> {

        let lua = LuaWrapper::new()?;
        let stdout = std::io::stdout();
        let termios = termios::tcgetattr(&stdout)?;

        let mut ui = UiInner {
            dirty: true,
            tui: Default::default(),
            cmdline: Default::default(),
            buffer: buffer::Buffer::new(),
            status_bar: Default::default(),
            keybinds: Default::default(),
            keybind_layer_counter: Default::default(),
            stdout,
            enhanced_keyboard: crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false),
            size: (1, 1),
            termios_input_flags: TermiosInputFlags {
                intr: crate::keybind::CONTROL_C_BYTE,
                eof: termios.control_chars[termios::SpecialCharacterIndices::VEOF as usize],
            },
            mouse_mode: false,
            cursor_style: None,
            pid_map: Default::default(),
            event_callbacks: Default::default(),
        };
        ui.keybinds.push(Default::default());

        ui.reset();

        let ui = _Ui {
            inner: RefCell::new(ui),
            lua,
            events,
            shell,
            has_foreground_process: Default::default(),
            print_lock: Default::default(),
            is_drawing: Default::default(),
            runtime,
        };
        let ui = Self(Rc::new(ui));
        ui.lua.ui.replace(ui.downgrade());

        Ok(ui)
    }


    pub fn try_borrow(&self) -> Result<std::cell::Ref<'_, UiInner>, BorrowError> {
        self.inner.try_borrow()
    }

    pub fn try_borrow_mut(&self) -> Result<std::cell::RefMut<'_, UiInner>, BorrowMutError> {
        self.inner.try_borrow_mut()
    }

    pub async fn start_cmd(&self, buffer: Option<&BString>) -> Result<()> {
        self.trigger_precmd_callbacks(buffer).await?;
        self.draw().await
    }

    pub fn queue_draw(&self) {
        if !crate::is_forked() {
            let old = self.is_drawing.replace(true);
            if !old {
                self.events.queue_draw();
            }
        }
    }

    pub async fn draw(&self) -> Result<()> {
        self.is_drawing.set(false);
        if let Ok(mut lock) = self.print_lock.try_lock() && lock.get_value() == 0 {
            let resized = self.draw_with_lock(&mut lock).await?;
            if !resized.is_empty() {
                self.trigger_message_resize_callbacks(&resized).await?;
            }
            Ok(())
        } else {
            // the shell will draw it later
            Ok(())
        }
    }

    pub fn draw_blocking(&self, force: bool) -> Result<()> {
        self.is_drawing.set(false);
        if let Ok(mut lock) = self.print_lock.try_lock() && lock.get_value() == 0 {
            let mut size = None;
            self.draw_with_lock_blocking(&mut lock, &mut size, None, force)?;
            Ok(())
        } else {
            // the shell will draw it later
            Ok(())
        }
    }

    async fn draw_with_lock(&self, lock: &mut PrintLockGuard<'_>) -> Result<Vec<usize>> {
        let mut size = None;
        let mut cursor_y = None;

        loop {
            if let Some(result) = self.draw_with_lock_blocking(lock, &mut size, cursor_y, false)? {
                return Ok(result)
            }

            // get the cursor y then reacquire the ui next loop
            let cursor = self.events.get_cursor_position();
            cursor_y = Some(tokio::time::timeout(crate::DEFAULT_DURATION, cursor).await.unwrap()?.1 as _);
        }
    }

    fn draw_with_lock_blocking(
        &self,
        _lock: &mut PrintLockGuard<'_>,
        size: &mut Option<(u32, u32)>,
        cursor_y: Option<u32>,
        force: bool,
    ) -> Result<Option<Vec<usize>>> {

        let mut ui = self.try_borrow_mut()?;

        if *size != Some(ui.size) {

            // if the size has changed, recompute everything
            *size = Some(ui.size);
            let (width, height) = ui.size;
            // redraw all if dimensions have changed
            if height != ui.tui.max_height || width != ui.tui.get_size().0.into() {
                ui.tui.max_height = height;
                ui.dirty = true;
            }

            if !(ui.dirty || ui.buffer.dirty || ui.tui.dirty || ui.status_bar.dirty || ui.cmdline.is_dirty()) {
                return Ok(Some(vec![]))
            }

            if ui.dirty && !force {
                // TOO dirty, need the cursor position
                return Ok(None);
            }

        }

        // grab shell vars as late as possible
        if (ui.dirty || ui.cmdline.is_dirty()) && ui.cmdline.uses_shell_vars() {
            let width = ui.size.0;
            ui.cmdline.update_shell_vars(&self.shell, width);
        }

        Ok(Some(ui.draw(cursor_y)?))
    }

    pub async fn call_lua_fn<T: IntoLuaMulti + 'static>(&self, draw: bool, callback: mlua::Function, arg: T) -> Result<Option<LuaValue>> {
        let result = crate::lua::call_lua_fn(&callback, arg).await;

        match result {
            Ok(result) => Ok(Some(result)),
            err => {
                if !self.report_error(err)? && draw {
                    self.queue_draw();
                }
                Ok(None)
            },
        }
    }

    pub fn report_error<T, E: std::fmt::Display>(&self, result: std::result::Result<T, E>) -> Result<bool> {
        if let Err(err) = result {
            log::error!("{err}");
            self.show_error_message(&format!("ERROR: {err}"))?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn show_error_message(&self, msg: &str) -> Result<()> {
        let mut ui = self.try_borrow_mut()?;
        ui.tui.add_error_message(msg);
        self.queue_draw();
        Ok(())
    }

    pub async fn handle_event(&mut self, event: Event, event_buffer: BString) -> Result<bool> {
        match event {
            Event::Key(ev) => self.trigger_key_callbacks(&ev.into(), &event_buffer).await?,
            Event::Mouse(ev) => self.trigger_mouse_callbacks(&ev.into(), &event_buffer).await?,
            _ => (),
        }

        let result = crate::keybind::KeyHandler(self).handle(&event, event_buffer.as_ref()).await;
        self.cancel_completion_suffix()?;
        match result? {
            Some(crate::keybind::Action::Done{exit: true}) => Ok(false),
            _ => Ok(true),
        }
    }

    pub async fn handle_window_resize(&self, width: u32, height: u32) -> Result<bool> {
        self.try_borrow_mut()?.size = (width, height);
        self.queue_draw();
        self.trigger_window_resize_callbacks(width, height).await?;
        Ok(true)
    }

    pub async fn set_vintr(&self, intr: u8) -> Result<()> {
        let _fg_lock = self.has_foreground_process.lock().await;
        let _print_lock = self.print_lock.lock_exclusive().await;

        let mut ui = self.try_borrow_mut()?;
        ui.termios_input_flags.intr = intr;
        ui.apply_intr(intr)?;
        Ok(())
    }

    pub fn handle_interrupt(&self) {
        // sigint
        // cancel the current command line?

        let ui = self.clone();
        crate::spawn_and_log::<_, _, anyhow::Error>(self, async move {
            if let Some(result) = ui.shell.accept_line(Some(b"".into())) && result.await.is_err() {
                return Ok(())
            }
            ui.try_borrow_mut()?.reset();
            ui.trigger_buffer_change_callbacks().await?;
            ui.start_cmd(Some(&"".into())).await?;
            Ok(())
        });

    }

    pub fn handle_sigchld_shout(&self, shout: BString) {
        ::log::debug!("DEBUG(whines)\t{}\t= {:?}", stringify!(shout), shout);
    }

    fn pre_accept_line<'a>(&'a self, lock: &mut PrintLockGuard<'a>) -> Result<()> {
        {
            self.try_borrow_mut()?.tui.clear_non_persistent();
            // TODO handle errors here properly
        }
        self.events.pause();
        self.prepare_for_unhandled_output_blocking(Some(lock), true)?;
        Ok(())
    }

    pub fn zle_cmd_trash(&self) -> Result<bool> {
        if self.print_lock.zle_cmd_trash() {
            self.prepare_for_unhandled_output_blocking(None, true)
        } else {
            Ok(false)
        }
    }

    fn prepare_for_unhandled_output_blocking<'a>(&'a self, lock: Option<&mut PrintLockGuard<'a>>, end_sync: bool) -> Result<bool> {
        // TODO if forked and trashed, zsh will NOT recover
        // we're going go to end up with janky output
        // how do we solve this?
        if crate::is_forked() {
            Ok(false)
        } else {

            let mut print_lock;
            let print_lock = if let Some(lock) = lock {
                lock
            } else {
                print_lock = self.print_lock.try_lock().unwrap();
                &mut print_lock
            };
            self.try_borrow_mut()?.prepare_for_unhandled_output(end_sync)?;
            print_lock.acquire();
            Ok(true)
        }
    }

    pub async fn prepare_for_unhandled_output(&self, end_sync: bool) -> Result<bool> {
        // TODO if forked and trashed, zsh will NOT recover
        // we're going go to end up with janky output
        // how do we solve this?
        if crate::is_forked() {
            Ok(false)
        } else {
            let mut print_lock = self.print_lock.lock().await;
            self.try_borrow_mut()?.prepare_for_unhandled_output(end_sync)?;
            print_lock.acquire();
            Ok(true)
        }
    }

    pub async fn zle_cmd_refresh(&self) -> Result<bool> {
        if self.print_lock.zle_cmd_refresh() {
            self.recover_from_unhandled_output(None).await
        } else {
            Ok(false)
        }
    }

    pub async fn recover_from_unhandled_output<'a>(
        &'a self,
        lock: Option<&mut PrintLockGuard<'a>>,
    ) -> Result<bool> {

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
                self.try_borrow()?.activate()?;
            }

            // move down one line if not at start of line
            let cursor = self.events.get_cursor_position();
            let cursor = tokio::time::timeout(crate::DEFAULT_DURATION, cursor).await.unwrap().unwrap_or((0, 0));

            let ui = &mut *self.try_borrow_mut()?;
            if cursor.0 != 0 {
                queue!(ui.stdout, style::Print("\r\n"))?;
            }
            execute!(ui.stdout, style::ResetColor)?;
            ui.dirty = true;
            ui.cmdline.make_command_line(&mut ui.buffer).hard_reset();
            self.queue_draw();
        }

        print_lock.release();
        Ok(print_lock.get_value() == 0)
    }

    pub fn exec_widget(&self, widget: &crate::shell::ZleWidget, token: crate::shell::TrampolineToken) -> Result<i32> {
        // widget may do anything so need to freeze

        // pause events
        self.events.pause();
        // acquire locks
        let fg_lock = self.runtime.block_on(self.has_foreground_process.lock());
        // back to cooked mode etc
        {
            let mut ui = self.try_borrow_mut()?;
            ui.tui.clear_zle();
            if !crate::is_forked() {
                ui.deactivate();
            }
        }

        let options = crate::shell::WidgetArgs {
            capture_shout: false,
            passthrough_shout: true,
            ..Default::default()
        };
        let (code, _output) = widget.exec(token, &self.shell, Some(options), [].into_iter());

        // unpause events
        self.events.unpause();

        let redraw = crate::shell::is_interrupted();
        {
            let mut ui = self.try_borrow_mut()?;
            ui.activate()?;
            if redraw {
                ui.dirty = true;
            }
        }
        if redraw {
            self.queue_draw();
        }

        // release locks
        drop(fg_lock);
        Ok(code)
    }

    async fn post_accept_line<'a>(&'a self, lock: &mut PrintLockGuard<'a>) -> Result<()> {
        {
            self.try_borrow_mut()?.reset();
        }
        self.events.unpause();
        self.recover_from_unhandled_output(Some(lock)).await?;
        Ok(())
    }

    pub async fn accept_line(&mut self) -> Result<bool> {
        if crate::is_forked() {
            return Ok(false)
        }

        let buffer = {
            let buffer = {
                let ui = self.try_borrow()?;
                ui.buffer.get_contents().clone()
            };
            let (complete, _tokens) = self.shell.parse(
                buffer.clone(),
                ParserOptions::default(),
            );
            ::log::debug!("DEBUG(zloty) \t{}\t= {:?}", stringify!(_tokens), _tokens);
            ::log::debug!("DEBUG(manses)\t{}\t= {:?}", stringify!(complete), complete);
            ::log::debug!("DEBUG(judged)\t{}\t= {:?}", stringify!(buffer), buffer);
            if complete {
                Some(buffer)
            } else {
                None
            }
        };

        // time to execute
        if let Some(buffer) = buffer {
            self.trigger_accept_line_callbacks(&buffer).await?;

            {
                let fg_lock = self.has_foreground_process.lock().await;
                let mut print_lock = self.print_lock.lock_exclusive().await;

                // last draw
                crate::log_if_err(self.draw_with_lock(&mut print_lock).await);
                self.pre_accept_line(&mut print_lock)?;
                // acceptline doesn't actually accept the line right now
                // only when we return control to zle using the trampoline
                let Some(result) = self.shell.accept_line(Some(buffer.clone())) else {
                    return Ok(false)
                };
                if result.await.is_err() {
                    return Ok(false)
                }
                self.post_accept_line(&mut print_lock).await?;
                drop(print_lock);
                drop(fg_lock);
            }

            self.trigger_buffer_change_callbacks().await?;
            self.start_cmd(Some(&buffer)).await?;

        } else {
            self.insert_or_set_buffer(true, b"\n", None).await?;
            self.trigger_buffer_change_callbacks().await?;
            self.draw().await?;
        }

        Ok(true)
    }

    pub fn downgrade(&self) -> WeakUi {
        Rc::downgrade(&self.0)
    }

    pub fn try_upgrade(weak: &WeakUi) -> Result<Self> {
        if let Some(ui) = weak.upgrade() {
            Ok(Self(ui))
        } else {
            anyhow::bail!("ui not running")
        }
    }

    fn cancel_completion_suffix(&self) -> Result<()> {
        self.try_borrow_mut()?.buffer.replace_completion_suffix(None);
        Ok(())
    }

    pub async fn insert_or_set_buffer(&self, insert: bool, data: &[u8], cursor: Option<usize>) -> Result<()> {
        self.queue_draw();

        // if we need to invoke a shfunc, need to trampline out
        let (func, num_chars, old_buffer, old_cursor) = {
            let buffer = &mut self.try_borrow_mut()?.buffer;

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
                            return Ok(())
                        },
                        Ok((func, num_chars)) => {
                            // pita = trampoline
                            (func, num_chars, buffer.get_contents().clone(), buffer.get_cursor())
                        },
                    }

                } else {
                    buffer.insert_at_cursor(insert);
                    buffer.set(None, cursor);
                    return Ok(())
                }

            } else {
                buffer.set(Some(data), cursor);
                return Ok(())
            }
        };

        // invoke the func, then reacquire the buf

        // execute the func
        // a func may run subprocesses so lock the ui
        let lock = self.has_foreground_process.lock().await;
        self.shell.set_zle_buffer(old_buffer, old_cursor as _);
        let _ = self.clone().shell.trampoline_out_callback(move |_ui, token| {
            let num_chars: crate::shell::MetaString = num_chars.to_string().into();
            crate::shell::Function::execute_by_name(token, func.as_ref(), [num_chars].iter());
        }).await;
        let zle = self.shell.get_zle_buffer();
        drop(lock);

        let buffer = &mut self.try_borrow_mut()?.buffer;
        buffer.set(Some(&zle.0), Some(zle.1.unwrap_or(zle.0.len() as _) as usize));
        // finally add the data we wanted originally
        buffer.set(Some(data), cursor);
        Ok(())
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
                self.events.pause();
            }
            self.prepare_for_unhandled_output(true).await?;
            Some(self.has_foreground_process.lock().await)
        } else {
            None
        };

        let result = f.await;

        if let Some(lock) = lock.take() {
            if freeze_events {
                self.events.unpause();
            }
            let recovered = self.recover_from_unhandled_output(None).await;
            drop(lock);
            if crate::log_if_err(recovered) == Some(true) {
                self.queue_draw();
            }
        }

        Ok(result)
    }

    pub async fn allocate_height(&self, height: u16) -> Result<()> {
        let locks = (
            self.has_foreground_process.lock().await,
            self.print_lock.lock_exclusive().await,
        );
        let mut stdout = self.try_borrow()?.stdout.lock();
        crate::tui::allocate_height(&mut stdout, height)?;
        drop(locks);
        Ok(())
    }

    pub fn shell_loop<F: Future>(&self, winch_unblock: bool, future: F) -> Result<F::Output> {
        tokio::pin!(future);

        let mut queueing_enabled = self.shell.queue_signal_level();
        self.shell.dont_queue_signals()?;
        if winch_unblock {
            self.shell.winch_unblock();
        }

        let result = loop {

            let trampoline = self.shell.trampoline_in();
            self.shell.install_sigint_handler()?;
            let result = self.runtime.block_on(async {
                tokio::select!(
                    result = &mut future => ControlFlow::Break(result),
                    result = trampoline => ControlFlow::Continue(result),
                )
            })?;

            match result {
                ControlFlow::Break(x) => break Ok(x),
                ControlFlow::Continue(Err(err)) => break Err(anyhow::anyhow!(err)),
                ControlFlow::Continue(Ok(callback)) => {
                    if winch_unblock {
                        self.shell.winch_block();
                    }
                    self.shell.restore_queue_signals(queueing_enabled);
                    self.shell.trampoline_push();
                    callback.call(self);
                    self.shell.trampoline_pop();
                    queueing_enabled = self.shell.queue_signal_level();
                    if let Err(err) = self.shell.dont_queue_signals() {
                        break Err(anyhow::anyhow!(err));
                    }
                    if winch_unblock {
                        self.shell.winch_unblock();
                    }
                },
            }
        };

        if winch_unblock {
            self.shell.winch_block();
        }
        self.shell.restore_queue_signals(queueing_enabled);
        result
    }

}

impl UiInner {
    pub fn activate(&self) -> Result<()> {
        crossterm::terminal::enable_raw_mode()?;

        // onlcr in case bg processes are outputting things
        let mut attrs = termios::tcgetattr(&self.stdout)?;
        attrs.output_flags.insert(termios::OutputFlags::OPOST | termios::OutputFlags::ONLCR);
        attrs.local_flags.insert(termios::LocalFlags::ISIG);
        attrs.control_chars[termios::SpecialCharacterIndices::VINTR as usize] = self.termios_input_flags.intr;
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

        let mut stdout = self.stdout.lock();
        if self.mouse_mode {
            queue!(stdout, ENABLE_SGR_MOUSE)?;
        }
        if let Some(style) = self.cursor_style {
            queue!(stdout, style)?;
        }
        execute!(
            stdout,
            event::EnableBracketedPaste,
            event::EnableFocusChange,
        )?;

        Ok(())
    }

    pub fn apply_mouse_mode(&self) -> std::io::Result<()> {
        execute!(
            self.stdout.lock(),
            if self.mouse_mode {
                ENABLE_SGR_MOUSE
            } else {
                DISABLE_SGR_MOUSE
            }
        )
    }

    pub fn apply_cursor_style(&self) -> std::io::Result<()> {
        if let Some(style) = self.cursor_style {
            execute!(self.stdout.lock(), style)
        } else {
            Ok(())
        }
    }

    pub fn apply_intr(&self, intr: u8) -> Result<()> {
        let mut attrs = termios::tcgetattr(&self.stdout)?;
        attrs.control_chars[termios::SpecialCharacterIndices::VINTR as usize] = intr;
        nix::sys::termios::tcsetattr(&self.stdout, termios::SetArg::TCSADRAIN, &attrs)?;
        Ok(())
    }

    pub fn deactivate(&mut self) {
        if self.enhanced_keyboard {
            // queue!(self.stdout, event::PopKeyboardEnhancementFlags)?;
        }

        crate::log_if_err(self.apply_intr(crate::keybind::CONTROL_C_BYTE)); // control c

        crate::log_if_err(execute!(
            self.stdout,
            SetCursorStyle::SteadyBlock,
            DISABLE_SGR_MOUSE,
            event::DisableBracketedPaste,
            event::DisableFocusChange,
        ));

        crate::log_if_err(crossterm::terminal::disable_raw_mode());
    }

    fn reset(&mut self) {
        self.buffer.reset();
        self.tui.reset();
        self.status_bar.reset();
        self.dirty = true;
    }

    fn prepare_for_unhandled_output(&mut self, end_sync: bool) -> Result<()> {
        self.deactivate();
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

        queue!(
            self.stdout,
            Clear(ClearType::FromCursorDown),
        )?;
        if end_sync {
            execute!(
                self.stdout,
                EndSynchronizedUpdate,
            )?;
        } else {
            execute!(self.stdout)?;
        }
        Ok(())
    }


    fn draw(&mut self, cursor_y: Option<u32>) -> Result<Vec<usize>> {
        let cmdline = self.cmdline.make_command_line(&mut self.buffer);
        let resized = self.tui.draw(
            &mut self.stdout,
            self.size,
            cursor_y,
            cmdline,
            &mut self.status_bar,
            self.dirty,
        )?;
        self.dirty = false;
        Ok(resized)
    }

    pub fn destroy(&mut self) {
        self.deactivate();
    }

}

impl Drop for UiInner {
    fn drop(&mut self) {
        self.destroy();
    }
}
