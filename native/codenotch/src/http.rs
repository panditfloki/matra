//! Remote HTTPS uses Windows' trusted certificate store (Schannel).
//! Never disable hostname/certificate checks. The localhost Antigravity bridge
//! has a separate, explicitly scoped connector and is not routed here.
pub fn builder() -> ureq::AgentBuilder {
    let tls = native_tls::TlsConnector::new().expect("initialize OS TLS trust");
    ureq::AgentBuilder::new().tls_connector(std::sync::Arc::new(tls))
        .timeout(std::time::Duration::from_secs(15))
}

pub fn agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| builder().build())
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "Live network probe, no credentials; opt in on the target machine"]
    fn os_trust_connects_without_credentials() {
        for url in ["https://chatgpt.com", "https://cursor.com"] {
            match super::agent().get(url).call() {
                Ok(_) | Err(ureq::Error::Status(_, _)) => {},
                Err(e) => panic!("TLS/connectivity probe failed for {url}: {e}"),
            }
        }
    }
}
