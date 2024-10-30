use std::{io::Error, task::{Context, Poll}};
use std::pin::Pin;
use tokio::{io::{AsyncRead, AsyncWrite, ReadBuf}, net::TcpStream};
use tokio_socks::tcp::Socks5Stream;

pub enum ProxyStream {
    Http(TcpStream),
    Socks(Socks5Stream<TcpStream>)
}

impl AsyncRead for ProxyStream {
    fn poll_read(self: Pin<&mut Self>,
                 cx: &mut Context<'_>,
                 buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            ProxyStream::Http(s) => Pin::new(s).poll_read(cx, buf),
            ProxyStream::Socks(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for ProxyStream {
    fn poll_write(self: Pin<&mut Self>,
                  cx: &mut Context<'_>,
                  buf: &[u8]) -> Poll<Result<usize, Error>> {
        match self.get_mut() {
            ProxyStream::Http(s) => Pin::new(s).poll_write(cx, buf),
            ProxyStream::Socks(s) => Pin::new(s).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.get_mut() {
            ProxyStream::Http(s) => Pin::new(s).poll_flush(cx),
            ProxyStream::Socks(s) => Pin::new(s).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        match self.get_mut() {
            ProxyStream::Http(s) => Pin::new(s).poll_shutdown(cx),
            ProxyStream::Socks(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}