use anyhow::Result;
use tokio::io::unix::AsyncFd;
use crate::shell::bin_zle::{FdChange, FdChangeHook};

pub async fn handle_fd_change(ui: &crate::ui::Ui, fd_change: FdChange) -> Result<()> {
    match fd_change {
        FdChange::Added(fd, hook, mut canceller) => {

            match AsyncFd::new(fd) {
                Ok(reader) => {
                    // spawn a task to wait on the fd
                    let ui = ui.clone();
                    tokio::task::spawn(async move {
                        loop {
                            tokio::select!(
                                guard = reader.readable() => {
                                    let result = ui.freeze_if(true, true, FdChangeHook::run_locked(&hook, &ui.shell, fd, guard.err())).await;
                                    if matches!(result, Ok((false, _))) {
                                        break
                                    }
                                },
                                _ = &mut canceller => break, // cancelled
                            );
                        }
                    });
                },
                Err(err) => {
                    ui.freeze_if(true, true, FdChangeHook::run_locked(&hook, &ui.shell, fd, Some(err))).await?.1?;
                },
            }
        },
        FdChange::Removed(_fd) => (),
    }
    Ok(())
}

pub fn init(ui: &crate::ui::Ui) {
    // spawn a task to take care of watched fd
    if let Some(mut fd_source) = super::bin_zle::take_fd_change_source() {
        let ui = ui.clone();
        tokio::task::spawn(async move {
            while let Some(change) = fd_source.recv().await {
                handle_fd_change(&ui, change).await;
            }
        });
    }
}
