//! One sequential download per preview.
//!
//! The library's remote path fetches one latency-adaptive range at a time,
//! which pays a full round trip per chunk. That starves playback on a
//! high-latency link. A preview instead keeps a single GET open and writes
//! into the [`ChunkedCache`] as bytes arrive, at link speed, until the end of
//! the file. Decode reads block on ranges that have not arrived yet.
//!
//! A read outside the downloaded area (a seek past the frontier) asks the
//! downloader to restart from that offset with `Range: bytes=offset-`. Data
//! already on disk is not fetched twice for long: a restarted stream stops
//! when it runs into a large cached run and resumes at the next gap.

use crate::audio::{chunked_cache::ChunkedCache, remote_source::FetchEvent};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};
use symphonia::core::io::MediaSource;

#[derive(Clone)]
pub(super) enum PreviewMedia {
    Host {
        host: crate::source_host::SourceHost,
        source_id: String,
        descriptor: crate::source_host::ResourceDescriptor,
        cancellation: crate::source_host::Cancellation,
    },
    #[cfg(test)]
    Http(String),
    #[cfg(test)]
    Expired,
}

pub(super) enum MediaResponse {
    Host(crate::source_host::MediaResponse),
    #[cfg(test)]
    Http(reqwest::blocking::Response),
}

#[derive(Debug)]
pub(super) struct MediaOpenError {
    pub message: String,
    expired: bool,
}

impl From<String> for MediaOpenError {
    fn from(message: String) -> Self {
        Self {
            message,
            expired: false,
        }
    }
}

impl PreviewMedia {
    pub(super) fn open(
        &self,
        start: u64,
        end: Option<u64>,
        alive: &Alive,
    ) -> Result<MediaResponse, MediaOpenError> {
        match self {
            Self::Host {
                host,
                source_id,
                descriptor,
                cancellation,
            } => tauri::async_runtime::block_on(async {
                tokio::select! {
                    result = host.open_media(source_id, descriptor, Some((start, end)), cancellation.clone()) => result.map(MediaResponse::Host).map_err(|error| MediaOpenError {
                        expired: error.code == "media_expired", message: error.message,
                    }),
                    _ = wait_stopped(alive) => Err("preview stopped".to_owned().into()),
                }
            }),
            #[cfg(test)]
            Self::Http(url) => reqwest::blocking::Client::builder()
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(READ_TIMEOUT)
                .build()
                .map_err(|error| error.to_string())?
                .get(url)
                .header(
                    "Range",
                    format!(
                        "bytes={start}-{}",
                        end.map(|n| n.to_string()).unwrap_or_default()
                    ),
                )
                .send()
                .map(MediaResponse::Http)
                .map_err(|error| error.to_string().into()),
            #[cfg(test)]
            Self::Expired => Err(MediaOpenError {
                message: "media descriptor expired".to_owned(),
                expired: true,
            }),
        }
    }
}

impl MediaResponse {
    pub(super) fn status(&self) -> u16 {
        match self {
            Self::Host(response) => response.status,
            #[cfg(test)]
            Self::Http(response) => response.status().as_u16(),
        }
    }

    pub(super) fn content_range(&self) -> Option<&str> {
        match self {
            Self::Host(response) => response.content_range.as_deref(),
            #[cfg(test)]
            Self::Http(response) => response.headers().get("content-range")?.to_str().ok(),
        }
    }

    fn chunk(&mut self, alive: &Alive) -> Result<Option<Vec<u8>>, String> {
        match self {
            Self::Host(response) => tauri::async_runtime::block_on(async {
                tokio::select! {
                    result = response.chunk() => result.map_err(|error| error.message),
                    _ = wait_stopped(alive) => Err("preview stopped".to_owned()),
                    _ = tokio::time::sleep(Duration::from_secs(30)) => Err("preview read timed out".to_owned()),
                }
            }),
            #[cfg(test)]
            Self::Http(response) => {
                let mut bytes = vec![0; CHUNK];
                let read = response
                    .read(&mut bytes)
                    .map_err(|error| error.to_string())?;
                bytes.truncate(read);
                Ok((read > 0).then_some(bytes))
            }
        }
    }
}

