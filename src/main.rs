use anyhow::Result;
use async_std::stream::StreamExt;
use futures::{select, future::FutureExt};
mod fanos;
mod ui;

#[async_std::main]
async fn main() -> Result<()> {

    let mut client = fanos::FanosClient::new().await?;

    let ui = ui::Ui::new()?;
    let mut reader = crossterm::event::EventStream::new();

    loop {
        let mut delay = std::pin::pin!(async_std::task::sleep(std::time::Duration::from_millis(1_000)).fuse());
        let mut event = reader.next().fuse();

        select! {
            _ = delay => { println!(".\r"); },
            maybe_event = event => {
                match maybe_event {
                    Some(Ok(event)) => {
                        println!("Event::{:?}\r", event);

                        if event == crossterm::event::Event::Key(crossterm::event::KeyCode::Char('c').into()) {
                            println!("Cursor position: {:?}\r", crossterm::cursor::position());
                        }

                        if event == crossterm::event::Event::Key(crossterm::event::KeyCode::Esc.into()) {
                            break;
                        }
                    }
                    Some(Err(e)) => println!("Error: {:?}\r", e),
                    None => break,
                }
            }
        };
    }

    client.send(b"EVAL echo $PWD", None).await?;
    client.recv().await?;

    Ok(())
}
