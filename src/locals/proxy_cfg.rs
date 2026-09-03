use ipnet::IpNet;
use std::net::IpAddr;

#[derive(Debug, Clone)]
pub struct ProxyConfig {
    trusted_proxies: Vec<IpNet>,
}

impl ProxyConfig {
    pub fn new(trusted_proxies: Vec<IpNet>) -> Self {
        Self { trusted_proxies }
    }

    pub fn is_trusted(&self, ip: IpAddr) -> bool {
        self.trusted_proxies
            .iter()
            .any(|network| network.contains(&ip))
    }
}
