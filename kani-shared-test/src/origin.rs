//! A controllable HTTP origin for integration tests.
//!
//! Kani's riskiest behaviour only appears against a *misbehaving* server:
//! listings that change between scans, servers that ignore `Range`, bodies that
//! stop halfway through the length they announced, sequences of 429s. None of
//! that can be arranged against a real source, and a framework-based mock will
//! not produce it either — a correct HTTP stack refuses to send a wrong
//! `Content-Length`. So this is written straight onto a `TcpStream`, which is
//! the only way to be deliberately wrong.
//!
//! Loopback IP literals bypass `ValidatingResolver` (it is a DNS resolver, and
//! rquest only consults it for hostnames), so the production `SmartClient`
//! reaches this server unmodified. Tests exercise the real request path.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// How a route answers. Everything here except `Bytes` is a way of being wrong
/// on purpose.
#[derive(Clone, Debug)]
pub enum Body {
    Bytes(Vec<u8>),
    /// Announce `announced` bytes, send the first `sent`, then close the socket
    /// mid-body — an interrupted download.
    Truncated {
        bytes: Vec<u8>,
        announced: usize,
        sent: usize,
    },
    /// `200` with `Content-Length: 0`.
    Empty,
    /// Accept the connection, send nothing, hold it open.
    Stall,
    /// Accept the connection and reset it without writing a byte.
    Reset,
    /// Answer with a JSON description of the request that arrived — method,
    /// path, query and headers.
    ///
    /// The only way to assert what a source *actually put on the wire*, as
    /// opposed to what it intended to. Filter mapping, URL interpolation and
    /// preference injection are all invisible without this.
    Echo,
}

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Body,
    /// Optional pause before the response head is written — a "slow origin". Used
    /// to keep concurrent requests in flight (coalescing) or to trip a client
    /// read timeout without holding the socket forever like [`Body::Stall`].
    pub delay: Option<std::time::Duration>,
}

impl Response {
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: Vec::new(),
            body: Body::Bytes(body.into()),
            delay: None,
        }
    }

    pub fn html(body: &str) -> Self {
        Self::ok(body.as_bytes().to_vec()).header("Content-Type", "text/html; charset=utf-8")
    }

    pub fn json(body: &str) -> Self {
        Self::ok(body.as_bytes().to_vec()).header("Content-Type", "application/json")
    }

    pub fn image(bytes: Vec<u8>) -> Self {
        Self::ok(bytes).header("Content-Type", "image/jpeg")
    }

    pub fn status(status: u16) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: Body::Empty,
            delay: None,
        }
    }

    pub fn redirect(status: u16, location: &str) -> Self {
        Self::status(status).header("Location", location)
    }

    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// A route that reflects the request back as JSON. See [`Body::Echo`].
    pub fn echo() -> Self {
        Self {
            status: 200,
            headers: vec![("Content-Type".into(), "application/json".into())],
            body: Body::Echo,
            delay: None,
        }
    }

    pub fn body(mut self, body: Body) -> Self {
        self.body = body;
        self
    }

    /// Pause for `d` before responding (a slow origin).
    pub fn delay(mut self, d: std::time::Duration) -> Self {
        self.delay = Some(d);
        self
    }
}

#[derive(Default)]
struct Route {
    /// Consumed front to back; the last entry repeats once exhausted.
    responses: Vec<Response>,
    cursor: usize,
}

/// What actually arrived on the wire for one request.
#[derive(Clone, Debug, Default)]
pub struct SeenRequest {
    pub method: String,
    pub path: String,
    pub query: Option<String>,
    /// Header names lowercased.
    pub headers: Vec<(String, String)>,
    /// The request body, read to its `Content-Length`.
    pub body: Vec<u8>,
}

impl SeenRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        let want = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == want)
            .map(|(_, v)| v.as_str())
    }

    /// `?a=1&b=2` → the value of `a`.
    pub fn query_param(&self, name: &str) -> Option<String> {
        self.query.as_deref()?.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=')?;
            (k == name).then(|| v.to_string())
        })
    }

    fn to_json(&self) -> String {
        let headers: Vec<String> = self
            .headers
            .iter()
            .map(|(k, v)| format!("{}:{}", json_escape(k), json_escape(v)))
            .map(|s| format!("\"{s}\""))
            .collect();
        format!(
            "{{\"method\":\"{}\",\"path\":\"{}\",\"query\":\"{}\",\"headers\":[{}]}}",
            json_escape(&self.method),
            json_escape(&self.path),
            json_escape(self.query.as_deref().unwrap_or("")),
            headers.join(",")
        )
    }
}

fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[derive(Default)]
struct OriginState {
    routes: Mutex<HashMap<String, Route>>,
    hits: Mutex<HashMap<String, usize>>,
    last_request: Mutex<HashMap<String, SeenRequest>>,
    total_hits: AtomicUsize,
    ignore_range: AtomicBool,
}

