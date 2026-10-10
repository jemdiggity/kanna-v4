use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_BODY: usize = 256 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
const REDIRECTS: usize = 4;

pub struct Response {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub fn allowed(url: &str) -> Result<(), String> {
    let loopback = ["http://127.0.0.1:", "http://127.0.0.1/", "http://localhost:", "http://localhost/"];
    if url.starts_with("https://") || loopback.iter().any(|p| url.starts_with(p)) {
        Ok(())
    } else {
        Err(format!("{url}: rules are fetched over HTTPS only (plain HTTP to this machine is allowed for tests)"))
    }
}

pub fn resolve(base: &str, link: &str) -> String {
    if link.contains("://") {
        return link.to_string();
    }
    let scheme_end = base.find("://").map(|i| i + 3).unwrap_or(0);
    let host_end = base[scheme_end..].find('/').map(|i| scheme_end + i).unwrap_or(base.len());
    if let Some(path) = link.strip_prefix('/') {
        return format!("{}/{path}", &base[..host_end]);
    }
    let dir_end = base.rfind('/').filter(|i| *i >= host_end).map(|i| i + 1).unwrap_or(base.len());
    let dir = if dir_end == base.len() && !base.ends_with('/') { format!("{base}/") } else { base[..dir_end].to_string() };
    format!("{dir}{link}")
}

pub fn origin(url: &str) -> &str {
    let start = url.find("://").map(|i| i + 3).unwrap_or(0);
    let end = url[start..].find('/').map(|i| start + i).unwrap_or(url.len());
    &url[..end]
}

pub fn get(url: &str, etag: Option<&str>) -> Result<Response, String> {
    let home = origin(url).to_ascii_lowercase();
    let mut url = url.to_string();
    for _ in 0..=REDIRECTS {
        let response = once(&url, etag)?;
        if matches!(response.status, 301 | 302 | 303 | 307 | 308) {
            let location = response.headers.get("location").ok_or_else(|| format!("{url}: a redirect without a Location"))?;
            url = resolve(&url, location);
            allowed(&url)?;
            if origin(&url).to_ascii_lowercase() != home {
                return Err(format!("{url}: a redirect to another host"));
            }
            continue;
        }
        return Ok(response);
    }
    Err(format!("{url}: too many redirects"))
}

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn once(url: &str, etag: Option<&str>) -> Result<Response, String> {
    allowed(url)?;
    let tls = url.starts_with("https://");
    let rest = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let (authority, path) = rest.split_once('/').map(|(a, p)| (a, format!("/{p}"))).unwrap_or((rest, "/".into()));
    if authority.is_empty() || authority.contains('@') {
        return Err(format!("{url}: not a URL Kanna fetches"));
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if p.bytes().all(|b| b.is_ascii_digit()) => (h, p.parse::<u16>().map_err(|_| format!("{url}: bad port"))?),
        _ => (authority, if tls { 443 } else { 80 }),
    };
    let address = (host, port).to_socket_addrs().map_err(|e| format!("{host}: {e}"))?.next().ok_or_else(|| format!("{host}: no address"))?;
    let socket = TcpStream::connect_timeout(&address, TIMEOUT).map_err(|e| format!("{host}: {e}"))?;
    socket.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    socket.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
    let mut stream: Box<dyn Stream> = if tls {
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let name = rustls::pki_types::ServerName::try_from(host.to_owned()).map_err(|e| format!("{host}: {e}"))?;
        let connection = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
        Box::new(rustls::StreamOwned::new(connection, socket))
    } else {
        Box::new(socket)
    };
    let mut request = format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nAccept: application/json\r\nUser-Agent: kanna-agent-rules\r\n");
    if let Some(etag) = etag {
        request += &format!("If-None-Match: {etag}\r\n");
    }
    request += "\r\n";
    stream.write_all(request.as_bytes()).map_err(|e| format!("{host}: {e}"))?;
    let mut raw = Vec::new();
    match stream.take((MAX_BODY + 64 * 1024) as u64).read_to_end(&mut raw) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof && !raw.is_empty() => {}
        Err(e) => return Err(format!("{host}: {e}")),
    }
    parse(&raw).map_err(|e| format!("{url}: {e}"))
}

fn parse(raw: &[u8]) -> Result<Response, String> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or("not an HTTP response")?;
    let head = String::from_utf8_lossy(&raw[..end]);
    let mut lines = head.lines();
    let status = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|s| s.parse().ok()).ok_or("no HTTP status")?;
    let headers: HashMap<String, String> =
        lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    let mut body = raw[end + 4..].to_vec();
    if headers.get("transfer-encoding").is_some_and(|t| t.eq_ignore_ascii_case("chunked")) {
        body = unchunk(&body)?;
    } else if let Some(length) = headers.get("content-length").and_then(|l| l.parse::<usize>().ok()) {
        if body.len() < length {
            return Err("the response ended early".into());
        }
        body.truncate(length);
    }
    if body.len() > MAX_BODY {
        return Err(format!("the response is over {} KiB", MAX_BODY / 1024));
    }
    Ok(Response { status, headers, body })
}

fn unchunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line_end = data.windows(2).position(|w| w == b"\r\n").ok_or("a bad chunk")?;
        let size_text = String::from_utf8_lossy(&data[..line_end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "a bad chunk size")?;
        data = &data[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if data.len() < size + 2 || out.len() + size > MAX_BODY {
            return Err("a bad or oversized chunk".into());
        }
        out.extend_from_slice(&data[..size]);
        data = &data[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_resolve_against_the_index() {
        let index = "https://jemdiggity.github.io/kanna-v4/agent-rules/index.json";
        assert_eq!(resolve(index, "claude.json"), "https://jemdiggity.github.io/kanna-v4/agent-rules/claude.json");
        assert_eq!(resolve(index, "/x.json"), "https://jemdiggity.github.io/x.json");
        assert_eq!(resolve(index, "https://example.com/a.json"), "https://example.com/a.json");
        assert_eq!(resolve("http://127.0.0.1:8000", "a.json"), "http://127.0.0.1:8000/a.json");
        assert_eq!(origin(index), "https://jemdiggity.github.io");
    }

    #[test]
    fn only_https_and_loopback_are_fetched() {
        assert!(allowed("https://example.com/i.json").is_ok());
        assert!(allowed("http://127.0.0.1:9/i.json").is_ok());
        assert!(allowed("http://example.com/i.json").is_err());
        assert!(allowed("http://127.0.0.1.example.com/i.json").is_err());
        assert!(allowed("file:///etc/passwd").is_err());
    }

    #[test]
    fn chunked_and_sized_bodies_are_read() {
        let r = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nETag: \"x\"\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n").unwrap();
        assert_eq!((r.status, r.body.as_slice(), r.headers["etag"].as_str()), (200, &b"Wikipedia"[..], "\"x\""));
        let r = parse(b"HTTP/1.1 304 Not Modified\r\nContent-Length: 0\r\n\r\n").unwrap();
        assert_eq!(r.status, 304);
        assert!(parse(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nshort").is_err());
    }
}
