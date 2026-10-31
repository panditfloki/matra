//! One bounded HTTP call and the address rules around it.
//!
//! Every request refuses redirects, so a credential cannot follow one to another host, and caps both
//! time and body size. Failure kinds carry no text from the transport or the server: those strings can
//! name private addresses, and they are never shown or logged.
use std::io::Read;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;
use url::{Host, Url};

pub const LOCAL_TIMEOUT: Duration = Duration::from_secs(3);
pub const ENDPOINT_TIMEOUT: Duration = Duration::from_secs(6);
pub const CLOUD_TIMEOUT: Duration = Duration::from_secs(15);
pub const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    Timeout,
    Unreachable,
    TooLarge,
}

#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub retry_after: Option<u64>,
    pub cookies: Vec<String>,
}

pub type Headers = Vec<(String, String)>;

pub fn header(name: &str, value: impl Into<String>) -> (String, String) {
    (name.to_string(), value.into())
}

pub fn bearer(token: &str) -> (String, String) {
    header("Authorization", format!("Bearer {token}"))
}

pub fn send(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    body: Option<&str>,
    timeout: Duration,
) -> Result<Reply, NetError> {
    let agent = crate::http::builder()
        .redirects(0)
        .timeout(timeout)
        .build();
    let mut request = agent
        .request(method, url)
        .set("User-Agent", "Matra")
        .set("Accept", "application/json");
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let result = match body {
        Some(text) => request
            .set("Content-Type", "application/json")
            .send_string(text),
        None => request.call(),
    };
    let response = match result {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(t)) => {
            let text = t.to_string().to_lowercase();
            return Err(if text.contains("timed out") || text.contains("timeout") {
                NetError::Timeout
            } else {
                NetError::Unreachable
            });
        }
    };
    let status = response.status();
    let retry_after = response.header("Retry-After").and_then(parse_retry_after);
    let cookies = response
        .all("set-cookie")
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| NetError::Unreachable)?;
    if bytes.len() > MAX_BODY {
        return Err(NetError::TooLarge);
    }
    Ok(Reply {
        status,
        body: bytes,
        retry_after,
        cookies,
    })
}

pub fn get(url: &str, headers: &[(String, String)], timeout: Duration) -> Result<Reply, NetError> {
    send("GET", url, headers, None, timeout)
}

/// `Retry-After` is a number of seconds or an HTTP date.
pub fn parse_retry_after(text: &str) -> Option<u64> {
    let text = text.trim();
    if let Ok(secs) = text.parse::<u64>() {
        return Some(secs);
    }
    let date = chrono::DateTime::parse_from_rfc2822(text).ok()?;
    let delta = date.timestamp() - chrono::Utc::now().timestamp();
    Some(delta.max(0) as u64)
}

/// A minute, doubling per consecutive limit, capped at 15 minutes so it recovers on its own. The
/// server's own hint only raises the wait, even past the cap.
pub fn backoff_secs(attempt: u32, retry_after: Option<u64>) -> u64 {
    let doubled = 60u64.saturating_mul(1u64 << attempt.min(4)).min(900);
    doubled.max(retry_after.unwrap_or(0))
}

fn clean_url(address: &str) -> Result<Url, ()> {
    let url = Url::parse(address.trim()).map_err(|_| ())?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return Err(());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(());
    }
    Ok(url)
}

/// `http://127.0.0.1:port`, with no path. A local monitor must not reach another machine because of a
/// typo, and `localhost` is resolved here so neither DNS nor a proxy is involved.
pub fn parse_loopback(address: &str) -> Result<String, String> {
    let refuse = || "Use an HTTP address on this PC, such as http://127.0.0.1:11434.".to_string();
    let url = clean_url(address).map_err(|_| refuse())?;
    if url.scheme() != "http" || !matches!(url.path(), "" | "/") || !is_loopback(&url) {
        return Err(refuse());
    }
    let host = match url.host() {
        Some(Host::Domain(_)) => "127.0.0.1".to_string(),
        Some(other) => other.to_string(),
        None => return Err(refuse()),
    };
    Ok(match url.port() {
        Some(port) => format!("http://{host}:{port}"),
        None => format!("http://{host}"),
    })
}

