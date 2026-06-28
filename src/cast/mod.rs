use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, UdpSocket};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CastDeviceKind {
    UPnP,
    Chromecast,
}

#[derive(Clone, Debug)]
pub struct CastDevice {
    pub id: String,
    pub name: String,
    pub kind: CastDeviceKind,
    pub host: String,
    pub port: u16,
    pub av_transport_url: String,
}

impl CastDevice {
    pub fn kind_label(&self) -> &'static str {
        match self.kind {
            CastDeviceKind::UPnP => "UPnP/DLNA",
            CastDeviceKind::Chromecast => "Chromecast",
        }
    }
}

// ── CastSession — persistent session for sync'd playback ─────────────────────

pub enum CastCommand {
    Load { url: String, content_type: String },
    Play,
    Pause,
    Seek(f64),
    Stop,
}

#[derive(Clone, Debug)]
pub enum CastEvent {
    Playing { current_time: f64 },
    Paused { current_time: f64 },
    TrackFinished,
    Error(String),
    Disconnected,
}

pub struct CastSession {
    pub device: CastDevice,
    cmd_tx: mpsc::SyncSender<CastCommand>,
    event_rx: mpsc::Receiver<CastEvent>,
}

impl CastSession {
    pub fn connect(device: CastDevice) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::sync_channel::<CastCommand>(16);
        let (event_tx, event_rx) = mpsc::channel::<CastEvent>();

        let host = device.host.clone();
        let port = device.port;

        std::thread::spawn(move || {
            run_cast_session(host, port, cmd_rx, event_tx);
        });

        Ok(CastSession { device, cmd_tx, event_rx })
    }

    pub fn load(&self, url: String, content_type: String) {
        let _ = self.cmd_tx.try_send(CastCommand::Load { url, content_type });
    }

    pub fn play(&self) {
        let _ = self.cmd_tx.try_send(CastCommand::Play);
    }

    pub fn pause(&self) {
        let _ = self.cmd_tx.try_send(CastCommand::Pause);
    }

    pub fn seek(&self, position_secs: f64) {
        let _ = self.cmd_tx.try_send(CastCommand::Seek(position_secs));
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.try_send(CastCommand::Stop);
    }

    pub fn try_recv_event(&self) -> Option<CastEvent> {
        self.event_rx.try_recv().ok()
    }
}

const CONNECTION_NS: &str = "urn:x-cast:com.google.cast.tp.connection";
const HEARTBEAT_NS: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
const RECEIVER_NS: &str = "urn:x-cast:com.google.cast.receiver";
const MEDIA_NS: &str = "urn:x-cast:com.google.cast.media";

