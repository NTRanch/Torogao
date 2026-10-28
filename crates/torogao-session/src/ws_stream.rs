//! Byte adapters between the WebSocket connection thread
//! (synchronous tungstenite, as in Phase C) and the async
//! guard/SDK pipeline (which speaks bytes over tokio traits).
//!
//! rmcp 3.8.0 ships no WebSocket transport, so the WS leg
//! adapts at the byte seam the fleet guard already defines:
//! inbound WS text messages become byte lines for the guard;
//! outbound lines become WS text messages, one line per
//! message — Phase C's framing semantics on the same wire
//! (one request line, one response text frame).

use std::io;
use std::pin::Pin;
use std::sync::mpsc as std_mpsc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::sync::mpsc as tokio_mpsc;

/// The read half: bytes arriving from the WS thread over a
/// channel. EOF when the channel closes (the client closed
/// the socket or the connection thread ended).
pub struct ChannelRead {
    rx: tokio_mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
    offset: usize,
}

impl ChannelRead {
    /// A read half fed by `rx`.
    pub fn new(rx: tokio_mpsc::Receiver<Vec<u8>>) -> Self {
        ChannelRead {
            rx,
            pending: Vec::new(),
            offset: 0,
        }
    }
}

impl AsyncRead for ChannelRead {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        loop {
            if self.offset < self.pending.len() {
                let n = (self.pending.len() - self.offset).min(buf.remaining());
                buf.put_slice(&self.pending[self.offset..self.offset + n]);
                self.offset += n;
                if self.offset == self.pending.len() {
                    self.pending.clear();
                    self.offset = 0;
                }
                return Poll::Ready(Ok(()));
            }
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(bytes)) => {
                    self.pending = bytes;
                    self.offset = 0;
                }
                Poll::Ready(None) => return Poll::Ready(Ok(())),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The write half: buffers written bytes and forwards each
/// completed line to the WS thread as one message payload
/// (without the newline). Writing after the connection
/// thread has gone reports `BrokenPipe`, which ends the
/// pipeline through the guard's normal error path.
pub struct LineWrite {
    tx: std_mpsc::Sender<Vec<u8>>,
    pending: Vec<u8>,
}

impl LineWrite {
    /// A write half sending lines to `tx`.
    pub fn new(tx: std_mpsc::Sender<Vec<u8>>) -> Self {
        LineWrite {
            tx,
            pending: Vec::new(),
        }
    }

    fn emit_lines(&mut self) -> io::Result<()> {
        while let Some(pos) = self.pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=pos).collect();
            let payload = &line[..line.len() - 1];
            self.tx.send(payload.to_vec()).map_err(|_| {
                io::Error::new(io::ErrorKind::BrokenPipe, "websocket connection closed")
            })?;
        }
        Ok(())
    }
}

impl AsyncWrite for LineWrite {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.pending.extend_from_slice(buf);
        match self.emit_lines() {
            Ok(()) => Poll::Ready(Ok(buf.len())),
            Err(err) => Poll::Ready(Err(err)),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Complete lines are emitted on write; a partial line
        // stays buffered by design (a line ends at its newline).
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            let _ = self.tx.send(rest);
        }
        Poll::Ready(Ok(()))
    }
}
