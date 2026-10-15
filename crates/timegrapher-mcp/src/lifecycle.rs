//! Signal active requests when the stdio transport closes or fails.
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::watch;

pub struct Transport<T> {
    inner: T,
    closed: watch::Sender<bool>,
}

impl<T> Transport<T> {
    pub fn new(inner: T, closed: watch::Sender<bool>) -> Self {
        Self { inner, closed }
    }
}

impl<T> Drop for Transport<T> {
    fn drop(&mut self) {
        self.closed.send_replace(true);
    }
}

impl<T: AsyncRead + Unpin> AsyncRead for Transport<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let nonempty = buf.remaining() > 0;
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if matches!(&result, Poll::Ready(Err(_)))
            || (nonempty && matches!(&result, Poll::Ready(Ok(()))) && buf.filled().len() == before)
        {
            self.closed.send_replace(true);
        }
        result
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Transport<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, buf);
        if matches!(&result, Poll::Ready(Err(_))) {
            self.closed.send_replace(true);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let result = Pin::new(&mut self.inner).poll_flush(cx);
        if matches!(&result, Poll::Ready(Err(_))) {
            self.closed.send_replace(true);
        }
        result
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let result = Pin::new(&mut self.inner).poll_shutdown(cx);
        if result.is_ready() {
            self.closed.send_replace(true);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncReadExt;
    #[tokio::test]
    async fn eof_and_drop_signal_disconnect_but_an_empty_read_does_not() {
        let (tx, rx) = watch::channel(false);
        let mut input = Transport::new(tokio::io::empty(), tx.clone());
        assert_eq!(input.read(&mut []).await.unwrap(), 0);
        assert!(!*rx.borrow());
        assert_eq!(input.read(&mut [0; 1]).await.unwrap(), 0);
        assert!(*rx.borrow());
        let (tx, rx) = watch::channel(false);
        drop(Transport::new((), tx));
        assert!(*rx.borrow());
    }
}
