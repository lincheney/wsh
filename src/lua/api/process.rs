use anyhow::{Result};
use crate::ui::{Ui};
mod zpty;
mod shell;
mod subshell;
mod spawn;
pub use shell::{shell_run_with_args, ShellRunCmd};

pub fn init_lua(ui: &Ui) -> Result<()> {

    shell::init_lua(ui)?;
    subshell::init_lua(ui)?;
    spawn::init_lua(ui)?;
    zpty::init_lua(ui)?;

    Ok(())
}
