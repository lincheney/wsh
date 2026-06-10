use bstr::{BString, BStr};
use crate::lua::LuaWrapper;
use crate::shell::MetaString;
use std::rc::Rc;
use std::os::fd::{OwnedFd, AsFd, RawFd, AsRawFd, BorrowedFd};
use std::io::{Read, Seek, Write};
use std::fs::OpenOptions;
use anyhow::Result;
use mlua::prelude::*;
use serde::Deserialize;
use nix::sys::memfd::{memfd_create, MFdFlags};
use crate::ui::Ui;
use super::spawn::Stdio;

#[derive(Copy, Clone)]
pub enum Stream<T> {
    Stdin(T),
    Stdout(Stdio),
    Stderr(Stdio),
}

impl<T> Stream<T> {
    fn dup2(&self, fd: &OwnedFd) -> nix::Result<()> {
        match self {
            Stream::Stdin(_) => nix::unistd::dup2_stdin(fd),
            Stream::Stderr(_) => nix::unistd::dup2_stderr(fd),
            Stream::Stdout(_) => nix::unistd::dup2_stdout(fd),
        }
    }

    fn with_fd<R, F: FnOnce(BorrowedFd<'_>) -> R>(&self, func: F) -> R {
        match self {
            Stream::Stdin(_) => func(std::io::stdin().as_fd()),
            Stream::Stdout(_) => func(std::io::stdout().as_fd()),
            Stream::Stderr(_) => func(std::io::stderr().as_fd()),
        }
    }

    pub fn as_raw_fd(&self) -> RawFd {
        self.with_fd(|fd| fd.as_raw_fd())
    }
}

pub enum ShellRunCmd {
    Simple(BString),
    Function {
        func: Rc<crate::shell::Function>,
        args: Vec<BString>,
        arg0: Option<BString>,
    },
}

// manages replacing a single fd (0, 1, or 2) temporarily, restoring it on drop
struct FdOverride {
    stream: Stream<()>,
    saved_fd:  Option<OwnedFd>,
    capture:   Option<OwnedFd>,  // Some(memfd) only for piped output
}

impl FdOverride {

    // set up stdout/stderr: piped captures via memfd, null redirects to /dev/null
    fn new(stream: Stream<Option<&BStr>>) -> Result<Option<Self>> {

        let (fd, capture) = match stream {
            Stream::Stdin(None) | Stream::Stdout(Stdio::inherit) | Stream::Stderr(Stdio::inherit) => {
                return Ok(None)
            },
            Stream::Stdin(Some(content)) => {
                let fd = memfd_create(c"capture", MFdFlags::empty())?;
                let mut file = std::fs::File::from(fd);
                file.write_all(content)?;
                file.rewind()?;
                (file.into(), false)
            },
            Stream::Stdout(Stdio::piped) | Stream::Stderr(Stdio::piped) => {
                (memfd_create(c"capture", MFdFlags::empty())?, true)
            },
            Stream::Stdout(Stdio::null) | Stream::Stderr(Stdio::null) => {
                (OpenOptions::new().write(true).open("/dev/null")?.into(), false)
            },
        };

        let stream = match stream {
            Stream::Stdin(_) => Stream::Stdin(()),
            Stream::Stdout(x) => Stream::Stdout(x),
            Stream::Stderr(x) => Stream::Stderr(x),
        };
        let saved_fd = stream.with_fd(|fd| nix::unistd::dup(fd))?;

        stream.dup2(&fd)?;
        Ok(Some(Self {
            stream,
            saved_fd: Some(saved_fd),
            capture: capture.then_some(fd),
        }))
    }

    fn read(mut self) -> Result<Option<BString>> {
        let Some(fd) = self.capture.take()
            else { return Ok(None) };

        let mut file = std::fs::File::from(fd);
        let mut buf = vec![];
        file.rewind()?;
        file.read_to_end(&mut buf)?;
        Ok(Some(buf.into()))
    }
}

impl Drop for FdOverride {
    fn drop(&mut self) {
        if let Some(fd) = self.saved_fd.take() {
            crate::log_if_err(self.stream.dup2(&fd));
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FullShellRunArgs {
    command:    BString,
    foreground: Option<bool>,
    stdin:      Option<BString>,
    stdout:     Stdio,
    stderr:     Stdio,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ShellRunArgs {
    Simple(BString),
    Full(FullShellRunArgs),
}

async fn shell_run(ui: Ui, lua: Lua, val: LuaValue) -> Result<LuaMultiValue> {
    let args = match lua.from_value(val)? {
        ShellRunArgs::Simple(command) => FullShellRunArgs { command, ..Default::default() },
        ShellRunArgs::Full(args) => args,
    };
    let (code, stdout, stderr) = shell_run_with_args(
        ui,
        lua.clone(),
        ShellRunCmd::Simple(args.command),
        args.foreground,
        args.stdin,
        args.stdout,
        args.stderr,
    ).await?;
    Ok(lua.pack_multi((code, stdout, stderr))?)
}

pub async fn shell_run_with_args(
    ui: Ui,
    _lua: Lua,
    cmd: ShellRunCmd,
    foreground: Option<bool>,
    stdin: Option<BString>,
    stdout: Stdio,
    stderr: Stdio,
) -> Result<(i64, Option<BString>, Option<BString>)> {

    let foreground = foreground.unwrap_or(
        stdin.is_none()
        || matches!(stdout, Stdio::inherit)
        || matches!(stderr, Stdio::inherit)
    );

    let result = ui.clone().shell.trampoline_out_callback(move |ui, token| {
        ui.clone().shell_loop(false, async move {

            let result: Result<_> = ui.freeze_if(foreground, true, async {

                let stdin = FdOverride::new(Stream::Stdin(stdin.as_ref().map(|x| x.as_ref())))?;
                let stdout = FdOverride::new(Stream::Stdout(stdout))?;
                let stderr = FdOverride::new(Stream::Stderr(stderr))?;

                let code = match cmd {
                    ShellRunCmd::Simple(command) => ui.shell.exec(token, command.into()),
                    ShellRunCmd::Function { func, args, arg0, .. } => {
                        let arg0: Option<MetaString> = arg0.map(|x| x.into());
                        let arg0 = arg0.as_ref().map(|x| x.as_ref());
                        let args: Vec<_> = args.into_iter().map(MetaString::from).collect();
                        ui.shell.exec_function(token, func.clone(), arg0, args.iter()).into()
                    },
                };

                drop(stdin);
                let stdout = if let Some(stdout) = stdout {
                    stdout.read()?
                } else {
                    None
                };
                let stderr = if let Some(stderr) = stderr {
                    stderr.read()?
                } else {
                    None
                };

                Ok((code, stdout, stderr))

            }).await?;

            if !crate::is_forked() {
                // sometimes zsh will trash zle without refreshing
                crate::log_if_err(ui.zle_cmd_refresh().await);
            }

            result
        })
    }).await???;

    if foreground {
        ui.queue_draw();
    }

    Ok(result)
}

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {
    lua.set_async_fn("__shell_run", shell_run)?;
    Ok(())
}