/// A server elsewhere on the user's network, for a runtime that cannot run on this PC.
pub fn parse_network(address: &str) -> Result<String, String> {
    let refuse =
        || "Use an http:// or https:// address such as http://192.168.1.20:8000, with no path.".to_string();
    let url = clean_url(address).map_err(|_| refuse())?;
    if !matches!(url.path(), "" | "/") {
        return Err(refuse());
    }
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// Base URLs of custom endpoints may carry a path, but never userinfo, a query or a fragment.
pub fn parse_base(address: &str) -> Result<Url, ()> {
    clean_url(address)
}

pub fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

fn private_v4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        // 100.64.0.0/10, the shared range VPN overlays such as Tailscale use.
        || (a == 100 && (64..128).contains(&b))
}

fn private_v6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    ip.is_loopback() || (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
}

/// May a credential travel to this address? Over TLS, always. Over plain HTTP only inside the user's
/// own network, because on the public internet it would be readable in transit.
pub fn key_allowed(address: &str) -> bool {
    let Ok(url) = Url::parse(address.trim()) else {
        return false;
    };
    match url.scheme() {
        "https" => true,
        "http" => match url.host() {
            Some(Host::Ipv4(ip)) => private_v4(ip),
            Some(Host::Ipv6(ip)) => private_v6(ip),
            Some(Host::Domain(d)) => {
                let d = d.to_ascii_lowercase();
                d == "localhost" || d.ends_with(".local") || !d.contains('.')
            }
            None => false,
        },
        _ => false,
    }
}

/// Scheme, host and effective port. A stored key follows its address and nothing else.
pub fn origin(address: &str) -> Option<(String, String, u16)> {
    let url = Url::parse(address.trim()).ok()?;
    Some((
        url.scheme().to_string(),
        url.host_str()?.to_ascii_lowercase(),
        url.port_or_known_default()?,
    ))
}

/// A header name a user may choose for a custom endpoint's key. Names that would change how the
/// request is framed or routed are refused.
pub fn header_name_ok(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    !name.is_empty()
        && name.len() <= 64
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        && !matches!(
            lower.as_str(),
            "host" | "content-length" | "transfer-encoding" | "connection" | "cookie" | "user-agent" | "accept"
        )
}

/// A secret that could not safely become a header value is treated as not being there.
pub fn clean_secret(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty() && text.len() <= 8192 && !text.chars().any(char::is_control))
        .then(|| text.to_string())
}

#[cfg(test)]
pub mod testkit {
    //! A scripted HTTP server on an ephemeral loopback port. No test in this module tree leaves the
    //! machine, and none reads a real profile.
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    pub struct Server {
        pub base: String,
        pub requests: Arc<Mutex<Vec<String>>>,
    }

    impl Server {
        pub fn saw(&self, text: &str) -> bool {
            self.requests.lock().unwrap().iter().any(|r| r.contains(text))
        }
        pub fn count(&self) -> usize {
            self.requests.lock().unwrap().len()
        }
    }

