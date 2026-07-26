use anyhow::{Context, Result};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex,
    },
    thread,
    time::Duration,
};
use tiny_http::{Header, Method, Response, Server, StatusCode};

#[derive(Debug, Clone, PartialEq)]
pub struct AirPlayAudioChunk {
    pub epoch: u64,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

#[derive(Debug)]
pub struct AirPlayAudioTap {
    epoch: AtomicU64,
    buffer: Mutex<AirPlayAudioBuffer>,
}

#[derive(Debug)]
struct AirPlayAudioBuffer {
    slots: Vec<Option<AirPlayAudioChunk>>,
    head: usize,
    len: usize,
}

impl AirPlayAudioTap {
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            epoch: AtomicU64::new(1),
            buffer: Mutex::new(AirPlayAudioBuffer::new(capacity)),
        }
    }

    pub fn current_epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }

    pub fn bump_epoch(&self) -> u64 {
        self.epoch.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Takes ownership of `samples` to avoid heap allocation on the realtime
    /// audio callback thread. Caller passes its pre-allocated scratch buffer and
    /// replaces it with a fresh Vec for the next callback.
    pub fn push_interleaved(&self, sample_rate: u32, channels: u16, samples: Vec<f32>) {
        if samples.is_empty() {
            return;
        }

        // Use try_lock() instead of blocking lock() on the realtime audio
        // callback thread. If drain_pending holds the lock, dropping samples is
        // preferable to blocking the callback and causing audible glitches.
        let Ok(mut buffer) = self.buffer.try_lock() else {
            return;
        };

        buffer.push(AirPlayAudioChunk {
            epoch: self.current_epoch(),
            sample_rate,
            channels,
            samples,
        });
    }

    pub fn drain_pending(&self) -> Vec<AirPlayAudioChunk> {
        let Ok(mut buffer) = self.buffer.lock() else {
            return Vec::new();
        };

        buffer.drain()
    }
}

impl AirPlayAudioBuffer {
    fn new(capacity: usize) -> Self {
        Self {
            slots: vec![None; capacity.max(1)],
            head: 0,
            len: 0,
        }
    }

    fn capacity(&self) -> usize {
        self.slots.len()
    }

    fn push(&mut self, chunk: AirPlayAudioChunk) {
        let capacity = self.capacity();
        let insert_index = (self.head + self.len) % capacity;

        if self.len == capacity {
            self.slots[self.head] = Some(chunk);
            self.head = (self.head + 1) % capacity;
            return;
        }

        self.slots[insert_index] = Some(chunk);
        self.len += 1;
    }

    fn drain(&mut self) -> Vec<AirPlayAudioChunk> {
        let mut drained = Vec::with_capacity(self.len);
        for offset in 0..self.len {
            let index = (self.head + offset) % self.capacity();
            if let Some(chunk) = self.slots[index].take() {
                drained.push(chunk);
            }
        }
        self.head = 0;
        self.len = 0;
        drained
    }
}

pub fn select_forwardable_audio_chunks(
    current_epoch: u64,
    chunks: Vec<AirPlayAudioChunk>,
) -> (u64, Vec<AirPlayAudioChunk>) {
    // `epoch` is only a freshness boundary. It invalidates old PCM
    // after play/pause/seek/track changes, but it does not mean "forward only
    // the first chunk from that epoch". AirPlay must continue streaming every
    // subsequent chunk in the newest epoch or TV audio will fall silent after
    // the first short burst.
    let next_epoch = chunks
        .iter()
        .map(|chunk| chunk.epoch)
        .max()
        .map(|epoch| epoch.max(current_epoch))
        .unwrap_or(current_epoch);

    let forwardable = chunks
        .into_iter()
        .filter(|chunk| chunk.epoch == next_epoch)
        .collect();

    (next_epoch, forwardable)
}

#[derive(Debug)]
pub struct AirPlayHttpServer {
    root_dir: PathBuf,
    base_url: String,
    _thread: thread::JoinHandle<()>,
}

impl AirPlayHttpServer {
    pub fn bind(root_dir: &Path) -> Result<Self> {
        let published_ip = detect_airplay_publish_ip()?;
        Self::bind_with_publish_ip(root_dir, published_ip)
    }

