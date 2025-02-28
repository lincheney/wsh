use anyhow::Result;
use crate::ui::Ui;
use crate::shell::Shell;

mod keybind;
mod string;
mod completion;
mod history;
mod events;
mod tui;
mod log;
mod process;
mod asyncio;
pub use keybind::KeybindMapping;
pub use events::EventCallbacks;

pub async fn init_lua(ui: &Ui, shell: &Shell) -> Result<()> {

    keybind::init_lua(ui, shell).await?;
    string::init_lua(ui, shell).await?;
    completion::init_lua(ui, shell).await?;
    history::init_lua(ui, shell).await?;
    events::init_lua(ui, shell).await?;
    tui::init_lua(ui, shell).await?;
    log::init_lua(ui, shell).await?;
    asyncio::init_lua(ui, shell).await?;
    process::init_lua(ui, shell).await?;

    Ok(())
}
