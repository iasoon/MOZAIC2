use std::sync::{Arc, Mutex};
use std::pin::Pin;
use futures::task::{Context, Poll};
use futures::FutureExt;
use futures::{Future, Stream};
use futures::task::AtomicWaker;

pub fn msg_stream() -> MsgStreamHandle {
    let stream = Arc::new(Mutex::new(MsgStream::new()));
    let handle = MsgStreamHandle { stream: stream.clone() };
    return handle;
}

pub struct MsgStream {
    messages: Vec<Arc<Vec<u8>>>,
    wakers: Vec<AtomicWaker>,
}

impl MsgStream {
    fn new() -> Self {
        MsgStream {
            messages: Vec::new(),
            wakers: Vec::new(),
        }
    }

    fn append(&mut self, msg: Vec<u8>) {
        self.messages.push(Arc::new(msg));
        for waker in self.wakers.drain(..) {
            waker.wake();
        }
    }
}

#[derive(Clone)]
pub struct MsgStreamHandle {
    stream: Arc<Mutex<MsgStream>>,
}

impl MsgStreamHandle {
    pub fn reader(&self, pos: usize) -> MsgStreamReader {
        MsgStreamReader {
            stream: self.stream.clone(),
            pos,
        }
    }

    pub fn write(&mut self, msg: Vec<u8>) {
        let mut inner = self.stream.lock().unwrap();
        inner.append(msg);
    }
}

pub struct MsgStreamReader {
    stream: Arc<Mutex<MsgStream>>,
    pos: usize,
}

impl MsgStreamReader {
    pub fn recv<'a>(&'a mut self) -> Recv<'a> {
        Recv {
            reader: self,
        }
    }
}

impl Stream for MsgStreamReader {
    type Item = Arc<Vec<u8>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>)
        -> Poll<Option<Self::Item>>
    {
        self.recv().poll_unpin(cx).map(|item| Some(item))
    }
}

pub struct Recv<'s> {
    reader: &'s mut MsgStreamReader,
}

impl<'s> Future for Recv<'s> {
    type Output = Arc<Vec<u8>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output>
    {
        let Recv { reader } = self.get_mut();
    
        let mut inner = reader.stream.lock().unwrap();
        if inner.messages.len() > reader.pos {
            let value = inner.messages[reader.pos].clone();
            reader.pos += 1;
            Poll::Ready(value)
        } else {
            let waker = AtomicWaker::new();
            waker.register(cx.waker());
            inner.wakers.push(waker);
            Poll::Pending
        }
    }
}