pub struct TestOrigin {
    port: u16,
    state: Arc<OriginState>,
}

impl TestOrigin {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let state = Arc::new(OriginState::default());

        let serve_state = Arc::clone(&state);
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let s = Arc::clone(&serve_state);
                tokio::spawn(async move {
                    handle(stream, s).await;
                });
            }
        });

        Self { port, state }
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base(), path)
    }

    /// Answers `path` with `response` every time.
    pub fn set(&self, path: &str, response: Response) {
        self.script(path, vec![response]);
    }

    /// Answers `path` with each response in turn; the last one repeats. This is
    /// how "429, 429, then 200" and "a 3-chapter listing, then a 5-chapter
    /// listing" are expressed.
    pub fn script(&self, path: &str, responses: Vec<Response>) {
        let mut routes = self.state.routes.lock().expect("routes lock");
        routes.insert(
            path.to_string(),
            Route {
                responses,
                cursor: 0,
            },
        );
    }

    /// Requests served for this exact path, ignoring the query string.
    pub fn hits(&self, path: &str) -> usize {
        self.state
            .hits
            .lock()
            .expect("hits lock")
            .get(path)
            .copied()
            .unwrap_or(0)
    }

    /// What the origin last received for this path — the assertion surface for
    /// "did the source actually send what it declared?".
    pub fn last_request(&self, path: &str) -> Option<SeenRequest> {
        self.state
            .last_request
            .lock()
            .expect("last_request lock")
            .get(path)
            .cloned()
    }

    pub fn total_hits(&self) -> usize {
        self.state.total_hits.load(Ordering::SeqCst)
    }

    /// When set, the server answers `200` with the whole body even for a
    /// `Range` request — the case that makes `content_range_total` fall back to
    /// `Content-Length`, and which no real source can be asked to perform.
    pub fn ignore_range(&self, ignore: bool) {
        self.state.ignore_range.store(ignore, Ordering::SeqCst);
    }
}

async fn handle(mut stream: tokio::net::TcpStream, state: Arc<OriginState>) {
    let mut buf = vec![0u8; 16 * 1024];
    let Ok(n) = stream.read(&mut buf).await else {
        return;
    };
    if n == 0 {
        return;
    }
    let mut raw = buf[..n].to_vec();
    let header_end = loop {
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        let Ok(more) = stream.read(&mut buf).await else {
            return;
        };
        if more == 0 {
            break raw.len();
        }
        raw.extend_from_slice(&buf[..more]);
    };
    let content_length = String::from_utf8_lossy(&raw[..header_end])
        .lines()
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while raw.len() < header_end + content_length {
        let Ok(more) = stream.read(&mut buf).await else {
            return;
        };
        if more == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..more]);
    }
    let body = raw[header_end..].to_vec();
    let request = String::from_utf8_lossy(&raw[..header_end]).to_string();

    let mut request_lines = request.lines();
    let start_line = request_lines.next().unwrap_or_default();
    let method = start_line
        .split_whitespace()
        .next()
        .unwrap_or("GET")
        .to_string();
    let path = start_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    let bare_path = path.split('?').next().unwrap_or("/").to_string();
    let query = path.split_once('?').map(|(_, q)| q.to_string());

    let headers: Vec<(String, String)> = request_lines
        .take_while(|l| !l.is_empty())
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();

    let seen = SeenRequest {
        method: method.clone(),
        path: bare_path.clone(),
        query: query.clone(),
        headers: headers.clone(),
        body,
    };
    state
        .last_request
        .lock()
        .expect("last_request lock")
        .insert(bare_path.clone(), seen.clone());

    state.total_hits.fetch_add(1, Ordering::SeqCst);
    *state
        .hits
        .lock()
        .expect("hits lock")
        .entry(bare_path.clone())
        .or_insert(0) += 1;

    let response = {
        let mut routes = state.routes.lock().expect("routes lock");
        // Exact path first, then the query-stripped form, so a test can pin one
        // specific query without having to register every variant.
        let key = if routes.contains_key(&path) {
            path.clone()
        } else {
            bare_path.clone()
        };
        match routes.get_mut(&key) {
            Some(r) if !r.responses.is_empty() => {
                let idx = r.cursor.min(r.responses.len() - 1);
                r.cursor += 1;
                Some(r.responses[idx].clone())
            }
            _ => None,
        }
    };

    let Some(response) = response else {
        let _ = stream
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
        let _ = stream.shutdown().await;
        return;
    };

    let range = parse_range(&request);
    let honour_range = range.is_some() && !state.ignore_range.load(Ordering::SeqCst);

    if let Some(d) = response.delay {
        tokio::time::sleep(d).await;
    }

    match response.body {
        Body::Echo => {
            let payload = seen.to_json();
            let head = build_head(
                response.status,
                &response.headers,
                Some(payload.len()),
                None,
            );
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(payload.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
        Body::Stall => {
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        }
        Body::Reset => {
            drop(stream);
        }
        Body::Empty => {
            let head = build_head(response.status, &response.headers, Some(0), None);
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.shutdown().await;
        }
        Body::Truncated {
            bytes,
            announced,
            sent,
        } => {
            let head = build_head(response.status, &response.headers, Some(announced), None);
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&bytes[..sent.min(bytes.len())]).await;
            // No shutdown: drop mid-body, which is what an interrupted transfer
            // looks like to the client.
            drop(stream);
        }
        Body::Bytes(bytes) => {
            let total = bytes.len();
            let (status, slice, content_range) = match (honour_range, range) {
                (true, Some((start, end))) => {
                    let end = end
                        .unwrap_or(total.saturating_sub(1))
                        .min(total.saturating_sub(1));
                    let start = start.min(total);
                    (
                        206,
                        bytes[start..=end.max(start)].to_vec(),
                        Some(format!("bytes {start}-{end}/{total}")),
                    )
                }
                _ => (response.status, bytes, None),
            };
            let head = build_head(status, &response.headers, Some(slice.len()), content_range);
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&slice).await;
            let _ = stream.shutdown().await;
        }
    }
}

