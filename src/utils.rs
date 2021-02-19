
use futures::stream::{StreamFuture, FusedStream, StreamExt};
use std::task::{Context, Poll};
use std::pin::Pin;
use futures::stream::{Stream, FuturesUnordered};
pub struct StreamSet<K, S> {
    inner: FuturesUnordered<StreamFuture<StreamSetEntry<K, S>>>,
}

impl<K, S> StreamSet<K, S>
    where S: Stream + Unpin,
          K: Unpin,
{
    pub fn new() -> Self {
        StreamSet { inner: FuturesUnordered::new() }
    }

    pub fn push(&mut self, key: K, stream: S) {
        self.inner.push(StreamSetEntry{ key, stream }.into_future());
    }
}

impl<K, S> Stream for StreamSet<K, S>
    where S: Stream + Unpin,
          K: Clone + Unpin,
{
    type Item = (K, Option<S::Item>);

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Option<Self::Item>>
    {
        let inner = &mut self.get_mut().inner;
        let res = ready!(inner.poll_next_unpin(cx));
        match res {
            None => return Poll::Pending,
            Some((None, stream)) => {
                return Poll::Ready(Some((stream.key, None)));
            }
            Some((Some(item), stream)) => {
                let key = stream.key.clone();
                inner.push(stream.into_future());
                return Poll::Ready(Some((key, Some(item))));
            }
        }
    }
}

impl<K, S> FusedStream for StreamSet<K, S>
    where S: Stream + Unpin,
          K: Clone + Unpin,
{
    fn is_terminated(&self) -> bool {
        return false;
    }
}

struct StreamSetEntry<K, S> {
    key: K,
    stream: S,
}

impl<K, S> Stream for StreamSetEntry<K, S>
    where S: Stream + Unpin,
          K: Unpin,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>)
        -> Poll<Option<Self::Item>>
    {
        self.get_mut().stream.poll_next_unpin(cx)
    }
}