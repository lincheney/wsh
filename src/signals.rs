use anyhow::Result;
use tokio::signal::unix::{signal, SignalKind};
use crate::ui::Ui;

pub fn setup(ui: &Ui) -> Result<()> {
    let signals = [
        SignalKind::hangup(),
        SignalKind::child(),
        SignalKind::window_change(),
        SignalKind::pipe(),
        SignalKind::alarm(),
    ];

    for sig in signals {
        let mut handler = signal(sig)?;
        let ui = ui.clone();
        tokio::task::spawn(async move {
            loop {
                handler.recv().await;
                let mut shell = ui.shell.lock().await;
                shell.call_signal_handler(sig.as_raw_value(), true);
            }
        });
    }

    Ok(())
}