fn build_head(
    status: u16,
    headers: &[(String, String)],
    content_length: Option<usize>,
    content_range: Option<String>,
) -> String {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Status",
    };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\n");
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(cr) = content_range {
        head.push_str(&format!("Content-Range: {cr}\r\n"));
        head.push_str("Accept-Ranges: bytes\r\n");
    }
    if let Some(len) = content_length {
        head.push_str(&format!("Content-Length: {len}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    head
}

/// `Range: bytes=0-4095` → `(0, Some(4095))`.
fn parse_range(request: &str) -> Option<(usize, Option<usize>)> {
    let line = request
        .lines()
        .find(|l| l.to_ascii_lowercase().starts_with("range:"))?;
    let spec = line.split_once('=')?.1.trim();
    let (start, end) = spec.split_once('-')?;
    let start: usize = start.trim().parse().ok()?;
    let end = end.trim();
    let end = if end.is_empty() {
        None
    } else {
        Some(end.parse().ok()?)
    };
    Some((start, end))
}

/// A JPEG of exactly these dimensions, encoded at `quality`, either colour or
/// grey *in content* — which is what the probe and the manifest actually judge,
/// as opposed to what the encoding is capable of.
pub fn jpeg_page(width: u32, height: u32, colour: bool, quality: u8) -> Vec<u8> {
    let mut img = image::RgbImage::new(width, height);
    for (x, y, p) in img.enumerate_pixels_mut() {
        let shade = ((x + y) % 200) as u8 + 20;
        *p = if colour {
            image::Rgb([shade, 40, 255 - shade])
        } else {
            image::Rgb([shade, shade, shade])
        };
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    encoder
        .encode_image(&image::DynamicImage::ImageRgb8(img))
        .expect("encode jpeg");
    out.into_inner()
}

pub fn png_page(width: u32, height: u32, colour: bool) -> Vec<u8> {
    let img = if colour {
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
            width,
            height,
            image::Rgb([200, 30, 30]),
        ))
    } else {
        image::DynamicImage::ImageLuma8(image::GrayImage::from_pixel(
            width,
            height,
            image::Luma([128]),
        ))
    };
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .expect("encode png");
    out.into_inner()
}

/// A JPEG cut off after `keep` bytes — enough for a probe to see a header, not
/// enough to decode.
pub fn truncated_jpeg(keep: usize) -> Vec<u8> {
    let full = jpeg_page(1600, 2400, false, 80);
    full[..keep.min(full.len())].to_vec()
}

/// Greyscale *content* in a JPEG.
///
/// Note this is still three-component YCbCr: the `image` crate has no
/// single-component encoder, which is exactly the situation `probe::jpeg_is_colour`
/// documents. A header probe therefore reads this as `Unknown`, while the
/// manifest — which decodes pixels — reads it as monochrome. Use `png_page` for
/// a fixture whose colour-ness is conclusive from the header alone.
pub fn greyscale_jpeg(width: u32, height: u32, quality: u8) -> Vec<u8> {
    let mut img = image::GrayImage::new(width, height);
    for (x, y, p) in img.enumerate_pixels_mut() {
        *p = image::Luma([((x + y) % 200) as u8 + 20]);
    }
    let mut out = std::io::Cursor::new(Vec::new());
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    encoder
        .encode_image(&image::DynamicImage::ImageLuma8(img))
        .expect("encode grey jpeg");
    out.into_inner()
}