    pub fn response(status: &str, extra: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    pub fn ok(body: &str) -> String {
        response("200 OK", "", body)
    }

    /// `routes`: (substring of the request line, raw response). Unmatched requests answer 404. Runs
    /// until the test process ends.
    pub fn serve(routes: Vec<(&str, String)>) -> Server {
        let routes: Vec<(String, String)> = routes.into_iter().map(|(a, b)| (a.to_string(), b)).collect();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(3)));
                let mut seen = Vec::new();
                let mut buffer = [0u8; 8192];
                loop {
                    let Ok(n) = stream.read(&mut buffer) else { break };
                    if n == 0 {
                        break;
                    }
                    seen.extend_from_slice(&buffer[..n]);
                    let text = String::from_utf8_lossy(&seen);
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text[..end]
                            .lines()
                            .find_map(|l| {
                                let (name, value) = l.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())?
                            })
                            .unwrap_or(0);
                        if seen.len() >= end + 4 + length {
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&seen).to_string();
                let line = text.lines().next().unwrap_or_default().to_string();
                log.lock().unwrap().push(text);
                let reply = routes
                    .iter()
                    .find(|(path, _)| line.contains(path.as_str()))
                    .map(|(_, reply)| reply.clone())
                    .unwrap_or_else(|| response("404 Not Found", "", ""));
                let _ = stream.write_all(reply.as_bytes());
            }
        });
        Server { base, requests }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_rules_match_the_mac() {
        assert_eq!(parse_loopback("http://localhost:11434").unwrap(), "http://127.0.0.1:11434");
        assert_eq!(parse_loopback(" http://127.0.0.1:1234/ ").unwrap(), "http://127.0.0.1:1234");
        assert_eq!(parse_loopback("http://[::1]:8080").unwrap(), "http://[::1]:8080");
        for bad in [
            "https://127.0.0.1:1234",
            "http://192.168.1.2:1234",
            "http://user:pw@127.0.0.1:1234",
            "http://127.0.0.1:1234/api",
            "http://127.0.0.1:1234?x=1",
            "file:///etc/passwd",
        ] {
            assert!(parse_loopback(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn network_rule_allows_a_mac_elsewhere_without_userinfo() {
        assert_eq!(parse_network("http://192.168.1.20:8000/").unwrap(), "http://192.168.1.20:8000");
        assert_eq!(parse_network("https://mac.example.com:8443").unwrap(), "https://mac.example.com:8443");
        assert!(parse_network("http://u:p@192.168.1.20:8000").is_err());
        assert!(parse_network("ftp://192.168.1.20").is_err());
        assert!(parse_network("http://192.168.1.20:8000/v1").is_err());
    }

    #[test]
    fn keys_stay_off_public_plain_http() {
        for ok in [
            "https://api.example.com",
            "http://127.0.0.1:1",
            "http://localhost:1",
            "http://192.168.1.2:1",
            "http://10.0.0.2:1",
            "http://172.20.0.2:1",
            "http://100.100.1.1:1",
            "http://mac-studio.local:1",
            "http://mac-studio:1",
            "http://[fd00::1]:1",
        ] {
            assert!(key_allowed(ok), "{ok}");
        }
        for bad in ["http://8.8.8.8", "http://api.example.com", "http://172.32.0.2", "http://[2001:db8::1]", "ftp://10.0.0.1"] {
            assert!(!key_allowed(bad), "{bad}");
        }
    }

    #[test]
    fn origin_uses_the_effective_port() {
        assert_eq!(origin("https://a.example/v1"), origin("https://A.example:443/other"));
        assert_ne!(origin("http://a.example"), origin("https://a.example"));
    }

    #[test]
    fn backoff_doubles_caps_and_lets_the_server_raise_it() {
        assert_eq!(backoff_secs(0, None), 60);
        assert_eq!(backoff_secs(1, None), 120);
        assert_eq!(backoff_secs(9, None), 900);
        assert_eq!(backoff_secs(0, Some(30)), 60);
        assert_eq!(backoff_secs(0, Some(3600)), 3600);
        assert_eq!(parse_retry_after("120"), Some(120));
        assert_eq!(parse_retry_after("Sun, 06 Nov 1994 08:49:37 GMT"), Some(0));
        assert_eq!(parse_retry_after("soon"), None);
    }

    #[test]
    fn header_and_secret_shapes_are_checked() {
        assert!(header_name_ok("Authorization"));
        assert!(header_name_ok("x-api-key"));
        assert!(!header_name_ok("Host"));
        assert!(!header_name_ok("x bad"));
        assert!(!header_name_ok("x\r\ninjected"));
        assert!(clean_secret("tok\nen").is_none());
        assert!(clean_secret("   ").is_none());
        assert_eq!(clean_secret(" abc ").as_deref(), Some("abc"));
    }

    #[test]
    fn redirects_are_refused_and_bodies_are_bounded() {
        let big = "x".repeat(MAX_BODY + 10);
        let server = testkit::serve(vec![
            (
                "/moved",
                testkit::response("302 Found", "Location: http://127.0.0.1:1/x\r\n", ""),
            ),
            ("/big", testkit::ok(&big)),
            (
                "/slow",
                testkit::response("429 Too Many Requests", "Retry-After: 900\r\n", ""),
            ),
        ]);
        let reply = get(&format!("{}/moved", server.base), &[], LOCAL_TIMEOUT).unwrap();
        assert_eq!(reply.status, 302);
        assert_eq!(server.count(), 1);
        assert_eq!(
            get(&format!("{}/big", server.base), &[], LOCAL_TIMEOUT).unwrap_err(),
            NetError::TooLarge
        );
        assert_eq!(
            get(&format!("{}/slow", server.base), &[], LOCAL_TIMEOUT)
                .unwrap()
                .retry_after,
            Some(900)
        );
    }

    #[test]
    fn a_closed_port_is_unreachable_not_a_panic() {
        let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}/x", free.local_addr().unwrap());
        drop(free);
        assert!(get(&address, &[], Duration::from_secs(1)).is_err());
    }
}
