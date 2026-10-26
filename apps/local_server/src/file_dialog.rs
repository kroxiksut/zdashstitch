// SPDX-License-Identifier: MIT OR Apache-2.0

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::sync::Notify;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogIntent {
    OpenFile,
    OpenDirectory,
    SaveFile,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DialogErrorCode {
    Unavailable,
    Busy,
    Timeout,
    UserCancelled,
}

impl DialogErrorCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unavailable => "dialog_unavailable",
            Self::Busy => "dialog_busy",
            Self::Timeout => "dialog_timeout",
            Self::UserCancelled => "user_cancelled",
        }
    }
}

pub trait DialogBackend: Send + Sync + 'static {
    fn available(&self, intent: DialogIntent) -> bool;
    fn show(&self, intent: DialogIntent) -> Option<PathBuf>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeDialogBackend;

impl DialogBackend for NativeDialogBackend {
    fn available(&self, _intent: DialogIntent) -> bool {
        native_dialog_environment_available()
    }

    fn show(&self, intent: DialogIntent) -> Option<PathBuf> {
        let dialog = rfd::FileDialog::new();
        match intent {
            DialogIntent::OpenFile => dialog.pick_file(),
            DialogIntent::OpenDirectory => dialog.pick_folder(),
            DialogIntent::SaveFile => dialog.save_file(),
        }
    }
}

#[derive(Clone, Default)]
pub struct DialogCancellation {
    inner: Arc<CancellationInner>,
}

#[derive(Default)]
struct CancellationInner {
    cancelled: AtomicBool,
    changed: Notify,
}

impl DialogCancellation {
    pub fn cancel(&self) {
        self.inner.cancelled.store(true, Ordering::Release);
        self.inner.changed.notify_waiters();
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::Acquire)
    }

    async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.inner.changed.notified().await;
    }
}

pub struct FileDialogService<B = NativeDialogBackend> {
    inner: Arc<ServiceInner<B>>,
}

impl<B> Clone for FileDialogService<B> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

struct ServiceInner<B> {
    backend: Arc<B>,
    busy: Arc<AtomicBool>,
}

impl Default for FileDialogService<NativeDialogBackend> {
    fn default() -> Self {
        Self::new(NativeDialogBackend)
    }
}

impl<B: DialogBackend> FileDialogService<B> {
    #[must_use]
    pub fn new(backend: B) -> Self {
        Self {
            inner: Arc::new(ServiceInner {
                backend: Arc::new(backend),
                busy: Arc::new(AtomicBool::new(false)),
            }),
        }
    }

    pub async fn choose(
        &self,
        intent: DialogIntent,
        cancellation: DialogCancellation,
        timeout: Duration,
    ) -> Result<PathBuf, DialogErrorCode> {
        if cancellation.is_cancelled() {
            return Err(DialogErrorCode::UserCancelled);
        }
        if !self.inner.backend.available(intent) {
            return Err(DialogErrorCode::Unavailable);
        }
        if self
            .inner
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(DialogErrorCode::Busy);
        }

        let backend = self.inner.backend.clone();
        let busy = self.inner.busy.clone();
        let mut worker = tokio::task::spawn_blocking(move || {
            let _guard = BusyGuard(busy);
            backend.show(intent)
        });

        tokio::select! {
            result = &mut worker => match result {
                Ok(Some(path)) => Ok(path),
                Ok(None) => Err(DialogErrorCode::UserCancelled),
                Err(_) => Err(DialogErrorCode::Unavailable),
            },
            () = cancellation.cancelled() => Err(DialogErrorCode::UserCancelled),
            () = tokio::time::sleep(timeout) => Err(DialogErrorCode::Timeout),
        }
    }
}

struct BusyGuard(Arc<AtomicBool>);

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[cfg(windows)]
fn native_dialog_environment_available() -> bool {
    true
}

#[cfg(target_os = "linux")]
fn native_dialog_environment_available() -> bool {
    let session_bus = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
        || std::env::var_os("XDG_RUNTIME_DIR").is_some();
    let graphical_session =
        std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
    session_bus && graphical_session
}

