use anyhow::Result;
use async_std::stream::StreamExt;
use futures::{select, future::FutureExt};

mod fanos;
mod ui;
mod keybind;
mod buffer;

#[async_std::main]
async fn main() -> Result<()> {

    let ui = ui::Ui::new()?;
    ui.activate()?;
    ui.draw()?;
    let mut events = crossterm::event::EventStream::new();

    loop {
        // let mut delay = std::pin::pin!(async_std::task::sleep(std::time::Duration::from_millis(1_000)).fuse());
        let mut events = events.next().fuse();

        select! {
            // _ = delay => { println!(".\r"); },
            event = events => {
                match event {
                    Some(Ok(event)) => {
                        if !ui.handle_event(event).await? {
                            break;
                        }
                    }
                    Some(Err(e)) => println!("Error: {:?}\r", e),
                    None => break,
                }
            }
        };
    }

    Ok(())
}
