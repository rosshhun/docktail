//! TLS — TlsStreamWrapper that implements tonic's Connected trait.

use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::net::TcpStream;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

/// Wrapper for TlsStream that implements tonic's Connected trait.
pub struct TlsStreamWrapper(pub tokio_rustls::server::TlsStream<TcpStream>);

impl tonic::transport::server::Connected for TlsStreamWrapper {
    type ConnectInfo = TlsConnectInfo;

    fn connect_info(&self) -> Self::ConnectInfo {
        TlsConnectInfo {
            peer_addr: self.0.get_ref().0.peer_addr().ok(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TlsConnectInfo {
    pub peer_addr: Option<SocketAddr>,
}

impl AsyncRead for TlsStreamWrapper {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buf)
    }
}

impl AsyncWrite for TlsStreamWrapper {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.0).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}