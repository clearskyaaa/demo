use std::io::{Error, ErrorKind};
use tokio::net::TcpStream;
use tokio_socks::tcp::Socks5Stream;
use url::Url;
use super::ProxyStream::ProxyStream;

pub enum InnerProxy {
    // http or https
    Http {
        auth: Option<Vec<u8>>,
        url: String,
    },
    // socks5
    Socks {
        auth: Option<(String, String)>,
        url: String,
    }
}

impl InnerProxy {
    pub fn from_proxy_str(proxy_str: &str) -> Result<InnerProxy, Error> {
        use url::Position;

        let url = match Url::parse(proxy_str) {
            Ok(u) => u,
            Err(e) => return Err(Error::new(
                ErrorKind::InvalidInput, "failed to parse proxy url"))
        };
        let addr = &url[Position::BeforeHost..Position::AfterPort];

        match url.scheme() {
            "http" | "https" => {
                let mut basic_bytes: Option<Vec<u8>> = None;
                if let Some(pwd) = url.password() {
                    let encoded_str = format!("Basic {}", base64::encode(&format!("{}:{}", url.username(), pwd)));
                    basic_bytes = Some(encoded_str.into_bytes());
                };

                Ok(InnerProxy::Http {
                    auth: basic_bytes,
                    url: addr.to_string(),
                })
            },
            "socks5" => {
                let mut auth_pair = None;
                if let Some(pwd) = url.password() {
                    auth_pair = Some((url.username().to_string(), pwd.to_string()))
                };

                Ok(InnerProxy::Socks {
                    auth: auth_pair,
                    url: addr.to_string(),
                })
            }

            _ => Err(Error::new(ErrorKind::Unsupported, "unknown schema"))
        }

    }

    pub async fn connect_async(&self, target: &str) -> Result<ProxyStream, Error> {
        let target_url = Url::parse(target)
            .unwrap_or_else(|e| panic!("failed to parse target url: {}", target));
        let host = match target_url.host_str() {
            Some(host) => host.to_string(),
            None => return Err(Error::new(ErrorKind::Unsupported,
                                          "target host not available")),
        };
        let port = target_url.port().unwrap_or(443);
        match self {
            InnerProxy::Http {auth, url } => {
                let mut tcp_stream = TcpStream::connect(url).await
                    .expect("failed to connect http[s] proxy");
                Ok(ProxyStream::Http(Self::tunnel(tcp_stream, host, port, auth).await.unwrap()))
            },
            InnerProxy::Socks { auth, url} => {
                let stream = match auth {
                    Some(au) => Socks5Stream::connect_with_password(
                        url.as_str(), (host.as_str(), port), &au.0, &au.1).await,
                    None => Socks5Stream::connect(url.as_str(), (host.as_str(), port)).await,
                };
                match stream {
                    Ok(s) => Ok(ProxyStream::Socks(s)),
                    Err(e) => Err(Error::new(ErrorKind::NotConnected, "failed to create socks proxy stream"))
                }
            }
        }
    }

    async fn tunnel(mut conn: TcpStream,
                    host: String,
                    port: u16,
                    auth: &Option<Vec<u8>>) -> Result<TcpStream, Error>
    {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut buf = format!(
            "\
         CONNECT {0}:{1} HTTP/1.1\r\n\
         Host: {0}:{1}\r\n\
         ",
            host, port
        ).into_bytes();

        if let Some(au) = auth {
            buf.extend_from_slice(b"Proxy-Authorization: ");
            buf.extend_from_slice(au.as_slice());
            buf.extend_from_slice(b"\r\n");
        }

        buf.extend_from_slice(b"\r\n");
        conn.write_all(&buf).await.unwrap();

        let mut buf = [0; 1024];
        let mut pos = 0;

        loop {
            let n = conn.read(&mut buf[pos..]).await?;
            if n == 0 {
                return Err(Error::new(ErrorKind::UnexpectedEof, "0 bytes in reading tunnel"));
            }
            pos += n;

            let recvd = &buf[..pos];
            if recvd.starts_with(b"HTTP/1.1 200") || recvd.starts_with(b"HTTP/1.0 200") {
                if recvd.ends_with(b"\r\n\r\n") {
                    return Ok(conn);
                }
                if pos == buf.len() {
                    return Err(Error::new(ErrorKind::UnexpectedEof, "proxy headers too long than tunnel"));
                }
            } else if recvd.starts_with(b"HTTP/1.1 407") {
                return Err(Error::new(ErrorKind::PermissionDenied, "proxy authentication required"));
            } else {
                return Err(Error::new(ErrorKind::Other, "unsuccessful tunnel"));
            }
        }
    }
}