    pub fn bind_with_publish_ip(root_dir: &Path, published_ip: Ipv4Addr) -> Result<Self> {
        fs::create_dir_all(root_dir)
            .with_context(|| format!("failed to create airplay root dir {}", root_dir.display()))?;

        // AirPlay receivers may fetch the HLS URL directly, so loopback URLs are not valid here.
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))
            .context("failed to bind airplay http server")?;
        let address = listener
            .local_addr()
            .context("failed to read airplay http server address")?;
        listener
            .set_nonblocking(false)
            .context("failed to configure airplay listener")?;

        let server = Server::from_listener(listener, None)
            .map_err(|error| anyhow::anyhow!("failed to start tiny_http server: {error}"))?;
        let root_dir = root_dir.to_path_buf();
        let server_root = root_dir.clone();

        let thread = thread::spawn(move || loop {
            let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(100)) else {
                continue;
            };
            if request.method() != &Method::Get && request.method() != &Method::Head {
                let _ = request.respond(Response::empty(StatusCode(405)));
                continue;
            }

            let Some(path) = sanitize_request_path(request.url()) else {
                let _ = request.respond(Response::empty(StatusCode(400)));
                continue;
            };
            let file_path = server_root.join(path);
            let range_header = request
                .headers()
                .iter()
                .find(|header| header.field.equiv("Range"))
                .map(|header| header.value.as_str().to_owned());

            match build_file_response(
                request.method() == &Method::Head,
                range_header.as_deref(),
                &file_path,
            ) {
                Ok(response) => {
                    let _ = request.respond(response);
                }
                Err(_) => {
                    let _ = request.respond(Response::empty(StatusCode(404)));
                }
            }
        });

        let base_url = format!("http://{}:{}", published_ip, address.port());
        eprintln!(
            "OpenKara AirPlay HLS publishing on {} (serving {})",
            base_url,
            root_dir.display()
        );

        Ok(Self {
            root_dir,
            base_url,
            _thread: thread,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }
}

