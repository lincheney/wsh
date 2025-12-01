use std::os::raw::{c_int};
use std::sync::{LazyLock};
use tokio::sync::{watch};
use anyhow::Result;
use tokio::signal::unix::{signal, SignalKind};
use crate::ui::Ui;

static CHILD_WATCH: LazyLock<(watch::Sender<()>, watch::Receiver<()>)> = LazyLock::new(|| watch::channel(()));

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
        let is_child = sig == SignalKind::child();

        tokio::task::spawn(async move {
            loop {
                handler.recv().await;
                ui.shell.call_signal_handler(sig.as_raw_value(), true).await;

                if is_child {
                    let _ = CHILD_WATCH.0.send(());
                }
            }
        });
    }

    Ok(())
}

pub async fn wait_for_pid(pid: i32, shell: &crate::shell::ShellClient) -> Option<c_int> {
    let mut receiver = CHILD_WATCH.1.clone();
    loop {
        let status = shell.find_process_status(pid, true).await?;
        if status >= 0 {
            return Some(status)
        }
        receiver.changed().await.unwrap();
    }
}