async fn wait_stopped(alive: &Alive) {
    while alive() {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

#[cfg(test)]
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
/// Idle timeout per read: a stalled connection fails instead of hanging.
#[cfg(test)]
const READ_TIMEOUT: Duration = Duration::from_secs(30);
#[cfg(test)]
const CHUNK: usize = 32 * 1024;
/// A wanted offset this close ahead of the write position is reached by the
/// running stream sooner than a new request would answer.
const CONTINUE_SLACK: u64 = 512 * 1024;
/// A restarted stream stops when this much is already cached ahead of it.
const SKIP_CACHED_RUN: u64 = 256 * 1024;
const MAX_FAILURES: u32 = 3;

/// True while the owning request is still wanted.
pub(super) type Alive = Arc<dyn Fn() -> bool + Send + Sync>;

/// Handle for the running downloader: send the offset a reader needs next.
pub(super) struct Download {
    pub wanted: mpsc::Sender<u64>,
    pub events: mpsc::Receiver<FetchEvent>,
    pub handle: std::thread::JoinHandle<()>,
}

/// Start downloading `url` into `cache`. The thread ends when every sender of
/// `wanted` is dropped, `alive` turns false, or a fatal event was reported.
pub(super) fn spawn(media: PreviewMedia, cache: Arc<ChunkedCache>, alive: Alive) -> Download {
    let (wanted, wanted_rx) = mpsc::channel();
    let (event_tx, events) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        Downloader {
            media,
            cache,
            wanted_rx,
            event_tx,
            alive,
            want: 0,
            failures: 0,
        }
        .run();
    });
    Download {
        wanted,
        events,
        handle,
    }
}

struct Downloader {
    media: PreviewMedia,
    cache: Arc<ChunkedCache>,
    wanted_rx: mpsc::Receiver<u64>,
    event_tx: mpsc::Sender<FetchEvent>,
    alive: Alive,
    /// Where the reader needs data next.
    want: u64,
    failures: u32,
}

enum StreamEnd {
    /// The whole body arrived or the stream reached cached data.
    Finished,
    /// The reader asked for an offset this stream will not reach soon.
    Restart,
    /// Stop the thread (superseded, disconnected, or an event was sent).
    Stop,
    /// A transient failure; retry from the write position.
    Retry,
}

