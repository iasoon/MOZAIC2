use std::sync::{Arc, Mutex};
use std::pin::Pin;
use futures::task::{Context, Poll};
use futures::FutureExt;
use futures::{Future, Stream};
use futures::task::AtomicWaker;
use std::sync::atomic::{AtomicUsize, Ordering};

// TODO: restructure this code for a bit

pub fn msg_stream<T>() -> MsgStreamHandle<T> {
    let stream = Arc::new(MsgStream {
        msg_count: AtomicUsize::new(0),
        state_mutex: Mutex::new(InnerState {
            messages: Vec::new(),
            wakers: Vec::new(),
            reader_counter: 0,
        }),
    });
    let handle = MsgStreamHandle { stream: stream.clone() };
    return handle;
}

pub struct InnerState<T> {
    messages: Vec<Arc<T>>,
    wakers: Vec<(usize, Arc<AtomicWaker>)>,
    reader_counter: usize,
}


struct MsgStream<T> {
    state_mutex: Mutex<InnerState<T>>,
    msg_count: AtomicUsize,
}

pub struct MsgStreamHandle<T> {
    stream: Arc<MsgStream<T>>,
}

impl<T> Clone for MsgStreamHandle<T> {
    fn clone(&self) -> Self {
        MsgStreamHandle {
            stream: self.stream.clone(),
        }
    }
}

impl<T> MsgStreamHandle<T> {
    pub fn reader(&self) -> MsgStreamReader<T> {
        let mut inner = self.stream.state_mutex.lock().unwrap();

        let reader_id = inner.reader_counter;
        inner.reader_counter += 1;

        let waker = Arc::new(AtomicWaker::new());
        inner.wakers.push((reader_id, waker.clone()));
    
        MsgStreamReader {
            stream_handle: self.clone(),
            reader_id,
            waker,
            // TODO: maybe set this to head?
            pos: 0,
        }
    }

    pub fn to_vec(&self) -> Vec<Arc<T>> {
        let state = self.stream.state_mutex.lock().unwrap();
        return state.messages.clone();
    }

    pub fn write(&mut self, msg: T) {
        let mut inner = self.stream.state_mutex.lock().unwrap();
        inner.messages.push(Arc::new(msg));
        self.stream.msg_count.store(inner.messages.len(), Ordering::Relaxed);
        for (_id, waker) in inner.wakers.iter() {
            waker.wake();
        }
    }
}

pub struct MsgStreamReader<T> {
    stream_handle: MsgStreamHandle<T>,
    waker: Arc<AtomicWaker>,
    reader_id: usize,
    pos: usize,
}

impl<T> MsgStreamReader<T> {
    pub fn recv<'a>(&'a mut self) -> Recv<'a, T> {
        Recv {
            reader: self,
        }
    }

    pub fn reset_pos(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn clone(&self) -> Self {
        let mut r = self.stream_handle.reader();
        r.pos = self.pos;
        return r;
    }
}

impl<T> Drop for MsgStreamReader<T> {
    
    fn drop(&mut self) {
        let mut inner = self.stream_handle.stream.state_mutex.lock().unwrap();
        inner.wakers.retain(|(id, _)| id != &self.reader_id);
    }
}

impl<T> Stream for MsgStreamReader<T> {
    type Item = Arc<T>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>)
        -> Poll<Option<Self::Item>>
    {
        self.recv().poll_unpin(cx).map(|item| Some(item))
    }
}

pub struct Recv<'s, T> {
    reader: &'s mut MsgStreamReader<T>,
}

impl<'s, T> Future for Recv<'s, T> {
    type Output = Arc<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>
    {
        let Recv { reader } = self.get_mut();

        let msg_count = reader.stream_handle.stream.msg_count
            .load(Ordering::Relaxed);
    
        if msg_count > reader.pos {
            let inner = reader.stream_handle
                .stream.state_mutex.lock().unwrap();
            let value = inner.messages[reader.pos].clone();
            reader.pos += 1;
            Poll::Ready(value)
        } else {
            reader.waker.register(cx.waker());
            Poll::Pending
        }
    }
}
