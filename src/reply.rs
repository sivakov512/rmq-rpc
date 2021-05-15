use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use tokio::sync::Mutex;

struct SharedState<T: Clone> {
    delivery: Option<T>,
    waker: Option<Waker>,
}

#[derive(Clone)]
struct FutureRpcReply<T: Clone> {
    shared_state: Arc<Mutex<SharedState<T>>>,
}

impl<T: Clone> FutureRpcReply<T> {
    fn new() -> Self {
        let shared_state = Arc::new(Mutex::new(SharedState {
            delivery: None,
            waker: None,
        }));
        Self { shared_state }
    }

    async fn resolve(self, delivery: T) {
        let mut shared_state = self.shared_state.lock().await;
        shared_state.delivery = Some(delivery);

        match shared_state.waker.clone() {
            Some(waker) => waker.wake(),
            None => panic!("Future has never awaited before!"), // TODO: return error
        }
    }
}

impl<T: Clone + std::fmt::Debug> Future for FutureRpcReply<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let mut lock_fut = self.shared_state.lock();
        let lock = unsafe { Pin::new_unchecked(&mut lock_fut) };

        match lock.poll(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(mut v) => {
                v.waker = Some(cx.waker().clone());
                match v.delivery.clone() {
                    Some(v) => Poll::Ready(v),
                    None => Poll::Pending,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolves_to_specified_result() {
        let reply = FutureRpcReply::<String>::new();
        let reply_clone = reply.clone();

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
            reply_clone.resolve("Hello!".to_owned()).await;
        });
        let got = reply.await;

        assert_eq!(got, "Hello!")
    }
}
