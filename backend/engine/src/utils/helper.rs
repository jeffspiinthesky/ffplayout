use ffmpeg_next::{Dictionary, Error as FfmpegError, format};

/// Timeout for blocking network I/O (`rw_timeout` is in microseconds). Without
/// it a stalled TCP connection blocks the playout worker indefinitely; the
/// skip/abort flags are only checked between packets and never reach a thread
/// that is stuck inside a single read or write syscall.
const NETWORK_IO_TIMEOUT_US: &str = "10000000";

pub fn even(value: u32) -> u32 {
    value & !1
}

pub(crate) fn is_network_url(path: &str) -> bool {
    path.split_once("://")
        .is_some_and(|(scheme, _)| !scheme.eq_ignore_ascii_case("file"))
}

pub(crate) fn network_io_options() -> Dictionary<'static> {
    let mut options = Dictionary::new();
    options.set("rw_timeout", NETWORK_IO_TIMEOUT_US);
    options
}

fn is_udp_url(path: &str) -> bool {
    path.split_once("://")
        .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("udp"))
}

/// Options for opening a network input. FFmpeg's UDP protocol ignores
/// `rw_timeout` while its receive thread waits for packets, so a silent UDP
/// source would block `avformat_open_input` forever; its own `timeout` option
/// bounds that wait. Only set it for UDP: other protocols give `timeout` a
/// different meaning (for RTMP it enables listen mode).
fn network_input_options(path: &str) -> Dictionary<'static> {
    let mut options = network_io_options();
    if is_udp_url(path) {
        options.set("timeout", NETWORK_IO_TIMEOUT_US);
    }
    options
}

/// Opens a media input, applying a read timeout for network sources so a
/// stalled remote server cannot hang the playout thread forever.
pub(crate) fn open_media_input(path: &str) -> Result<format::context::Input, FfmpegError> {
    if is_network_url(path) {
        format::input_with_dictionary(&path, network_input_options(path))
    } else {
        format::input(&path)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::UdpSocket,
        time::{Duration, Instant},
    };

    use super::{is_network_url, is_udp_url, network_input_options, open_media_input};

    #[test]
    fn classifies_network_urls() {
        assert!(is_network_url("rtmp://example.com/live/stream"));
        assert!(is_network_url("http://example.com/video.mp4"));
        assert!(!is_network_url("/var/lib/media/clip.mp4"));
        assert!(!is_network_url("file:///var/lib/media/clip.mp4"));
        assert!(!is_network_url("clip.mp4"));
    }

    #[test]
    fn sets_udp_timeout_only_for_udp_inputs() {
        assert!(is_udp_url("udp://127.0.0.1:5001"));
        assert!(is_udp_url("UDP://239.1.1.2:5001?pkt_size=1316"));
        assert!(!is_udp_url("rtmp://example.com/live/stream"));
        assert!(!is_udp_url("/var/lib/media/udp.mp4"));

        assert!(network_input_options("udp://127.0.0.1:5001").get("timeout").is_some());
        assert!(network_input_options("rtmp://example.com/live").get("timeout").is_none());
        assert!(network_input_options("rtmp://example.com/live").get("rw_timeout").is_some());
    }

    #[test]
    fn silent_udp_input_times_out() {
        // Reserve a free port, then release it so FFmpeg can bind it; nothing
        // ever sends to it.
        let port = UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();

        let started = Instant::now();
        let result = open_media_input(&format!("udp://127.0.0.1:{port}"));

        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(30));
    }
}
