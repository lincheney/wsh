use crate::lua::LuaWrapper;
use anyhow::{Result};
mod zpty;
mod shell;
mod subshell;
mod spawn;
pub use shell::{shell_run_with_args, ShellRunCmd};
pub use spawn::Stdio;

pub fn init_lua(lua: &LuaWrapper) -> Result<()> {

    shell::init_lua(lua)?;
    subshell::init_lua(lua)?;
    spawn::init_lua(lua)?;
    zpty::init_lua(lua)?;

    Ok(())
}
