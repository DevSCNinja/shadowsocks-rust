//! HTTP Client connector

use std::{
    future::Future,
    io::{self, ErrorKind},
    pin::Pin,
    sync::Arc,
    task::{self, Poll},
};

use futures::{future::BoxFuture, FutureExt};
use hyper::Uri;
use log::error;
use pin_project::pin_project;
use tower::Service;

use crate::local::{context::ServiceContext, loadbalancing::ServerIdent, net::AutoProxyClientStream};

use super::{http_stream::ProxyHttpStream, utils::host_addr};

#[derive(Clone)]
pub struct Connector {
    context: Arc<ServiceContext>,
    server: Option<Arc<ServerIdent>>,
}

impl Connector {
    pub fn new(context: Arc<ServiceContext>, server: Option<Arc<ServerIdent>>) -> Connector {
        Connector { context, server }
    }
}

impl Service<Uri> for Connector {
    type Response = ProxyHttpStream;
    type Error = io::Error;
    type Future = Connecting;

    fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, dst: Uri) -> Self::Future {
        let context = self.context.clone();
        let server = self.server.clone();
        Connecting {
            fut: async move {
                let is_https = dst.scheme_str() == Some("https");
                match host_addr(&dst) {
                    None => {
                        use std::io::Error;
                        error!("HTTP target URI must be a valid address, but found: {}", dst);
                        let err = Error::new(ErrorKind::Other, "URI must be a valid Address");
                        Err(err)
                    }
                    Some(addr) => {
                        let s = match server {
                            Some(ser) => AutoProxyClientStream::connect_proxied(context, ser.as_ref(), addr).await?,
                            None => AutoProxyClientStream::connect_bypassed(context, addr).await?,
                        };

                        if is_https {
                            let host = dst.host().unwrap().trim_start_matches('[').trim_start_matches(']');
                            ProxyHttpStream::connect_https(s, host).await
                        } else {
                            Ok(ProxyHttpStream::connect_http(s))
                        }
                    }
                }
            }
                .boxed(),
        }
    }
}

#[pin_project]
pub struct Connecting {
    #[pin]
    fut: BoxFuture<'static, io::Result<ProxyHttpStream>>,
}

impl Future for Connecting {
    type Output = io::Result<ProxyHttpStream>;

    fn poll(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Self::Output> {
        self.project().fut.poll(cx)
    }
}
