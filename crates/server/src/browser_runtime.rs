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
}
