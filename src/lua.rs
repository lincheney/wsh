use anyhow::Result;
use crate::ui::Ui;

mod keybind;
mod string;
mod completion;
mod history;
mod events;
mod tui;
mod log;
mod process;
mod asyncio;
mod parser;
mod variables;
pub use keybind::KeybindMapping;
pub use events::EventCallbacks;

pub async fn init_lua(ui: &Ui) -> Result<()> {

    keybind::init_lua(ui)?;
    string::init_lua(ui).await?;
    completion::init_lua(ui)?;
    history::init_lua(ui).await?;
    events::init_lua(ui)?;
    tui::init_lua(ui)?;
    log::init_lua(ui).await?;
    asyncio::init_lua(ui).await?;
    process::init_lua(ui)?;
    parser::init_lua(ui)?;
    variables::init_lua(ui).await?;

    Ok(())
}
