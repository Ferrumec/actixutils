use crate::extractors::ClientIp;
use crate::locals::ProxyConfig;
use actix_web::{
    Error, HttpMessage,
    dev::{Service, ServiceRequest, ServiceResponse, Transform},
};
use futures_util::future::{LocalBoxFuture, Ready, ready};
use std::{net::IpAddr, rc::Rc};

pub struct ClientIpMiddleware {
    config: Rc<ProxyConfig>,
}

impl ClientIpMiddleware {
    pub fn new(config: ProxyConfig) -> Self {
        Self {
            config: Rc::new(config),
        }
    }
}

impl<S, B> Transform<S, ServiceRequest> for ClientIpMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = ClientIpMiddlewareService<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(ClientIpMiddlewareService {
            service: Rc::new(service),
            config: Rc::clone(&self.config),
        }))
    }
}

pub struct ClientIpMiddlewareService<S> {
    service: Rc<S>,
    config: Rc<ProxyConfig>,
}

impl<S, B> Service<ServiceRequest> for ClientIpMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    actix_web::dev::forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let config = Rc::clone(&self.config);
        let service = Rc::clone(&self.service);

        Box::pin(async move {
            if let Some(peer) = req.peer_addr() {
                let peer_ip = peer.ip();

                let client_ip = if config.is_trusted(peer_ip) {
                    forwarded_client_ip(&req, &config).unwrap_or(peer_ip)
                } else {
                    peer_ip
                };

                req.extensions_mut().insert(ClientIp(client_ip));
            }

            service.call(req).await
        })
    }
}

fn forwarded_client_ip(req: &ServiceRequest, config: &ProxyConfig) -> Option<IpAddr> {
    let header = req.headers().get("X-Forwarded-For")?.to_str().ok()?;

    let addresses: Vec<IpAddr> = header
        .split(',')
        .filter_map(|value| value.trim().parse().ok())
        .collect();

    // Walk from the proxy closest to us toward the original client.
    for ip in addresses.iter().rev() {
        if !config.is_trusted(*ip) {
            return Some(*ip);
        }
    }

    addresses.first().copied()
}