#[cfg(not(any(windows, target_os = "linux")))]
fn native_dialog_environment_available() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
            mpsc,
        },
        time::Duration,
    };

    use super::*;

    struct UnavailableBackend {
        calls: Arc<AtomicUsize>,
    }

    impl DialogBackend for UnavailableBackend {
        fn available(&self, _intent: DialogIntent) -> bool {
            false
        }

        fn show(&self, _intent: DialogIntent) -> Option<PathBuf> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Some(PathBuf::from("must-not-open"))
        }
    }

    struct BlockingBackend {
        entered: mpsc::Sender<()>,
        release: Arc<std::sync::Mutex<mpsc::Receiver<()>>>,
        calls: Arc<AtomicUsize>,
    }

    impl DialogBackend for BlockingBackend {
        fn available(&self, _intent: DialogIntent) -> bool {
            true
        }

        fn show(&self, _intent: DialogIntent) -> Option<PathBuf> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let _ = self.entered.send(());
            let receiver = self
                .release
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = receiver.recv();
            Some(PathBuf::from("selected.yaml"))
        }
    }

    #[tokio::test]
    async fn unavailable_is_detected_before_backend_invocation() {
        let calls = Arc::new(AtomicUsize::new(0));
        let service = FileDialogService::new(UnavailableBackend {
            calls: calls.clone(),
        });
        let result = service
            .choose(
                DialogIntent::OpenFile,
                DialogCancellation::default(),
                Duration::from_secs(1),
            )
            .await;
        assert_eq!(result, Err(DialogErrorCode::Unavailable));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn only_one_dialog_runs_and_cancelled_late_result_is_ignored() {
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let service = FileDialogService::new(BlockingBackend {
            entered: entered_tx,
            release: Arc::new(std::sync::Mutex::new(release_rx)),
            calls: calls.clone(),
        });
        let cancellation = DialogCancellation::default();
        let first_service = service.clone();
        let first_cancellation = cancellation.clone();
        let first = tokio::spawn(async move {
            first_service
                .choose(
                    DialogIntent::OpenFile,
                    first_cancellation,
                    Duration::from_secs(5),
                )
                .await
        });
        entered_rx
            .recv_timeout(Duration::from_secs(1))
            .unwrap_or_else(|error| panic!("dialog did not start: {error}"));

        let second = service
            .choose(
                DialogIntent::OpenFile,
                DialogCancellation::default(),
                Duration::from_secs(1),
            )
            .await;
        assert_eq!(second, Err(DialogErrorCode::Busy));
        cancellation.cancel();
        assert_eq!(
            first.await.unwrap_or_else(|error| panic!("join: {error}")),
            Err(DialogErrorCode::UserCancelled)
        );
        assert_eq!(
            service
                .choose(
                    DialogIntent::OpenFile,
                    DialogCancellation::default(),
                    Duration::from_millis(10),
                )
                .await,
            Err(DialogErrorCode::Busy)
        );
        release_tx
            .send(())
            .unwrap_or_else(|error| panic!("release dialog: {error}"));
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn timeout_returns_without_accepting_the_late_selection() {
        let (entered_tx, _entered_rx) = mpsc::channel();
        let (_release_tx, release_rx) = mpsc::channel();
        let service = FileDialogService::new(BlockingBackend {
            entered: entered_tx,
            release: Arc::new(std::sync::Mutex::new(release_rx)),
            calls: Arc::new(AtomicUsize::new(0)),
        });
        let result = service
            .choose(
                DialogIntent::OpenDirectory,
                DialogCancellation::default(),
                Duration::from_millis(10),
            )
            .await;
        assert_eq!(result, Err(DialogErrorCode::Timeout));
    }

    #[test]
    fn wire_codes_are_stable() {
        assert_eq!(DialogErrorCode::Unavailable.as_str(), "dialog_unavailable");
        assert_eq!(DialogErrorCode::Busy.as_str(), "dialog_busy");
        assert_eq!(DialogErrorCode::Timeout.as_str(), "dialog_timeout");
        assert_eq!(DialogErrorCode::UserCancelled.as_str(), "user_cancelled");
    }
}