impl Downloader {
    fn run(mut self) {
        let size = self.cache.file_size();
        loop {
            if !(self.alive)() {
                return;
            }
            let Some(start) = next_gap(&self.cache, self.want, size) else {
                // Everything is on disk; wait for the senders to go away.
                match self.wanted_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(offset) => {
                        self.want = offset;
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            };
            match self.stream_from(start, size) {
                StreamEnd::Stop => return,
                StreamEnd::Restart | StreamEnd::Finished => {}
                StreamEnd::Retry => {
                    self.failures += 1;
                    if self.failures >= MAX_FAILURES {
                        let _ = self.event_tx.send(FetchEvent::ConsecutiveFailures {
                            count: self.failures,
                        });
                        return;
                    }
                    if !self.pause(Duration::from_millis(500 * u64::from(self.failures))) {
                        return;
                    }
                }
            }
        }
    }

    /// Sleep while staying responsive; `false` means stop the thread.
    fn pause(&mut self, total: Duration) -> bool {
        let step = Duration::from_millis(50);
        let mut slept = Duration::ZERO;
        while slept < total {
            if !(self.alive)() {
                return false;
            }
            match self.wanted_rx.try_recv() {
                Ok(offset) => self.want = offset,
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => return false,
            }
            std::thread::sleep(step);
            slept += step;
        }
        true
    }

    fn stream_from(&mut self, start: u64, size: u64) -> StreamEnd {
        let mut response = match self.media.open(start, None, &self.alive) {
            Ok(response) => response,
            Err(error) if error.expired => {
                let _ = self.event_tx.send(FetchEvent::UrlExpired);
                return StreamEnd::Stop;
            }
            Err(_) => return StreamEnd::Retry,
        };
        match response.status() {
            206 => {}
            200 if start == 0 => {}
            200 | 416 => {
                let _ = self.event_tx.send(FetchEvent::RangeNotSupported);
                return StreamEnd::Stop;
            }
            403 | 410 => {
                let _ = self.event_tx.send(FetchEvent::UrlExpired);
                return StreamEnd::Stop;
            }
            _ => return StreamEnd::Retry,
        }

        let mut position = start;
        loop {
            if !(self.alive)() {
                return StreamEnd::Stop;
            }
            let mut latest = None;
            loop {
                match self.wanted_rx.try_recv() {
                    Ok(offset) => latest = Some(offset),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return StreamEnd::Stop,
                }
            }
            if let Some(offset) = latest {
                self.want = offset;
                let reachable = offset >= position && offset - position <= CONTINUE_SLACK;
                if !self.cache.is_cached(offset, 1) && !reachable {
                    return StreamEnd::Restart;
                }
            }
            if position >= size {
                self.want = size;
                return StreamEnd::Finished;
            }
            match response.chunk(&self.alive) {
                Ok(None) => {
                    // The body ended before the advertised size.
                    self.want = position;
                    return StreamEnd::Retry;
                }
                Ok(Some(bytes)) => {
                    let read = bytes.len().min((size - position) as usize);
                    if self.cache.write_at(position, &bytes[..read]).is_err() {
                        let _ = self
                            .event_tx
                            .send(FetchEvent::ConsecutiveFailures { count: 1 });
                        return StreamEnd::Stop;
                    }
                    position += read as u64;
                    self.failures = 0;
                    if position < size
                        && self.cache.cached_length_from(position, SKIP_CACHED_RUN)
                            >= SKIP_CACHED_RUN
                    {
                        self.want = position;
                        return StreamEnd::Finished;
                    }
                }
                Err(_) => {
                    self.want = position;
                    return StreamEnd::Retry;
                }
            }
        }
    }
}

/// First byte at or after `from` that is not on disk; wraps to the start of
/// the file, and is `None` once everything is cached.
fn next_gap(cache: &ChunkedCache, from: u64, size: u64) -> Option<u64> {
    if from < size {
        let end = from + cache.cached_length_from(from, size - from);
        if end < size {
            return Some(end);
        }
    }
    let head = cache.cached_length_from(0, size);
    (head < size).then_some(head)
}

/// Reads the preview's cache. Bytes that have not arrived yet block until
/// the downloader writes them; a read past the frontier asks it to restart
/// there.
pub(super) struct PreviewSource {
    cache: Arc<ChunkedCache>,
    position: u64,
    wanted: mpsc::Sender<u64>,
    alive: Alive,
    stop_on_drop: Option<Arc<std::sync::atomic::AtomicBool>>,
}

impl PreviewSource {
    #[cfg(test)]
    pub(super) fn new(cache: Arc<ChunkedCache>, wanted: mpsc::Sender<u64>) -> Self {
        Self::with_alive(cache, wanted, Arc::new(|| true))
    }

    pub(super) fn with_alive(
        cache: Arc<ChunkedCache>,
        wanted: mpsc::Sender<u64>,
        alive: Alive,
    ) -> Self {
        Self {
            cache,
            position: 0,
            wanted,
            alive,
            stop_on_drop: None,
        }
    }

    pub(super) fn stop_on_drop(mut self, stop: Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.stop_on_drop = Some(stop);
        self
    }
}

impl Drop for PreviewSource {
    fn drop(&mut self) {
        if let Some(stop) = &self.stop_on_drop {
            stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

impl Read for PreviewSource {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let size = self.cache.file_size();
        if buf.is_empty() || self.position >= size {
            return Ok(0);
        }
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.cache.cached_length_from(self.position, 1) == 0 {
            if !(self.alive)() {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "preview stopped",
                ));
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "preview buffer timed out",
                ));
            }
            let _ = self.wanted.send(self.position);
            std::thread::sleep(Duration::from_millis(25));
        }
        let length = self
            .cache
            .cached_length_from(self.position, (buf.len() as u64).min(size - self.position))
            as usize;
        let read = self
            .cache
            .read_at(self.position, &mut buf[..length])
            .map_err(|error| io::Error::other(format!("cache read error: {error}")))?;
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for PreviewSource {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let size = self.cache.file_size();
        self.position = match pos {
            SeekFrom::Start(offset) => offset,
            SeekFrom::End(offset) if offset <= 0 => size.saturating_sub(offset.unsigned_abs()),
            SeekFrom::End(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "seek from end with positive offset",
                ))
            }
            SeekFrom::Current(offset) if offset >= 0 => self.position.saturating_add(offset as u64),
            SeekFrom::Current(offset) => self.position.saturating_sub(offset.unsigned_abs()),
        };
        Ok(self.position)
    }
}

