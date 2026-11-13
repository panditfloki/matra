//! Explicit opt-in Ollama relay. Request/response bytes only transit memory and the socket.
//! The observer retains counts, durations, model names and activity, never conversation text.
use super::{local_history::Prediction, net};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tiny_http::{Header, Request, Response, Server, StatusCode};

const MAX_REQUEST: usize = 32 * 1024 * 1024;
const MAX_OBSERVATION: usize = 1024 * 1024;
const MAX_RESPONSE: usize = 256 * 1024 * 1024;
const MAX_ACTIVE: usize = 4;
const MAX_PENDING: usize = 1000;
static NEXT: AtomicU64 = AtomicU64::new(1);
pub fn port() -> u16 {
    if crate::platform::review_build() {
        11438
    } else {
        11435
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Options {
    enabled: bool,
}
#[derive(Clone, Serialize)]
pub struct Status {
    pub enabled: bool,
    pub ready: bool,
    pub address: String,
    pub message: String,
    pub active: usize,
    pub observed_responses: u64,
    pub observation_gaps: u64,
    pub activities: BTreeMap<String, String>,
}
#[derive(Default)]
struct Observations {
    endpoint: String,
    records: Vec<Prediction>,
    activities: BTreeMap<u64, (String, String)>,
    completed: u64,
    gaps: u64,
}
struct Running {
    stop: Arc<AtomicBool>,
    worker: std::thread::JoinHandle<()>,
    endpoint: String,
}
pub struct Relay {
    path: PathBuf,
    options: Options,
    running: Option<Running>,
    error: String,
    observations: Arc<Mutex<Observations>>,
    active: Arc<AtomicUsize>,
    bind_port: u16,
}
impl Relay {
    pub fn new(directory: &Path) -> Self {
        let path = directory.join("ollama-relay.json");
        let options = std::fs::File::open(&path)
            .ok()
            .filter(|file| file.metadata().is_ok_and(|m| m.len() <= 4096))
            .and_then(|file| serde_json::from_reader(file.take(4096)).ok())
            .unwrap_or_default();
        Self {
            path,
            options,
            running: None,
            error: String::new(),
            observations: Arc::new(Mutex::new(Observations::default())),
            active: Arc::new(AtomicUsize::new(0)),
            bind_port: port(),
        }
    }
    pub fn status(&self) -> Status {
        let observations = self.observations.lock().unwrap();
        let ready = self
            .running
            .as_ref()
            .is_some_and(|r| !r.stop.load(Ordering::Acquire));
        let active = self.active.load(Ordering::Acquire);
        let message = if !ready && active > 0 {
            "New relay requests are stopped. Existing requests are closing; a blocked upstream read can remain until its 120-second idle timeout. No responses are counted after stopping.".into()
        } else if !self.error.is_empty() {
            self.error.clone()
        } else if ready {
            "Ready. Point an Ollama client's base URL at this relay to observe its responses. Direct requests to Ollama are not observed. Prompts and replies are forwarded, never saved.".into()
        } else {
            "Off. No client traffic is routed through Mātrā. Enable the relay, then explicitly point your client at its address.".into()
        };
        Status {
            enabled: self.options.enabled,
            ready,
            address: format!("http://127.0.0.1:{}", self.bind_port),
            message,
            active,
            observed_responses: observations.completed,
            observation_gaps: observations.gaps,
            activities: observations
                .activities
                .values()
                .map(|(model, phase)| (model.clone(), phase.clone()))
                .collect(),
        }
    }
    pub fn configure(&mut self, enabled: bool, endpoint: &str) -> Result<Status, String> {
        if enabled {
            validate_endpoint(endpoint, self.bind_port)?;
        }
        let previous = self.options.clone();
        self.options.enabled = enabled;
        if let Err(error) = self.sync(endpoint) {
            self.options = previous;
            return Err(error);
        }
        let write = || -> Result<(), String> {
            std::fs::create_dir_all(
                self.path
                    .parent()
                    .ok_or("No relay preferences directory.")?,
            )
            .map_err(|_| "Cannot save relay preferences.")?;
            let pending = self.path.with_extension("json.pending");
            std::fs::write(&pending, serde_json::to_vec(&self.options).unwrap())
                .map_err(|_| "Cannot save relay preferences.")?;
            std::fs::rename(&pending, &self.path)
                .map_err(|_| "Cannot save relay preferences.".to_string())
        };
        if let Err(error) = write() {
            self.options = previous;
            self.stop();
            self.error = error.clone();
            return Err(error);
        }
        Ok(self.status())
    }
    pub fn sync(&mut self, endpoint: &str) -> Result<(), String> {
        if !self.options.enabled {
            self.stop();
            self.error.clear();
            return Ok(());
        }
        let endpoint = match validate_endpoint(endpoint, self.bind_port) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.stop();
                self.error = error.clone();
                return Err(error);
            }
        };
        if self
            .running
            .as_ref()
            .is_some_and(|r| r.endpoint == endpoint && !r.worker.is_finished())
        {
            return Ok(());
        }
        self.stop();
        self.error.clear();
        *self.observations.lock().unwrap() = Observations {
            endpoint: endpoint.clone(),
            ..Default::default()
        };
        let server = match Server::http(("127.0.0.1", self.bind_port)) {
            Ok(server) => server,
            Err(_) => {
                self.error =
                    "Cannot start the relay. Its local port is already in use or unavailable."
                        .into();
                return Err(self.error.clone());
            }
        };
        let port = server
            .server_addr()
            .to_ip()
            .ok_or("The relay has no loopback address.")?
            .port();
        self.bind_port = port;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let upstream = endpoint.clone();
        let observations = self.observations.clone();
        let active = self.active.clone();
        let worker = std::thread::Builder::new()
            .name("matra-ollama-relay".into())
            .spawn(move || {
                while !flag.load(Ordering::Acquire) {
                    let request = match server.recv_timeout(Duration::from_millis(100)) {
                        Ok(Some(request)) => request,
                        Ok(None) => continue,
                        Err(_) => break,
                    };
                    if active
                        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| {
                            (v < MAX_ACTIVE).then_some(v + 1)
                        })
                        .is_err()
                    {
                        reject(request, 503, "The relay is busy.");
                        continue;
                    }
                    let endpoint = upstream.clone();
                    let state = observations.clone();
                    let stop = flag.clone();
                    let count = active.clone();
                    let _ = std::thread::Builder::new()
                        .name("matra-ollama-request".into())
                        .spawn(move || {
                            struct Active(Arc<AtomicUsize>);
                            impl Drop for Active {
                                fn drop(&mut self) {
                                    self.0.fetch_sub(1, Ordering::AcqRel);
                                }
                            }
                            let _active = Active(count);
                            forward(request, &endpoint, port, state, stop);
                        })
                        .map_err(|_| active.fetch_sub(1, Ordering::AcqRel));
                }
                flag.store(true, Ordering::Release);
            })
            .map_err(|_| "The relay worker could not start.")?;
        self.running = Some(Running {
            stop,
            worker,
            endpoint,
        });
        Ok(())
    }
    pub fn drain(&mut self, endpoint: &str) -> Vec<Prediction> {
        let mut observations = self.observations.lock().unwrap();
        if observations.endpoint == endpoint {
            std::mem::take(&mut observations.records)
        } else {
            Vec::new()
        }
    }
    pub fn pause(&mut self) {
        self.stop();
        if self.options.enabled {
            self.error = "The Ollama source is off. Enable it to resume the optional relay.".into();
        }
    }
    fn stop(&mut self) {
        if let Some(running) = self.running.take() {
            running.stop.store(true, Ordering::Release);
            let _ = running.worker.join();
        }
        self.observations.lock().unwrap().activities.clear();
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn validate_endpoint(endpoint: &str, port: u16) -> Result<String, String> {
    let endpoint = net::parse_loopback(endpoint)?;
    if url::Url::parse(&endpoint).is_ok_and(|url| url.port_or_known_default() == Some(port)) {
        return Err("The upstream Ollama port must differ from the relay port.".into());
    }
    Ok(endpoint)
}
fn allowed_origin(origin: &str) -> bool {
    url::Url::parse(origin).is_ok_and(|u| {
        matches!(u.scheme(), "http" | "https")
            && net::is_loopback(&u)
            && u.username().is_empty()
            && u.password().is_none()
            && u.path() == "/"
            && u.query().is_none()
            && u.fragment().is_none()
    })
}
fn allowed_request(request: &Request, port: u16) -> bool {
    let values = |name: &str| {
        request
            .headers()
            .iter()
            .filter(move |h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
            .map(|h| h.value.as_str())
            .collect::<Vec<_>>()
    };
    let hosts = values("host");
    let origins = values("origin");
    hosts.len() == 1
        && [format!("127.0.0.1:{port}"), format!("localhost:{port}")]
            .contains(&hosts[0].to_lowercase())
        && origins.len() <= 1
        && origins.iter().all(|o| allowed_origin(o))
        && !values("sec-fetch-site")
            .iter()
            .any(|v| v.eq_ignore_ascii_case("cross-site"))
        && values("upgrade").is_empty()
        && !request.url().contains('\\')
        && !request.url().starts_with("//")
}
fn reject(request: Request, status: u16, message: &str) {
    let _ = request.respond(Response::from_string(message).with_status_code(StatusCode(status)));
}
fn forward(
    mut request: Request,
    endpoint: &str,
    port: u16,
    observations: Arc<Mutex<Observations>>,
    stop: Arc<AtomicBool>,
) {
    if !allowed_request(&request, port) {
        reject(request, 403, "This relay accepts local clients only.");
        return;
    }
    let target = request.url().to_owned();
    let path = target.split('?').next().unwrap_or("");
    let method = request.method().as_str().to_owned();
    let inference = method == "POST"
        && matches!(
            path,
            "/api/chat" | "/api/generate" | "/v1/chat/completions" | "/v1/completions"
        );
    let listing = method == "GET"
        && matches!(
            path,
            "/api/tags" | "/api/ps" | "/api/version" | "/v1/models"
        );
    if !inference && !listing {
        reject(
            request,
            405,
            "Only model listing and completion requests are supported.",
        );
        return;
    }
    if request.body_length().is_some_and(|n| n > MAX_REQUEST) {
        reject(request, 413, "Request exceeds the relay limit.");
        return;
    }
    let mut body = Vec::new();
    if request
        .as_reader()
        .take(MAX_REQUEST as u64 + 1)
        .read_to_end(&mut body)
        .is_err()
    {
        reject(request, 400, "The request could not be read.");
        return;
    }
    if body.len() > MAX_REQUEST {
        reject(request, 413, "Request exceeds the relay limit.");
        return;
    }
    if stop.load(Ordering::Acquire) {
        reject(request, 503, "The relay is stopping.");
        return;
    }
    let parsed = serde_json::from_slice::<Value>(&body).ok();
    let model = parsed
        .as_ref()
        .and_then(|v| v["model"].as_str())
        .filter(|v| !v.is_empty() && v.len() <= 256)
        .unwrap_or("")
        .to_owned();
    let streaming = parsed
        .as_ref()
        .and_then(|v| v["stream"].as_bool())
        .unwrap_or(path.starts_with("/api/"));
    drop(parsed);
    let agent = crate::http::builder()
        .try_proxy_from_env(false)
        .redirects(0)
        .timeout_connect(Duration::from_secs(3))
        .timeout_read(Duration::from_secs(120))
        .timeout_write(Duration::from_secs(30))
        .timeout(Duration::from_secs(600))
        .build();
    let mut upstream = agent
        .request(&method, &format!("{endpoint}{target}"))
        .set("Accept-Encoding", "identity");
    for header in request.headers() {
        if ["authorization", "content-type", "accept"]
            .iter()
            .any(|name| header.field.as_str().as_str().eq_ignore_ascii_case(name))
        {
            upstream = upstream.set(header.field.as_str().as_str(), header.value.as_str());
        }
    }
    let reply = if inference {
        upstream.send_bytes(&body)
    } else {
        upstream.call()
    };
    drop(body);
    let response = match reply {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(_) => {
            reject(request, 502, "The Ollama upstream did not answer.");
            return;
        }
    };
    let status = response.status();
    let headers = ["content-type", "cache-control"]
        .into_iter()
        .filter_map(|name| {
            response
                .header(name)
                .and_then(|value| Header::from_bytes(name.as_bytes(), value.as_bytes()).ok())
        })
        .collect();
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let observer = Observer::new(path, &model, streaming, id);
    if inference && !model.is_empty() {
        observations
            .lock()
            .unwrap()
            .activities
            .insert(id, (model.clone(), "generating".into()));
    }
    let reader = ObservedReader {
        inner: response.into_reader(),
        observer,
        observations: observations.clone(),
        stop: stop.clone(),
        consumed: 0,
        accepted: inference && (200..300).contains(&status),
        id,
    };
    let _ = request.respond(Response::new(
        StatusCode(status),
        headers,
        reader,
        None,
        None,
    ));
    observations.lock().unwrap().activities.remove(&id);
}

struct Observer {
    model: String,
    sse: bool,
    streaming: bool,
    pending: Vec<u8>,
    disabled: bool,
    completed: bool,
    measured: bool,
    candidate: Option<Prediction>,
    id: u64,
    phase: Option<&'static str>,
}
impl Observer {
    fn new(path: &str, model: &str, streaming: bool, id: u64) -> Self {
        Self {
            model: model.into(),
            sse: path.starts_with("/v1/"),
            streaming,
            pending: Vec::new(),
            disabled: false,
            completed: false,
            measured: false,
            candidate: None,
            id,
            phase: Some("generating"),
        }
    }
    fn append(&mut self, bytes: &[u8]) -> Vec<Prediction> {
        let mut found = Vec::new();
        if self.disabled || self.completed {
            return found;
        }
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            if self.pending.len() + part.len() > MAX_OBSERVATION {
                self.pending.clear();
                self.disabled = true;
                break;
            }
            self.pending.extend_from_slice(part);
            if self.streaming && part.last() == Some(&b'\n') {
                let line = std::mem::take(&mut self.pending);
                if let Some(p) = self.consume(&line) {
                    found.push(p);
                }
                if self.completed {
                    break;
                }
            }
        }
        found
    }
    fn finish(&mut self) -> Option<Prediction> {
        if self.disabled || self.completed {
            return None;
        }
        let line = std::mem::take(&mut self.pending);
        let found = self.consume(&line);
        if found.is_some() {
            return found;
        }
        let candidate = self.candidate.take();
        if candidate.is_some() {
            self.completed = true;
            self.measured = true;
            self.phase = None;
        }
        candidate
    }
    fn consume(&mut self, bytes: &[u8]) -> Option<Prediction> {
        let text = std::str::from_utf8(bytes).ok()?.trim();
        let text = if self.sse && self.streaming {
            text.strip_prefix("data:")?.trim()
        } else {
            text
        };
        if text == "[DONE]" {
            self.completed = true;
            self.phase = None;
            self.measured = self.candidate.is_some();
            return self.candidate.take();
        }
        let value: Value = serde_json::from_str(text).ok()?;
        if value.get("error").is_some() {
            self.disabled = true;
            self.phase = None;
            self.candidate = None;
            return None;
        }
        if let Some(model) = value["model"]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 256)
        {
            self.model = model.into();
        }
        if value["thinking"].as_str().is_some_and(|s| !s.is_empty())
            || value["message"]["thinking"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        {
            self.phase = Some("thinking");
        }
        if value["response"].as_str().is_some_and(|s| !s.is_empty())
            || value["message"]["content"]
                .as_str()
                .is_some_and(|s| !s.is_empty())
        {
            self.phase = Some("generating");
        }
        for choice in value["choices"].as_array().into_iter().flatten() {
            let delta = &choice["delta"];
            if delta["reasoning"].as_str().is_some_and(|s| !s.is_empty())
                || delta["reasoning_content"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
            {
                self.phase = Some("thinking");
            }
            if delta["content"].as_str().is_some_and(|s| !s.is_empty())
                || delta["tool_calls"]
                    .as_array()
                    .is_some_and(|v| !v.is_empty())
            {
                self.phase = Some("generating");
            }
        }
        let counts = if self.sse { &value["usage"] } else { &value };
        let input = counts[if self.sse {
            "prompt_tokens"
        } else {
            "prompt_eval_count"
        }]
        .as_u64()
        .filter(|v| *v < 1_000_000_000_000);
        let output = counts[if self.sse {
            "completion_tokens"
        } else {
            "eval_count"
        }]
        .as_u64()
        .filter(|v| *v < 1_000_000_000_000);
        let done = value["done"].as_bool() == Some(true) || (self.sse && !self.streaming);
        if (input.is_some() || output.is_some()) && (self.sse || done) && !self.model.is_empty() {
            let seconds = value["eval_duration"]
                .as_u64()
                .filter(|v| *v > 0)
                .map(|v| v as f64 / 1e9);
            let p = Prediction {
                model: self.model.clone(),
                at: super::model::now_ms(),
                input_tokens: input,
                output_tokens: output,
                reasoning_tokens: counts["completion_tokens_details"]["reasoning_tokens"].as_u64(),
                tokens_per_second: output
                    .zip(seconds)
                    .filter(|(n, _)| *n > 0)
                    .map(|(n, t)| n as f64 / t),
                generation_seconds: seconds,
                identity: format!("relay-{}-{}", super::model::now_ms(), self.id),
                ..Default::default()
            };
            if done {
                self.completed = true;
                self.measured = true;
                self.phase = None;
                Some(p)
            } else {
                self.candidate = Some(p);
                None
            }
        } else {
            None
        }
    }
}
struct ObservedReader {
    inner: Box<dyn Read + Send + Sync>,
    observer: Observer,
    observations: Arc<Mutex<Observations>>,
    stop: Arc<AtomicBool>,
    consumed: usize,
    accepted: bool,
    id: u64,
}
impl ObservedReader {
    fn record(&self, records: Vec<Prediction>) {
        if self.stop.load(Ordering::Acquire) {
            return;
        }
        let mut state = self.observations.lock().unwrap();
        for p in records {
            state.completed += 1;
            if state.records.len() < MAX_PENDING {
                state.records.push(p);
            } else {
                state.gaps += 1;
            }
        }
    }
}
impl Read for ObservedReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.stop.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Relay stopped"));
        }
        let n = self.inner.read(buffer)?;
        if self.stop.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Relay stopped"));
        }
        self.consumed += n;
        if self.consumed > MAX_RESPONSE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Relay response limit",
            ));
        }
        if self.accepted {
            let records = if n == 0 {
                self.observer.finish().into_iter().collect()
            } else {
                self.observer.append(&buffer[..n])
            };
            self.record(records);
            let mut state = self.observations.lock().unwrap();
            if let Some(phase) = self
                .observer
                .phase
                .filter(|_| !self.observer.model.is_empty())
            {
                state
                    .activities
                    .insert(self.id, (self.observer.model.clone(), phase.into()));
            } else {
                state.activities.remove(&self.id);
            }
        }
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stopping_during_an_upstream_read_does_not_forward_or_count_the_returned_chunk() {
        struct StopOnRead(Arc<AtomicBool>);
        impl Read for StopOnRead {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                self.0.store(true, Ordering::Release);
                buffer[0] = b'x';
                Ok(1)
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let observations = Arc::new(Mutex::new(Observations::default()));
        let mut reader = ObservedReader {
            inner: Box::new(StopOnRead(stop.clone())),
            observer: Observer::new("/api/chat", "sample", true, 0),
            observations: observations.clone(),
            stop,
            consumed: 0,
            accepted: true,
            id: 0,
        };
        let error = reader.read(&mut [0; 32]).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(observations.lock().unwrap().records.is_empty());
        assert_eq!(reader.consumed, 0);
    }
    fn native() -> &'static [u8] {
        b"{\"model\":\"sample:latest\",\"message\":{\"thinking\":\"PRIVATE_THINKING\"},\"done\":false}\n{\"model\":\"sample:latest\",\"message\":{\"content\":\"PRIVATE_REPLY\"},\"done\":false}\n{\"model\":\"sample:latest\",\"done\":true,\"prompt_eval_count\":10,\"eval_count\":20,\"eval_duration\":2000000000}\n"
    }
    #[test]
    fn native_stream_counts_once_and_keeps_no_text() {
        let mut observer = Observer::new("/api/chat", "sample", true, 1);
        let mut records = Vec::new();
        for chunk in native().chunks(3) {
            records.extend(observer.append(chunk));
        }
        assert_eq!(records.len(), 1);
        assert!(observer.finish().is_none());
        assert_eq!(
            (
                records[0].input_tokens,
                records[0].output_tokens,
                records[0].tokens_per_second
            ),
            (Some(10), Some(20), Some(10.0))
        );
        assert!(!serde_json::to_string(&records).unwrap().contains("PRIVATE"));
        assert!(observer.pending.is_empty());
    }
    #[test]
    fn nonstream_and_sse_usage_have_no_invented_speed() {
        let mut plain = Observer::new("/api/generate", "m", false, 2);
        plain.append(b"{\"done\":true,\"eval_count\":2,\"response\":\"PRIVATE\"}");
        let p = plain.finish().unwrap();
        assert_eq!(p.input_tokens, None);
        assert_eq!(p.tokens_per_second, None);
        let mut sse = Observer::new("/v1/chat/completions", "m", true, 3);
        assert!(sse
            .append(b"data: {\"usage\":{\"prompt_tokens\":4,\"completion_tokens\":7}}\n\n")
            .is_empty());
        let p = sse.append(b"data: [DONE]\n").pop().unwrap();
        assert_eq!(p.output_tokens, Some(7));
        assert_eq!(p.generation_seconds, None);
    }
    #[test]
    fn malformed_oversized_cancelled_and_error_streams_do_not_count() {
        let mut observer = Observer::new("/api/chat", "m", true, 1);
        assert!(observer
            .append(b"{bad}\n{\"done\":false,\"eval_count\":1}\n")
            .is_empty());
        assert!(observer.finish().is_none());
        let mut observer = Observer::new("/api/chat", "m", true, 1);
        assert!(observer.append(&vec![b'x'; MAX_OBSERVATION + 1]).is_empty());
        assert!(observer.disabled);
        assert!(observer.pending.is_empty());
        let mut observer = Observer::new("/api/chat", "m", true, 1);
        assert!(observer
            .append(b"{\"error\":\"PRIVATE\",\"done\":true,\"eval_count\":1}\n")
            .is_empty());
        assert!(!allowed_origin("null"));
        assert!(!allowed_origin("https://example.com"));
        assert!(!allowed_origin("http://localhost/attack"));
        assert!(allowed_origin("http://localhost:8080"));
        assert!(validate_endpoint("http://127.0.0.1:11438", 11438).is_err());
    }
    #[test]
    fn explicit_relay_forwards_bytes_and_rejects_cross_site_clients() {
        let root = std::env::temp_dir().join(format!("matra-relay-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let upstream = Server::http(("127.0.0.1", 0)).unwrap();
        let endpoint = format!("http://{}", upstream.server_addr());
        let served = std::thread::spawn(move || {
            let mut request = upstream
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .expect("forwarded request");
            let mut body = String::new();
            request.as_reader().read_to_string(&mut body).unwrap();
            assert!(body.contains("PRIVATE_PROMPT"));
            request
                .respond(Response::from_data(native().to_vec()).with_header(
                    Header::from_bytes("Content-Type", "application/x-ndjson").unwrap(),
                ))
                .unwrap();
        });
        let mut relay = Relay::new(&root);
        relay.bind_port = 0;
        assert!(!relay.status().ready);
        let status = relay.configure(true, &endpoint).unwrap();
        assert!(status.ready);
        let forbidden = ureq::post(&format!("{}/api/chat", status.address))
            .set("Origin", "https://example.com")
            .send_string("{}");
        assert!(matches!(forbidden, Err(ureq::Error::Status(403, _))));
        let response = ureq::post(&format!("{}/api/chat", status.address))
            .set("Content-Type", "application/json")
            .send_string("{\"model\":\"sample\",\"messages\":[{\"content\":\"PRIVATE_PROMPT\"}]}")
            .unwrap();
        let mut actual = Vec::new();
        response.into_reader().read_to_end(&mut actual).unwrap();
        assert_eq!(actual, native());
        served.join().unwrap();
        for _ in 0..50 {
            if relay.status().active == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let records = relay.drain(&endpoint);
        assert_eq!(records.len(), 1);
        assert_eq!(relay.status().observed_responses, 1);
        let mut history = super::super::local_history::Store::new(&root.join("history"));
        let now = super::super::model::now_ms();
        history.record_ollama(&endpoint, records, true, 0, now);
        let summary = history.ollama_summary(&endpoint, now);
        assert_eq!(summary["today"]["input_tokens"], 10);
        assert_eq!(summary["today"]["output_tokens"], 20);
        assert!(
            !std::fs::read_to_string(root.join("history/local-history-ollama.json"))
                .unwrap()
                .contains("PRIVATE")
        );
        assert_eq!(relay.status().active, 0);
        assert!(relay.status().activities.is_empty());
        relay.configure(false, &endpoint).unwrap();
        assert!(!relay.status().ready);
        assert!(!std::fs::read_to_string(root.join("ollama-relay.json"))
            .unwrap()
            .contains("PRIVATE"));
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn stopping_preserves_completed_records_for_the_ledger_and_bind_errors_are_visible() {
        let root = std::env::temp_dir().join(format!("matra-relay-stop-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut relay = Relay::new(&root);
        relay.bind_port = 0;
        relay.configure(true, "http://127.0.0.1:11434").unwrap();
        let blocker = Server::http(("127.0.0.1", 0)).unwrap();
        let busy_port = blocker.server_addr().to_ip().unwrap().port();
        relay.observations.lock().unwrap().records.push(Prediction {
            model: "m".into(),
            output_tokens: Some(4),
            ..Default::default()
        });
        relay.configure(false, "http://127.0.0.1:11434").unwrap();
        assert_eq!(relay.drain("http://127.0.0.1:11434").len(), 1);
        relay.bind_port = busy_port;
        assert!(relay.configure(true, "http://127.0.0.1:11434").is_err());
        assert!(!relay.status().ready);
        assert!(relay.status().message.contains("port"));
        drop(blocker);
        std::fs::remove_dir_all(root).unwrap();
    }
}
impl Drop for ObservedReader {
    fn drop(&mut self) {
        let mut state = self.observations.lock().unwrap();
        state.activities.remove(&self.id);
        if !self.stop.load(Ordering::Acquire) && self.accepted && !self.observer.measured {
            state.gaps += 1;
        }
    }
}