fn run_cast_session(
    host: String,
    port: u16,
    cmd_rx: mpsc::Receiver<CastCommand>,
    event_tx: mpsc::Sender<CastEvent>,
) {
    let addr = format!("{host}:{port}");
    let tcp = match TcpStream::connect(&addr) {
        Ok(t) => t,
        Err(e) => {
            let _ = event_tx.send(CastEvent::Error(format!("connect: {e}")));
            let _ = event_tx.send(CastEvent::Disconnected);
            return;
        }
    };
    tcp.set_write_timeout(Some(Duration::from_secs(5))).ok();
    tcp.set_read_timeout(Some(Duration::from_millis(150))).ok();

    let connector = match native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            let _ = event_tx.send(CastEvent::Error(format!("TLS build: {e}")));
            let _ = event_tx.send(CastEvent::Disconnected);
            return;
        }
    };

    let mut tls = match connector.connect(&host, tcp) {
        Ok(t) => t,
        Err(e) => {
            let _ = event_tx.send(CastEvent::Error(format!("TLS handshake: {e}")));
            let _ = event_tx.send(CastEvent::Disconnected);
            return;
        }
    };

    if cast_send(&mut tls, "sender-0", "receiver-0", CONNECTION_NS,
        r#"{"type":"CONNECT","origin":{},"userAgent":"gtunes"}"#).is_err()
        || cast_send(&mut tls, "sender-0", "receiver-0", RECEIVER_NS,
            r#"{"type":"LAUNCH","appId":"CC1AD845","requestId":1}"#).is_err()
    {
        let _ = event_tx.send(CastEvent::Disconnected);
        return;
    }

    let transport_id = match wait_for_transport(&mut tls) {
        Ok(id) => id,
        Err(e) => {
            let _ = event_tx.send(CastEvent::Error(e));
            let _ = event_tx.send(CastEvent::Disconnected);
            return;
        }
    };

    if cast_send(&mut tls, "sender-0", &transport_id, CONNECTION_NS,
        r#"{"type":"CONNECT","origin":{}}"#).is_err()
    {
        let _ = event_tx.send(CastEvent::Disconnected);
        return;
    }

    let mut media_session_id: Option<u32> = None;
    let mut req_id: u32 = 10;

    loop {
        // Drain all pending commands
        loop {
            match cmd_rx.try_recv() {
                Ok(CastCommand::Load { url, content_type }) => {
                    req_id += 1;
                    media_session_id = None;
                    let json = format!(
                        r#"{{"type":"LOAD","requestId":{req},"media":{{"contentId":"{url}","contentType":"{ct}","streamType":"BUFFERED"}},"autoplay":true,"currentTime":0}}"#,
                        req = req_id,
                        url = url.replace('"', "\\\""),
                        ct = content_type,
                    );
                    if cast_send(&mut tls, "sender-0", &transport_id, MEDIA_NS, &json).is_err() {
                        let _ = event_tx.send(CastEvent::Disconnected);
                        return;
                    }
                }
                Ok(CastCommand::Play) => {
                    if let Some(sid) = media_session_id {
                        req_id += 1;
                        let json = format!(
                            r#"{{"type":"PLAY","mediaSessionId":{sid},"requestId":{req}}}"#,
                            req = req_id
                        );
                        let _ = cast_send(&mut tls, "sender-0", &transport_id, MEDIA_NS, &json);
                    }
                }
                Ok(CastCommand::Pause) => {
                    if let Some(sid) = media_session_id {
                        req_id += 1;
                        let json = format!(
                            r#"{{"type":"PAUSE","mediaSessionId":{sid},"requestId":{req}}}"#,
                            req = req_id
                        );
                        let _ = cast_send(&mut tls, "sender-0", &transport_id, MEDIA_NS, &json);
                    }
                }
                Ok(CastCommand::Seek(pos)) => {
                    if let Some(sid) = media_session_id {
                        req_id += 1;
                        let json = format!(
                            r#"{{"type":"SEEK","mediaSessionId":{sid},"requestId":{req},"currentTime":{pos}}}"#,
                            req = req_id
                        );
                        let _ = cast_send(&mut tls, "sender-0", &transport_id, MEDIA_NS, &json);
                    }
                }
                Ok(CastCommand::Stop) => {
                    req_id += 1;
                    let json = format!(r#"{{"type":"STOP","requestId":{}}}"#, req_id);
                    let _ = cast_send(&mut tls, "sender-0", "receiver-0", RECEIVER_NS, &json);
                    let _ = event_tx.send(CastEvent::Disconnected);
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    req_id += 1;
                    let json = format!(r#"{{"type":"STOP","requestId":{}}}"#, req_id);
                    let _ = cast_send(&mut tls, "sender-0", "receiver-0", RECEIVER_NS, &json);
                    let _ = event_tx.send(CastEvent::Disconnected);
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }

        // Read one message (150ms timeout, error on timeout is normal)
        match cast_read_message(&mut tls) {
            Ok((ns, payload)) => {
                if payload.contains("\"PING\"") || ns == HEARTBEAT_NS {
                    let _ = cast_send(&mut tls, "sender-0", "receiver-0", HEARTBEAT_NS,
                        r#"{"type":"PONG"}"#);
                    continue;
                }

                if ns == MEDIA_NS && payload.contains("\"MEDIA_STATUS\"") {
                    let player_state = json_str(&payload, "playerState").unwrap_or_default();
                    let current_time = json_num(&payload, "currentTime").unwrap_or(0.0);
                    let idle_reason = json_str(&payload, "idleReason");

                    if let Some(sid) = json_num(&payload, "mediaSessionId").map(|n| n as u32) {
                        media_session_id = Some(sid);
                    }

                    match player_state.as_str() {
                        "PLAYING" => {
                            let _ = event_tx.send(CastEvent::Playing { current_time });
                        }
                        "PAUSED" => {
                            let _ = event_tx.send(CastEvent::Paused { current_time });
                        }
                        "IDLE" => match idle_reason.as_deref() {
                            Some("FINISHED") => {
                                let _ = event_tx.send(CastEvent::TrackFinished);
                            }
                            Some("ERROR") => {
                                let _ = event_tx.send(CastEvent::Error("Playback error on device".into()));
                            }
                            _ => {}
                        },
                        "BUFFERING" | "" => {}
                        _ => {}
                    }
                }
            }
            Err(e) => {
                // 150ms timeout is normal — only treat real errors as fatal
                if e.contains("os error 11") || e.contains("WouldBlock") || e.contains("timed out") {
                    continue;
                }
                tracing::warn!("Cast connection lost: {e}");
                let _ = event_tx.send(CastEvent::Disconnected);
                return;
            }
        }
    }
}

fn wait_for_transport(tls: &mut (impl Read + Write)) -> Result<String, String> {
    for _ in 0..40 {
        let (_, payload) = match cast_read_message(tls) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if payload.contains("\"PING\"") {
            let _ = cast_send(tls, "sender-0", "receiver-0", HEARTBEAT_NS, r#"{"type":"PONG"}"#);
            continue;
        }
        if payload.contains("\"RECEIVER_STATUS\"") {
            if let Some(id) = json_str(&payload, "transportId") {
                return Ok(id);
            }
        }
    }
    Err("timed out waiting for Cast session".into())
}

// ── Discovery ─────────────────────────────────────────────────────────────────

pub fn discover_devices() -> Vec<CastDevice> {
    let upnp_handle = std::thread::spawn(|| discover_upnp(Duration::from_secs(3)));
    let cast_handle = std::thread::spawn(|| discover_chromecast(Duration::from_secs(3)));
    let mut devices = Vec::new();
    if let Ok(d) = upnp_handle.join() {
        devices.extend(d);
    }
    if let Ok(d) = cast_handle.join() {
        devices.extend(d);
    }
    devices
}

// ── UPnP / DLNA ──────────────────────────────────────────────────────────────

fn discover_upnp(timeout: Duration) -> Vec<CastDevice> {
    let socket = match UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return vec![],
    };
    socket.set_read_timeout(Some(Duration::from_millis(400))).ok();
    socket.set_multicast_loop_v4(true).ok();

    let msearch = concat!(
        "M-SEARCH * HTTP/1.1\r\n",
        "HOST: 239.255.255.250:1900\r\n",
        "MAN: \"ssdp:discover\"\r\n",
        "MX: 3\r\n",
        "ST: urn:schemas-upnp-org:service:AVTransport:1\r\n",
        "\r\n"
    );
    let multicast: SocketAddr = "239.255.255.250:1900".parse().unwrap();
    socket.send_to(msearch.as_bytes(), multicast).ok();

    let mut devices = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let start = Instant::now();
    let mut buf = [0u8; 4096];

    while start.elapsed() < timeout {
        match socket.recv_from(&mut buf) {
            Ok((len, _)) => {
                let resp = std::str::from_utf8(&buf[..len]).unwrap_or("");
                if let Some(loc) = http_header(resp, "location") {
                    if seen.insert(loc.to_string()) {
                        if let Some(dev) = fetch_upnp_device(loc) {
                            devices.push(dev);
                        }
                    }
                }
            }
            Err(_) => {}
        }
    }
    devices
}

fn http_header<'a>(resp: &'a str, name: &str) -> Option<&'a str> {
    for line in resp.lines() {
        if let Some(colon) = line.find(':') {
            if line[..colon].trim().eq_ignore_ascii_case(name) {
                return Some(line[colon + 1..].trim());
            }
        }
    }
    None
}

fn fetch_upnp_device(location: &str) -> Option<CastDevice> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()
        .ok()?;
    let xml = client.get(location).send().ok()?.text().ok()?;

    let name = xml_text(&xml, "friendlyName")
        .unwrap_or_else(|| "Unknown Device".to_string());
    let av_path = find_av_transport_control_url(&xml)?;

    let parsed: url::Url = location.parse().ok()?;
    let host = parsed.host_str()?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let base = format!("{}://{}:{}", parsed.scheme(), host, port);

    let av_transport_url = if av_path.starts_with("http") {
        av_path
    } else {
        format!("{}/{}", base.trim_end_matches('/'), av_path.trim_start_matches('/'))
    };

    Some(CastDevice {
        id: location.to_string(),
        name,
        kind: CastDeviceKind::UPnP,
        host: base,
        port,
        av_transport_url,
    })
}

fn xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)?;
    let after_tag = xml[start..].find('>')? + start + 1;
    let end = xml[after_tag..].find(&close)? + after_tag;
    let value = xml[after_tag..end].trim();
    if value.is_empty() { None } else { Some(value.to_string()) }
}

fn find_av_transport_control_url(xml: &str) -> Option<String> {
    const AV_TYPE: &str = "urn:schemas-upnp-org:service:AVTransport:1";
    let mut pos = 0;
    while let Some(rel) = xml[pos..].find("<service") {
        let start = pos + rel;
        let end = xml[start..].find("</service>")? + start;
        let block = &xml[start..end];
        if block.contains(AV_TYPE) {
            return xml_text(block, "controlURL");
        }
        pos = end + 1;
    }
    None
}

pub fn upnp_play(device: &CastDevice, stream_url: &str, _title: &str) -> Result<(), String> {
    const SVC: &str = "urn:schemas-upnp-org:service:AVTransport:1";
    let set_body = format!(
        "<InstanceID>0</InstanceID>\
         <CurrentURI>{}</CurrentURI>\
         <CurrentURIMetaData></CurrentURIMetaData>",
        xml_escape(stream_url)
    );
    soap(&device.av_transport_url, SVC, "SetAVTransportURI", &set_body)
        .map_err(|e| format!("SetAVTransportURI: {e}"))?;
    soap(&device.av_transport_url, SVC, "Play", "<InstanceID>0</InstanceID><Speed>1</Speed>")
        .map_err(|e| format!("Play: {e}"))?;
    Ok(())
}

pub fn upnp_stop(device: &CastDevice) -> Result<(), String> {
    const SVC: &str = "urn:schemas-upnp-org:service:AVTransport:1";
    soap(&device.av_transport_url, SVC, "Stop", "<InstanceID>0</InstanceID>")
        .map(|_| ())
        .map_err(|e| format!("Stop: {e}"))
}

fn soap(url: &str, service: &str, action: &str, body: &str) -> Result<String, String> {
    let envelope = format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"
            s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <u:{action} xmlns:u="{service}">
      {body}
    </u:{action}>
  </s:Body>
</s:Envelope>"#
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    client
        .post(url)
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPACTION", format!("\"{}#{action}\"", service))
        .body(envelope)
        .send()
        .map_err(|e| e.to_string())?
        .text()
        .map_err(|e| e.to_string())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ── Chromecast (mDNS discovery) ───────────────────────────────────────────────

fn discover_chromecast(timeout: Duration) -> Vec<CastDevice> {
    use mdns_sd::{ServiceDaemon, ServiceEvent};

    let daemon = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("mDNS daemon failed: {e}");
            return vec![];
        }
    };
    let receiver = match daemon.browse("_googlecast._tcp.local.") {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("mDNS browse failed: {e}");
            let _ = daemon.shutdown();
            return vec![];
        }
    };

    let mut devices = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let start = Instant::now();

    while start.elapsed() < timeout {
        match receiver.recv_timeout(Duration::from_millis(200)) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let id = info.get_fullname().to_string();
                if seen.insert(id.clone()) {
                    let name = info
                        .get_property_val_str("fn")
                        .map(str::to_owned)
                        .unwrap_or_else(|| {
                            info.get_fullname()
                                .trim_end_matches("._googlecast._tcp.local.")
                                .to_string()
                        });
                    let ip = info
                        .get_addresses_v4()
                        .iter()
                        .next()
                        .map(|a| a.to_string())
                        .unwrap_or_default();
                    if !ip.is_empty() {
                        tracing::info!("Found Chromecast: {name} at {ip}:{}", info.get_port());
                        devices.push(CastDevice {
                            id,
                            name,
                            kind: CastDeviceKind::Chromecast,
                            host: ip,
                            port: info.get_port(),
                            av_transport_url: String::new(),
                        });
                    }
                }
            }
            Ok(_) => {}
            Err(_) => {}
        }
    }
    let _ = daemon.shutdown();
    devices
}