impl MediaSource for PreviewSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.cache.file_size())
    }
}

/// Bytes worth about three seconds of this file, at least 256 KiB, capped at
/// the file size. Playback starts once that much is on disk.
pub(super) fn startup_bytes(file_size: u64, duration_ms: Option<u64>) -> u64 {
    const FLOOR: u64 = 256 * 1024;
    let three_seconds = match duration_ms {
        Some(ms) if ms > 0 => {
            (u128::from(file_size) * 3000 / u128::from(ms)).min(u128::from(u64::MAX)) as u64
        }
        _ => 0,
    };
    FLOOR.max(three_seconds).min(file_size)
}

/// Block until `bytes` from the start of the file are cached, the preview is
/// superseded, or `timeout` passes. A downloader event ends the wait early
/// and is returned.
pub(super) fn wait_buffered(
    cache: &ChunkedCache,
    bytes: u64,
    alive: &Alive,
    events: &mpsc::Receiver<FetchEvent>,
    timeout: Duration,
) -> Result<(), FetchEvent> {
    let deadline = Instant::now() + timeout;
    while cache.cached_length_from(0, bytes) < bytes {
        if !alive() || Instant::now() >= deadline {
            return Ok(());
        }
        match events.try_recv() {
            Ok(event) => return Err(event),
            Err(_) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn body(len: usize) -> Vec<u8> {
        (0..len).map(|index| (index % 251) as u8).collect()
    }

    struct Link {
        /// Delay before the response head, like a long round trip.
        rtt: Duration,
        /// Bytes written per tick and the tick length: the link speed.
        tick_bytes: usize,
        tick: Duration,
        /// Answer every request with this status instead of the body.
        status: Option<u16>,
    }

    struct Server {
        url: String,
        starts: Arc<Mutex<Vec<usize>>>,
    }

    /// A range-capable loopback server with latency and a throttled link.
    fn serve(body: Vec<u8>, link: Link) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let starts = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&starts);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let body = body.clone();
                let recorded = Arc::clone(&recorded);
                let link = Link { ..link };
                std::thread::spawn(move || {
                    let mut request = Vec::new();
                    let mut byte = [0u8; 1];
                    while !request.ends_with(b"\r\n\r\n") {
                        match io::Read::read(&mut stream, &mut byte) {
                            Ok(1) => request.push(byte[0]),
                            _ => return,
                        }
                    }
                    let text = String::from_utf8_lossy(&request).to_ascii_lowercase();
                    let start = text
                        .lines()
                        .find_map(|line| line.strip_prefix("range: bytes="))
                        .and_then(|value| value.split('-').next()?.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    recorded.lock().expect("starts").push(start);
                    std::thread::sleep(link.rtt);
                    if let Some(status) = link.status {
                        let _ = stream.write_all(
                            format!(
                                "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                            )
                            .as_bytes(),
                        );
                        return;
                    }
                    let part = &body[start..];
                    let head = format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len() - 1,
                        body.len(),
                        part.len()
                    );
                    if stream.write_all(head.as_bytes()).is_err() {
                        return;
                    }
                    // Pace against a fixed schedule. A relative sleep per tick
                    // lets every oversleep accumulate, and a loaded host with
                    // coalesced timers then runs the link far below its rate.
                    let sending = Instant::now();
                    for (index, chunk) in part.chunks(link.tick_bytes).enumerate() {
                        if stream.write_all(chunk).is_err() {
                            return;
                        }
                        let due = sending + link.tick * (index as u32 + 1);
                        if let Some(wait) = due.checked_duration_since(Instant::now()) {
                            std::thread::sleep(wait);
                        }
                    }
                });
            }
        });
        Server {
            url: format!("http://{address}/track.mp3"),
            starts,
        }
    }

    fn open(dir: &std::path::Path, size: usize) -> Arc<ChunkedCache> {
        Arc::new(ChunkedCache::open(dir, "t", size as u64).expect("cache"))
    }

    fn always() -> Alive {
        Arc::new(|| true)
    }

    #[test]
    fn an_expired_descriptor_requests_link_refresh_without_transfer_retries() {
        let dir = tempfile::tempdir().expect("dir");
        let download = spawn(PreviewMedia::Expired, open(dir.path(), 1024), always());
        let event = download
            .events
            .recv_timeout(Duration::from_secs(1))
            .expect("expiry event");
        assert!(matches!(event, FetchEvent::UrlExpired));
        download.handle.join().expect("expired transfer stops");
        assert!(download.events.try_recv().is_err());
    }

    #[test]
    fn stopping_a_preview_interrupts_a_blocked_decoder_read() {
        let dir = tempfile::tempdir().expect("dir");
        let cache = open(dir.path(), 1024);
        let (wanted, _receiver) = mpsc::channel();
        let live = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&live);
        let mut source =
            PreviewSource::with_alive(cache, wanted, Arc::new(move || flag.load(Ordering::SeqCst)));
        let reader = std::thread::spawn(move || source.read(&mut [0; 16]));
        live.store(false, Ordering::SeqCst);
        assert_eq!(
            reader.join().expect("reader").unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
    }

    #[test]
    fn startup_bytes_covers_three_seconds_with_a_floor() {
        // 320 kbps for 200 s is 8 MB: three seconds is 120 KB, below the floor.
        assert_eq!(startup_bytes(8_000_000, Some(200_000)), 256 * 1024);
        // 2 MB/s of audio needs 6 MB.
        assert_eq!(startup_bytes(60_000_000, Some(30_000)), 6_000_000);
        assert_eq!(startup_bytes(100_000, Some(30_000)), 100_000);
        assert_eq!(startup_bytes(8_000_000, None), 256 * 1024);
    }

    /// A 320 kbps stream behind a 150 ms round trip. Time is compressed 4x
    /// (consumption at 160 KB/s, link at 640 KB/s) so the test runs in seconds;
    /// the round trip is not compressed, which is harsher than real time. A
    /// per-chunk fetcher pays 150 ms for every chunk and cannot keep up; one
    /// sequential download must never leave a read waiting: the bytes of every
    /// read are on disk before the reader asks for them.
    #[test]
    fn a_high_latency_320kbps_stream_never_starves_the_reader() {
        let data = body(1_000_000);
        let server = serve(
            data.clone(),
            Link {
                rtt: Duration::from_millis(150),
                tick_bytes: 6_400,
                tick: Duration::from_millis(10),
                status: None,
            },
        );
        let dir = tempfile::tempdir().expect("dir");
        let cache = open(dir.path(), data.len());
        let download = spawn(
            PreviewMedia::Http(server.url.clone()),
            Arc::clone(&cache),
            always(),
        );
        let mut source = PreviewSource::new(Arc::clone(&cache), download.wanted.clone());

        let startup = startup_bytes(data.len() as u64, Some(25_000));
        wait_buffered(
            &cache,
            startup,
            &always(),
            &download.events,
            Duration::from_secs(10),
        )
        .expect("buffered");
        assert_eq!(
            cache.cached_length_from(0, startup),
            startup,
            "startup bytes never arrived"
        );

        let mut buffer = [0u8; 8_000];
        let mut offset = 0usize;
        let began = Instant::now();
        while offset < data.len() {
            let paced = began + Duration::from_millis(50) * (offset / 8_000) as u32;
            if let Some(wait) = paced.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            let wanted = (data.len() - offset).min(buffer.len());
            assert_eq!(
                cache.cached_length_from(offset as u64, wanted as u64),
                wanted as u64,
                "the reader caught up with the download at byte {offset}"
            );
            let read = io::Read::read(&mut source, &mut buffer).expect("read");
            assert_eq!(read, wanted);
            assert_eq!(&buffer[..read], &data[offset..offset + read]);
            offset += read;
        }
        assert_eq!(server.starts.lock().expect("starts").len(), 1);
    }

    #[test]
    fn a_seek_past_the_frontier_restarts_the_stream_there() {
        let data = body(1_500_000);
        let server = serve(
            data.clone(),
            Link {
                rtt: Duration::from_millis(100),
                tick_bytes: 6_400,
                tick: Duration::from_millis(10),
                status: None,
            },
        );
        let dir = tempfile::tempdir().expect("dir");
        let cache = open(dir.path(), data.len());
        let download = spawn(
            PreviewMedia::Http(server.url.clone()),
            Arc::clone(&cache),
            always(),
        );
        let mut source = PreviewSource::new(Arc::clone(&cache), download.wanted.clone());
        wait_buffered(
            &cache,
            256 * 1024,
            &always(),
            &download.events,
            Duration::from_secs(10),
        )
        .expect("buffered");

        // Far ahead of the roughly 300 KB downloaded so far.
        io::Seek::seek(&mut source, SeekFrom::Start(1_200_000)).expect("seek");
        let mut buffer = [0u8; 4_000];
        let asked = Instant::now();
        let read = io::Read::read(&mut source, &mut buffer).expect("read");
        let waited = asked.elapsed();
        assert_eq!(&buffer[..read], &data[1_200_000..1_200_000 + read]);
        assert!(
            waited < Duration::from_millis(600),
            "the seek took {waited:?}"
        );
        assert!(server.starts.lock().expect("starts").contains(&1_200_000));

        // Already downloaded data answers without waiting on the network.
        io::Seek::seek(&mut source, SeekFrom::Start(10_000)).expect("seek back");
        let asked = Instant::now();
        let read = io::Read::read(&mut source, &mut buffer).expect("read");
        assert!(asked.elapsed() < Duration::from_millis(50));
        assert_eq!(&buffer[..read], &data[10_000..10_000 + read]);
    }

    #[test]
    fn the_whole_file_arrives_even_after_a_seek_left_a_gap() {
        let data = body(600_000);
        let server = serve(
            data.clone(),
            Link {
                rtt: Duration::from_millis(10),
                tick_bytes: 32_000,
                tick: Duration::from_millis(5),
                status: None,
            },
        );
        let dir = tempfile::tempdir().expect("dir");
        let cache = open(dir.path(), data.len());
        let download = spawn(
            PreviewMedia::Http(server.url.clone()),
            Arc::clone(&cache),
            always(),
        );
        let mut source = PreviewSource::new(Arc::clone(&cache), download.wanted.clone());
        io::Seek::seek(&mut source, SeekFrom::Start(550_000)).expect("seek");
        let mut buffer = [0u8; 100];
        io::Read::read(&mut source, &mut buffer).expect("read");

        let deadline = Instant::now() + Duration::from_secs(10);
        while !cache.is_complete() {
            assert!(Instant::now() < deadline, "the file never completed");
            std::thread::sleep(Duration::from_millis(20));
        }
        let mut all = vec![0u8; data.len()];
        cache.read_at(0, &mut all).expect("read all");
        assert!(all == data);
    }

    #[test]
    fn a_forbidden_link_is_reported_as_expired() {
        let data = body(10_000);
        let server = serve(
            data.clone(),
            Link {
                rtt: Duration::ZERO,
                tick_bytes: 1_000,
                tick: Duration::ZERO,
                status: Some(403),
            },
        );
        let dir = tempfile::tempdir().expect("dir");
        let cache = open(dir.path(), data.len());
        let download = spawn(PreviewMedia::Http(server.url.clone()), cache, always());
        let event = download
            .events
            .recv_timeout(Duration::from_secs(5))
            .expect("an event");
        assert!(matches!(event, FetchEvent::UrlExpired));
    }

    #[test]
    fn the_downloader_stops_when_the_request_is_superseded() {
        let data = body(2_000_000);
        let server = serve(
            data.clone(),
            Link {
                rtt: Duration::ZERO,
                tick_bytes: 1_000,
                tick: Duration::from_millis(5),
                status: None,
            },
        );
        let dir = tempfile::tempdir().expect("dir");
        let cache = open(dir.path(), data.len());
        let live = Arc::new(AtomicBool::new(true));
        let flag = Arc::clone(&live);
        let _download = spawn(
            PreviewMedia::Http(server.url.clone()),
            Arc::clone(&cache),
            Arc::new(move || flag.load(Ordering::SeqCst)),
        );
        std::thread::sleep(Duration::from_millis(200));
        live.store(false, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(200));
        let frozen = AtomicUsize::new(cache.cached_length_from(0, data.len() as u64) as usize);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            cache.cached_length_from(0, data.len() as u64) as usize,
            frozen.load(Ordering::SeqCst)
        );
    }
}