fn sanitize_request_path(url: &str) -> Option<PathBuf> {
    let trimmed = url.split('?').next()?.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some(PathBuf::from("playlist.m3u8"));
    }
    if trimmed
        .split('/')
        .any(|segment| segment == ".." || segment.is_empty())
    {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn build_file_response(
    head_only: bool,
    range_header: Option<&str>,
    file_path: &Path,
) -> Result<Response<std::io::Cursor<Vec<u8>>>> {
    let content_type = match file_path
        .extension()
        .and_then(|extension| extension.to_str())
    {
        Some("m3u8") => "application/vnd.apple.mpegurl",
        Some("mp4") => "video/mp4",
        Some("m4s") => "video/iso.segment",
        _ => "application/octet-stream",
    };

    // Determine file size from metadata rather than reading the whole file.
    let metadata = fs::metadata(file_path)
        .with_context(|| format!("failed to stat requested asset {}", file_path.display()))?;
    let total_len = metadata.len();

    let requested_range = range_header.and_then(|header| parse_byte_range(header, total_len));

    // For Range requests, seek to the start and read only the requested
    // byte range instead of reading the entire file into memory.
    let (status, response_body, content_range) = if let Some(range) = requested_range {
        let mut file = fs::File::open(file_path)
            .with_context(|| format!("failed to open requested asset {}", file_path.display()))?;
        file.seek(SeekFrom::Start(range.start)).with_context(|| {
            format!(
                "failed to seek to byte {} in {}",
                range.start,
                file_path.display()
            )
        })?;
        let read_len = (range.end - range.start + 1) as usize;
        let mut partial = vec![0u8; read_len];
        file.read_exact(&mut partial).with_context(|| {
            format!(
                "failed to read bytes {}-{} from {}",
                range.start,
                range.end,
                file_path.display()
            )
        })?;
        (
            StatusCode(206),
            partial,
            Some(format!("bytes {}-{}/{}", range.start, range.end, total_len)),
        )
    } else {
        let mut file = fs::File::open(file_path)
            .with_context(|| format!("failed to open requested asset {}", file_path.display()))?;
        let mut body = Vec::new();
        file.read_to_end(&mut body)
            .with_context(|| format!("failed to read requested asset {}", file_path.display()))?;
        (StatusCode(200), body, None)
    };

    let mut response = Response::from_data(response_body).with_status_code(status);
    response.add_header(
        Header::from_bytes("Content-Type", content_type.as_bytes())
            .expect("static ASCII header name is valid"),
    );
    response.add_header(
        Header::from_bytes("Accept-Ranges", "bytes").expect("static ASCII header name is valid"),
    );
    let _ = head_only; // HEAD responses share the same headers as GET
    if let Some(content_range) = content_range {
        response.add_header(
            Header::from_bytes("Content-Range", content_range.as_bytes())
                .expect("Content-Range value is ASCII"),
        );
    }

    Ok(response)
}

pub fn default_stream_root(root: &Path) -> PathBuf {
    root.join("airplay").join("live")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Arc, thread, time::Duration};

    /// push_interleaved must use try_lock() on the audio callback thread so
    /// that contention with drain_pending drops samples instead of blocking.
    /// Blocking the realtime callback causes audible glitches.
    #[test]
    fn push_interleaved_drops_samples_when_lock_is_contended() {
        let tap = Arc::new(AirPlayAudioTap::new(4));
        let guard = tap.buffer.lock().expect("test should hold the queue lock");
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let worker_tap = Arc::clone(&tap);

        let worker = thread::spawn(move || {
            ready_tx.send(()).expect("worker should signal readiness");
            worker_tap.push_interleaved(44_100, 2, vec![0.25, 0.5]);
        });

        ready_rx.recv().expect("worker should start");
        thread::sleep(Duration::from_millis(25));

        assert!(
            worker.is_finished(),
            "push_interleaved should return immediately (try_lock) when lock is contended"
        );

        drop(guard);
        worker.join().expect("worker should have finished");

        let drained = tap.drain_pending();
        assert!(
            drained.is_empty(),
            "samples should be dropped when lock is contended, got {} chunks",
            drained.len()
        );
    }

    /// push_interleaved successfully enqueues when the lock is available.
    #[test]
    fn push_interleaved_enqueues_when_lock_available() {
        let tap = AirPlayAudioTap::new(4);
        tap.push_interleaved(44_100, 2, vec![0.25, 0.5]);

        let drained = tap.drain_pending();
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0].samples, vec![0.25, 0.5]);
        assert_eq!(drained[0].sample_rate, 44_100);
        assert_eq!(drained[0].channels, 2);
    }

    /// Range requests must read only the requested byte range from disk,
    /// not the entire file.
    #[test]
    fn range_request_reads_only_requested_bytes() {
        let dir = std::env::temp_dir().join(format!("airplay_r11_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let file_path = dir.join("test.bin");
        let data: Vec<u8> = (0..=255).cycle().take(1000).collect();
        fs::write(&file_path, &data).unwrap();

        let response =
            super::build_file_response(false, Some("bytes=100-199"), &file_path).unwrap();

        let mut reader = response.into_reader();
        let mut body = Vec::new();
        reader.read_to_end(&mut body).unwrap();
        assert_eq!(
            body.len(),
            100,
            "Range response should contain exactly 100 bytes"
        );
        assert_eq!(
            body,
            &data[100..200],
            "Range response should match the file slice"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Full (non-Range) requests should still return the complete file.
    #[test]
    fn full_request_returns_entire_file() {
        let dir = std::env::temp_dir().join(format!("airplay_r11_full_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let file_path = dir.join("test.bin");
        let data: Vec<u8> = (0..=255).cycle().take(500).collect();
        fs::write(&file_path, &data).unwrap();

        let response = super::build_file_response(false, None, &file_path).unwrap();
        let mut reader = response.into_reader();
        let mut body = Vec::new();
        reader.read_to_end(&mut body).unwrap();
        assert_eq!(body.len(), 500, "Full response should contain entire file");

        let _ = fs::remove_dir_all(&dir);
    }
}

pub fn stream_tick_interval() -> Duration {
    Duration::from_millis(33)
}

fn detect_airplay_publish_ip() -> Result<Ipv4Addr> {
    let candidates = collect_publish_ip_candidates()?;
    pick_publish_ip(&candidates).ok_or_else(|| {
        anyhow::anyhow!("failed to determine a non-loopback ipv4 address for airplay")
    })
}

pub fn spawn_audio_forwarder(tap: std::sync::Arc<AirPlayAudioTap>) {
    #[cfg(target_os = "macos")]
    {
        thread::spawn(move || {
            let mut current_epoch = 0;

            loop {
                let chunks = tap.drain_pending();
                if chunks.is_empty() {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }

                let (next_epoch, forwardable) =
                    select_forwardable_audio_chunks(current_epoch, chunks);
                current_epoch = next_epoch;

                for chunk in forwardable {
                    // SAFETY: the bridge copies `len` samples out of the pointer
                    // before it returns, and `chunk` outlives the call, so the
                    // slice stays alive and unaliased for the whole call.
                    unsafe {
                        ok_airplay_push_audio_samples(
                            chunk.samples.as_ptr(),
                            chunk.samples.len(),
                            chunk.sample_rate,
                            chunk.channels,
                            chunk.epoch,
                        );
                    }
                }
            }
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = tap;
    }
}

pub fn notify_audio_epoch(epoch: u64) {
    // SAFETY: passes a plain integer to the bridge, which stores it under its
    // own lock. No pointers cross the boundary.
    #[cfg(target_os = "macos")]
    unsafe {
        ok_airplay_set_audio_epoch(epoch);
    }

    #[cfg(not(target_os = "macos"))]
    let _ = epoch;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ByteRange {
    start: u64,
    end: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishIpCandidate {
    name: String,
    ip: Ipv4Addr,
}

fn parse_byte_range(header: &str, total_len: u64) -> Option<ByteRange> {
    let value = header.strip_prefix("bytes=")?;
    let (start, end) = value.split_once('-')?;

    if total_len == 0 {
        return None;
    }

    if start.is_empty() {
        let suffix_len = end.parse::<u64>().ok()?;
        if suffix_len == 0 {
            return None;
        }
        let suffix_len = suffix_len.min(total_len);
        return Some(ByteRange {
            start: total_len - suffix_len,
            end: total_len - 1,
        });
    }

    let start = start.parse::<u64>().ok()?;
    let end = if end.is_empty() {
        total_len - 1
    } else {
        end.parse::<u64>().ok()?.min(total_len - 1)
    };

    if start > end || start >= total_len {
        return None;
    }

    Some(ByteRange { start, end })
}

#[cfg(unix)]
fn collect_publish_ip_candidates() -> Result<Vec<PublishIpCandidate>> {
    use std::{ffi::CStr, net::Ipv4Addr, ptr};

    let mut addresses = ptr::null_mut();
    // SAFETY: libc fills a linked list owned by the OS until freeifaddrs is called below.
    let result = unsafe { libc::getifaddrs(&mut addresses) };
    if result != 0 {
        return Err(anyhow::anyhow!(
            "failed to enumerate local network interfaces"
        ));
    }

    // SAFETY: getifaddrs succeeded (result == 0), so `addresses` is a valid
    // linked-list head (or null for an empty list). Each node's `ifa_next`
    // is either a valid next node or null. The list remains valid until
    // freeifaddrs is called at the end of this function.
    struct IfAddrsGuard(*mut libc::ifaddrs);
    impl Drop for IfAddrsGuard {
        fn drop(&mut self) {
            // SAFETY: the guard is only constructed from a pointer a successful
            // getifaddrs wrote, and Drop runs once, so this frees that list
            // exactly once.
            unsafe { libc::freeifaddrs(self.0) };
        }
    }
    let _guard = IfAddrsGuard(addresses);

    let mut candidates = Vec::new();
    let mut cursor = addresses;

    while let Some(entry) = ptr::NonNull::new(cursor) {
        // SAFETY: NonNull::new returned Some, so entry is non-null and valid.
        let entry = unsafe { entry.as_ref() };
        let addr = entry.ifa_addr;
        if !addr.is_null() {
            // SAFETY: getifaddrs guarantees `ifa_name` is a NUL-terminated C
            // string owned by the list, which outlives this borrow.
            let name = unsafe { CStr::from_ptr(entry.ifa_name) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: `addr` was null-checked above and points at a sockaddr
            // owned by the list; `sa_family` is present in every sockaddr
            // variant, so it is readable regardless of the concrete family.
            let family = unsafe { (*addr).sa_family as i32 };
            let flags = entry.ifa_flags as i32;
            let is_up = flags & libc::IFF_UP != 0;
            let is_running = flags & libc::IFF_RUNNING != 0;
            let is_loopback = flags & libc::IFF_LOOPBACK != 0;
            let is_point_to_point = flags & libc::IFF_POINTOPOINT != 0;

            if family == libc::AF_INET && is_up && is_running && !is_loopback && !is_point_to_point
            {
                // SAFETY: reached only when `sa_family == AF_INET`, which is
                // exactly the tag that makes this sockaddr a sockaddr_in.
                let socket_addr = unsafe { &*(addr as *const libc::sockaddr_in) };
                let ip = Ipv4Addr::from(u32::from_be(socket_addr.sin_addr.s_addr));

                if is_eligible_publish_ip(&name, ip) {
                    candidates.push(PublishIpCandidate { name, ip });
                }
            }
        }

        cursor = entry.ifa_next;
    }

    Ok(candidates)
}

#[cfg(not(unix))]
fn collect_publish_ip_candidates() -> Result<Vec<PublishIpCandidate>> {
    Ok(Vec::new())
}

fn pick_publish_ip(candidates: &[PublishIpCandidate]) -> Option<Ipv4Addr> {
    let mut ranked: Vec<_> = candidates
        .iter()
        .filter_map(|candidate| {
            rank_publish_ip_candidate(candidate).map(|rank| (rank, candidate.ip))
        })
        .collect();
    ranked.sort_by_key(|(rank, _)| *rank);
    ranked.first().map(|(_, ip)| *ip)
}

fn rank_publish_ip_candidate(candidate: &PublishIpCandidate) -> Option<(u8, u32)> {
    let name = candidate.name.as_str();

    if is_virtual_interface(name) {
        return None;
    }

    if let Some(index) = interface_index(name, "en") {
        // macOS exposes built-in Wi‑Fi as en0 on the common hardware path.
        // Preferring it ahead of other active en* interfaces keeps the AirPlay
        // publish address stable on laptops that also have docks/adapters.
        return Some(if index == 0 { (0, index) } else { (1, index) });
    }

    if let Some(index) = interface_index(name, "wlan")
        .or_else(|| interface_index(name, "wifi"))
        .or_else(|| interface_index(name, "wl"))
    {
        return Some((0, index));
    }

    if let Some(index) = interface_index(name, "eth") {
        return Some((1, index));
    }

    Some((2, u32::MAX))
}

fn interface_index(name: &str, prefix: &str) -> Option<u32> {
    let suffix = name.strip_prefix(prefix)?;
    suffix.parse::<u32>().ok()
}

fn is_virtual_interface(name: &str) -> bool {
    const VIRTUAL_PREFIXES: &[&str] = &[
        "lo",
        "utun",
        "awdl",
        "llw",
        "bridge",
        "ap",
        "p2p",
        "gif",
        "stf",
        "anpi",
        "vnic",
        "vmnet",
        "vboxnet",
        "docker",
        "tailscale",
        "tap",
        "tun",
        "wg",
    ];

    VIRTUAL_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

#[cfg(unix)]
fn is_eligible_publish_ip(name: &str, ip: Ipv4Addr) -> bool {
    if ip.is_loopback() || ip.is_unspecified() || is_virtual_interface(name) {
        return false;
    }

    let octets = ip.octets();
    !(octets[0] == 169 && octets[1] == 254)
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn ok_airplay_push_audio_samples(
        samples: *const f32,
        sample_count: usize,
        sample_rate: u32,
        channels: u16,
        epoch: u64,
    );
    fn ok_airplay_set_audio_epoch(epoch: u64);
}