// ── Cast protocol helpers ─────────────────────────────────────────────────────

fn cast_send(stream: &mut impl Write, src: &str, dst: &str, ns: &str, payload: &str) -> Result<(), String> {
    let msg = cast_encode(src, dst, ns, payload);
    let mut frame = Vec::with_capacity(4 + msg.len());
    frame.extend_from_slice(&(msg.len() as u32).to_be_bytes());
    frame.extend_from_slice(&msg);
    stream.write_all(&frame).map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())
}

fn cast_read_message(stream: &mut impl Read) -> Result<(String, String), String> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).map_err(|e| e.to_string())?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 1_048_576 {
        return Err("cast message too large".to_string());
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).map_err(|e| e.to_string())?;
    let ns = pb_string_field(&buf, 4).unwrap_or_default();
    let payload = pb_string_field(&buf, 6).unwrap_or_default();
    Ok((ns, payload))
}

fn json_str(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{}\":\"", key);
    let start = json.find(&needle)? + needle.len();
    let end = json[start..].find('"')? + start;
    Some(json[start..end].to_string())
}

fn json_num(json: &str, key: &str) -> Option<f64> {
    let needle = format!("\"{}\":", key);
    let start = json.find(&needle)? + needle.len();
    let rest = json[start..].trim_start_matches(' ');
    let end = rest.find(|c: char| c != '-' && c != '.' && !c.is_ascii_digit())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

// ── Protobuf encode / decode (CastMessage) ───────────────────────────────────

fn cast_encode(source_id: &str, destination_id: &str, namespace: &str, payload: &str) -> Vec<u8> {
    let mut out = Vec::new();
    pb_varint_field(&mut out, 1, 0);
    pb_bytes_field(&mut out, 2, source_id.as_bytes());
    pb_bytes_field(&mut out, 3, destination_id.as_bytes());
    pb_bytes_field(&mut out, 4, namespace.as_bytes());
    pb_varint_field(&mut out, 5, 0);
    pb_bytes_field(&mut out, 6, payload.as_bytes());
    out
}

fn pb_varint_field(buf: &mut Vec<u8>, field: u32, value: u64) {
    pb_varint(buf, (field << 3) as u64);
    pb_varint(buf, value);
}

fn pb_bytes_field(buf: &mut Vec<u8>, field: u32, value: &[u8]) {
    pb_varint(buf, ((field << 3) | 2) as u64);
    pb_varint(buf, value.len() as u64);
    buf.extend_from_slice(value);
}

fn pb_varint(buf: &mut Vec<u8>, mut v: u64) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            buf.push(b);
            break;
        }
        buf.push(b | 0x80);
    }
}

fn pb_string_field(data: &[u8], target_field: u32) -> Option<String> {
    let mut i = 0;
    while i < data.len() {
        let (tag, n) = pb_varint_decode(data, i)?;
        i += n;
        let field = (tag >> 3) as u32;
        match tag & 7 {
            0 => {
                let (_, n) = pb_varint_decode(data, i)?;
                i += n;
            }
            2 => {
                let (len, n) = pb_varint_decode(data, i)?;
                i += n;
                let len = len as usize;
                if i + len > data.len() {
                    break;
                }
                if field == target_field {
                    return String::from_utf8(data[i..i + len].to_vec()).ok();
                }
                i += len;
            }
            _ => break,
        }
    }
    None
}

fn pb_varint_decode(data: &[u8], start: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0u32;
    let mut i = start;
    loop {
        if i >= data.len() || shift >= 64 {
            return None;
        }
        let b = data[i] as u64;
        i += 1;
        value |= (b & 0x7f) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            break;
        }
    }
    Some((value, i - start))
}
