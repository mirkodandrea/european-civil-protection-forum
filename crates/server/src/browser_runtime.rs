//! Browser executor for the shared exercise workflows. No sockets or OS threads.
use std::future::Future;

pub struct JoinHandle<T>(futures::channel::oneshot::Receiver<T>);
impl<T> Future for JoinHandle<T> {
    type Output = Result<T, &'static str>;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        std::pin::Pin::new(&mut self.0)
            .poll(cx)
            .map(|r| r.map_err(|_| "Browser task cancelled"))
    }
}
pub fn spawn<F: Future + 'static>(future: F) -> JoinHandle<F::Output>
where
    F::Output: 'static,
{
    let (tx, rx) = futures::channel::oneshot::channel();
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tx.send(future.await);
    });
    JoinHandle(rx)
}
pub mod task {
    pub async fn yield_now() {
        gloo_timers::future::TimeoutFuture::new(0).await;
    }
    pub fn spawn_blocking<F, T>(
        work: F,
    ) -> impl std::future::Future<Output = Result<T, &'static str>>
    where
        F: FnOnce() -> T + 'static,
        T: 'static,
    {
        super::spawn(async move {
            yield_now().await;
            work()
        })
    }
}
pub mod time {
    use std::{future::Future, time::Duration};
    pub async fn sleep(duration: Duration) {
        gloo_timers::future::TimeoutFuture::new(duration.as_millis().min(u32::MAX as u128) as u32)
            .await;
    }
    pub async fn timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, ()> {
        use futures::{
            future::{select, Either},
            pin_mut,
        };
        let timer = sleep(duration);
        pin_mut!(future, timer);
        match select(future, timer).await {
            Either::Left((value, _)) => Ok(value),
            Either::Right(_) => Err(()),
        }
    }
}
pub mod sync {
    pub use futures::lock::Mutex;
    use std::{cell::Cell, sync::Arc};
    pub struct Semaphore(Cell<usize>);
    pub struct Permit(Arc<Semaphore>);
    impl Semaphore {
        pub fn new(permits: usize) -> Self {
            Self(Cell::new(permits))
        }
        pub fn try_acquire_owned(self: Arc<Self>) -> Result<Permit, ()> {
            if self.0.get() == 0 {
                return Err(());
            }
            self.0.set(self.0.get() - 1);
            Ok(Permit(self))
        }
    }
    impl Drop for Permit {
        fn drop(&mut self) {
            self.0 .0.set(self.0 .0.get() + 1);
        }
    }
}
