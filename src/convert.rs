//! Langkah konversi milik DANload (CLAUDE.md Bagian 2.10): mengubah file hasil yt-dlp menjadi
//! WEBM, MOV, AVI, atau MPEG dengan FFmpeg, sambil melaporkan kemajuannya.
//!
//! 1. [`probe`] menjalankan `ffprobe` untuk membaca kodek video/audio dan durasi file sumber.
//! 2. [`plan`] memutuskan per stream: cukup **disalin** (cepat, kualitas sama persis) atau
//!    **diubah ulang** (lambat). Contoh: VP9 + Opus dari YouTube cukup disalin ke WEBM.
//! 3. [`run`] menjalankan FFmpeg dengan `-progress pipe:1`, yang mencetak `out_time_us=` dan
//!    `speed=` sekitar dua kali per detik. Persen = waktu yang sudah diproses ÷ durasi.
//!
//! Modul ini tidak bergantung pada GUI. [`run`] lambat: **jangan dipanggil dari UI thread**.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;

use crate::deps;
use crate::downloader::{CancelHandle, format_duration};
use crate::formats::VideoFormat;
use crate::process;

/// Batas waktu ffprobe membaca satu file.
const PROBE_TIMEOUT: Duration = Duration::from_secs(60);

/// Jumlah baris error terakhir dari FFmpeg yang disimpan untuk pesan gagal.
const KEPT_ERROR_LINES: usize = 20;

/// Filter yang membulatkan lebar dan tinggi ke bilangan genap. Kodek dengan warna 4:2:0
/// (H.264, Xvid, MPEG-2) menolak ukuran ganjil.
const EVEN_SIZE_FILTER: &str = "scale=trunc(iw/2)*2:trunc(ih/2)*2";

/// Batas waktu FFmpeg membuat satu thumbnail.
const THUMBNAIL_TIMEOUT: Duration = Duration::from_secs(30);

/// Ukuran maksimum thumbnail Pustaka (sisi terpanjang), dalam piksel. Kecil supaya hemat RAM
/// saat ditampilkan (CLAUDE.md Bagian 2.6): 320x180 = ±230 KB setelah dibuka.
pub const THUMBNAIL_SIZE: u32 = 320;

/// Info stream video dan audio utama di file sumber.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MediaInfo {
    pub video: Option<StreamInfo>,
    pub audio: Option<StreamInfo>,
    /// Durasi dalam detik, jika diketahui.
    pub duration: Option<f64>,
    /// Bitrate seluruh file (bit per detik), jika diketahui.
    pub bit_rate: Option<u64>,
}

/// Satu stream di file sumber.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamInfo {
    /// Nama kodek versi FFmpeg, mis. `h264`, `vp9`, `av1`, `aac`, `opus`.
    pub codec: String,
    /// Format piksel (khusus video), mis. `yuv420p`.
    pub pixel_format: Option<String>,
    /// Lebar dan tinggi gambar (khusus video), dalam piksel.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Bitrate stream ini (bit per detik), jika dicatat di file.
    pub bit_rate: Option<u64>,
}

/// Membaca info file `file` dengan ffprobe.
pub fn probe(ffprobe: &Path, file: &Path) -> Result<MediaInfo, String> {
    let args = [
        OsString::from("-v"),
        "error".into(),
        "-print_format".into(),
        "json".into(),
        "-show_entries".into(),
        "stream=codec_type,codec_name,pix_fmt,width,height,bit_rate:stream_disposition=attached_pic:format=duration,bit_rate"
            .into(),
        file_arg(file),
    ];
    let output = deps::run_capture(ffprobe, &args, PROBE_TIMEOUT)?;
    parse_probe(&output)
}

/// Membaca keluaran JSON ffprobe.
fn parse_probe(json: &str) -> Result<MediaInfo, String> {
    #[derive(Deserialize)]
    struct Output {
        #[serde(default)]
        streams: Vec<Stream>,
        format: Option<Format>,
    }
    #[derive(Deserialize)]
    struct Stream {
        codec_type: Option<String>,
        codec_name: Option<String>,
        pix_fmt: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        bit_rate: Option<String>,
        disposition: Option<Disposition>,
    }
    #[derive(Deserialize)]
    struct Disposition {
        #[serde(default)]
        attached_pic: u8,
    }
    #[derive(Deserialize)]
    struct Format {
        duration: Option<String>,
        bit_rate: Option<String>,
    }
    // ffprobe menulis angka bitrate sebagai teks, mis. "192000".
    let bits = |text: &Option<String>| {
        text.as_deref()
            .and_then(|text| text.trim().parse::<u64>().ok())
            .filter(|bits| *bits > 0)
    };

    let output: Output = serde_json::from_str(json)
        .map_err(|error| format!("Jawaban ffprobe tidak bisa dibaca: {error}"))?;
    let first = |wanted: &str| {
        output
            .streams
            .iter()
            // Gambar sampul (attached picture) bukan video sungguhan.
            .filter(|stream| {
                stream
                    .disposition
                    .as_ref()
                    .is_none_or(|d| d.attached_pic == 0)
            })
            .find(|stream| stream.codec_type.as_deref() == Some(wanted))
            .and_then(|stream| {
                Some(StreamInfo {
                    codec: stream.codec_name.clone()?,
                    pixel_format: stream.pix_fmt.clone(),
                    width: stream.width.filter(|width| *width > 0),
                    height: stream.height.filter(|height| *height > 0),
                    bit_rate: bits(&stream.bit_rate),
                })
            })
    };
    Ok(MediaInfo {
        video: first("video"),
        audio: first("audio"),
        duration: output
            .format
            .as_ref()
            .and_then(|format| format.duration.as_deref())
            .and_then(|text| text.trim().parse::<f64>().ok())
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0),
        bit_rate: output
            .format
            .as_ref()
            .and_then(|format| bits(&format.bit_rate)),
    })
}

/// Membuat thumbnail kecil (JPEG, sisi terpanjang [`THUMBNAIL_SIZE`] piksel) dari gambar
/// `source` (JPEG/PNG/WebP dari platform) ke `target`. Gambar yang lebih kecil tidak diperbesar.
pub fn make_thumbnail(ffmpeg: &Path, source: &Path, target: &Path) -> Result<(), String> {
    let size = THUMBNAIL_SIZE;
    let args = [
        OsString::from("-hide_banner"),
        "-nostdin".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
        "-i".into(),
        file_arg(source),
        "-vf".into(),
        format!("scale=w='min({size},iw)':h='min({size},ih)':force_original_aspect_ratio=decrease")
            .into(),
        "-frames:v".into(),
        "1".into(),
        "-q:v".into(),
        "4".into(),
        file_arg(target),
    ];
    deps::run_capture(ffmpeg, &args, THUMBNAIL_TIMEOUT)?;
    if target.is_file() {
        Ok(())
    } else {
        Err("FFmpeg tidak membuat thumbnail.".to_string())
    }
}

/// Yang dilakukan pada satu stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Disalin apa adanya (cepat, kualitas sama persis).
    Copy,
    /// Diubah ulang ke kodek format tujuan (lambat).
    Encode,
}

/// Rencana konversi satu file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub target: VideoFormat,
    pub video: Action,
    pub audio: Action,
}

/// Menyusun rencana konversi ke `target`. `info` = `None` jika file sumber tidak bisa
/// dibaca ffprobe; demi aman, semua stream lalu diubah ulang.
pub fn plan(target: VideoFormat, info: Option<&MediaInfo>) -> Plan {
    let Some(info) = info else {
        return Plan {
            target,
            video: Action::Encode,
            audio: Action::Encode,
        };
    };
    let video = info.video.as_ref();
    let video_codec = video.map(|stream| stream.codec.as_str());
    let pixel_format = video.and_then(|stream| stream.pixel_format.as_deref());
    // Tanpa stream audio tidak ada yang perlu diubah.
    let audio_codec = info.audio.as_ref().map(|stream| stream.codec.as_str());
    let audio_ok = |accepted: &[&str]| audio_codec.is_none_or(|codec| accepted.contains(&codec));
    let pick = |ok: bool| if ok { Action::Copy } else { Action::Encode };

    let (video, audio) = match target {
        VideoFormat::Webm => (
            pick(matches!(video_codec, Some("vp8" | "vp9" | "av1"))),
            pick(audio_ok(&["opus", "vorbis"])),
        ),
        // Hanya H.264 berwarna 4:2:0 yang pasti bisa diputar perangkat Apple.
        VideoFormat::Mov => (
            pick(
                video_codec == Some("h264")
                    && matches!(pixel_format, None | Some("yuv420p" | "yuvj420p")),
            ),
            pick(audio_ok(&["aac"])),
        ),
        VideoFormat::Avi => (Action::Encode, pick(audio_ok(&["mp3"]))),
        VideoFormat::Mpeg => (Action::Encode, pick(audio_ok(&["mp2"]))),
        // Tidak lewat langkah ini (dibuat langsung oleh yt-dlp), tetapi tetap aman: salin saja.
        VideoFormat::Mp4 | VideoFormat::Mkv => (Action::Copy, Action::Copy),
    };
    Plan {
        target,
        video,
        audio,
    }
}

impl Plan {
    /// `true` jika tidak ada yang diubah ulang (hanya dipindah ke wadah baru, cepat).
    pub fn copy_only(&self) -> bool {
        self.video == Action::Copy && self.audio == Action::Copy
    }

    /// Argumen FFmpeg untuk mengubah `input` menjadi `output`.
    pub fn ffmpeg_args(&self, input: &Path, output: &Path) -> Vec<OsString> {
        let mut args: Vec<OsString> = [
            "-hide_banner",
            "-nostdin",
            "-loglevel",
            "error",
            "-nostats",
            // Laporan kemajuan yang mudah dibaca program, dikirim ke stdout.
            "-progress",
            "pipe:1",
            "-y",
            "-i",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        args.push(file_arg(input));

        // Video utama (bukan gambar sampul) wajib ada; audio boleh tidak ada (`?`).
        let mut options: Vec<&str> = vec!["-map", "0:V:0", "-map", "0:a:0?"];
        options.extend(match self.video {
            Action::Copy => vec!["-c:v", "copy"],
            Action::Encode => self.video_encoder(),
        });
        options.extend(match self.audio {
            Action::Copy => vec!["-c:a", "copy"],
            Action::Encode => self.audio_encoder(),
        });
        // Mencegah error "Too many packets buffered" saat satu stream disalin dan lainnya diubah.
        options.extend(["-max_muxing_queue_size", "4096"]);
        if self.target == VideoFormat::Mpeg {
            // MPEG-2 Program Stream (bukan MPEG-1 System bawaan ekstensi .mpg).
            options.extend(["-f", "vob"]);
        }
        args.extend(options.into_iter().map(OsString::from));
        args.push(file_arg(output));
        args
    }

    fn video_encoder(&self) -> Vec<&'static str> {
        match self.target {
            // Mode `realtime` + `cpu-used 6`: sekitar 5x lebih cepat daripada `good` dengan
            // kualitas hampir sama (SSIM 0,982 vs 0,983 pada uji 1080p), file sekitar 20% lebih besar.
            VideoFormat::Webm => vec![
                "-c:v",
                "libvpx-vp9",
                "-crf",
                "31",
                "-b:v",
                "0",
                "-deadline",
                "realtime",
                "-cpu-used",
                "6",
                "-row-mt",
                "1",
                "-pix_fmt",
                "yuv420p",
            ],
            VideoFormat::Mov => vec![
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "20",
                "-pix_fmt",
                "yuv420p",
                "-vf",
                EVEN_SIZE_FILTER,
            ],
            // Encoder MPEG-4 bawaan FFmpeg (selalu ada), ditandai XVID supaya dikenali pemutar lama.
            VideoFormat::Avi => vec![
                "-c:v",
                "mpeg4",
                "-vtag",
                "XVID",
                "-q:v",
                "3",
                "-pix_fmt",
                "yuv420p",
                "-vf",
                EVEN_SIZE_FILTER,
            ],
            // `maxrate` + `bufsize` hanya mengisi ukuran buffer VBV, supaya muxer tidak memakai
            // buffer kecil bawaan (230 KB) yang memicu peringatan.
            VideoFormat::Mpeg => vec![
                "-c:v",
                "mpeg2video",
                "-q:v",
                "3",
                "-bf",
                "2",
                "-maxrate",
                "40M",
                "-bufsize",
                "9781k",
                "-pix_fmt",
                "yuv420p",
                "-vf",
                EVEN_SIZE_FILTER,
            ],
            VideoFormat::Mp4 | VideoFormat::Mkv => vec!["-c:v", "copy"],
        }
    }

    fn audio_encoder(&self) -> Vec<&'static str> {
        match self.target {
            VideoFormat::Webm => vec!["-c:a", "libopus", "-b:a", "160k"],
            VideoFormat::Mov => vec!["-c:a", "aac", "-b:a", "192k"],
            // MP3 dan MP2 hanya mendukung paling banyak 2 kanal (stereo).
            VideoFormat::Avi => vec!["-c:a", "libmp3lame", "-b:a", "192k", "-ac", "2"],
            VideoFormat::Mpeg => vec!["-c:a", "mp2", "-b:a", "224k", "-ac", "2"],
            VideoFormat::Mp4 | VideoFormat::Mkv => vec!["-c:a", "copy"],
        }
    }
}

/// Path untuk FFmpeg/ffprobe dengan awalan `file:`, seperti yang dilakukan yt-dlp. Tanpa awalan,
/// nama file yang kebetulan mirip protokol (mis. `concat:...`) bisa dibaca sebagai protokol.
fn file_arg(path: &Path) -> OsString {
    let mut arg = OsString::from("file:");
    arg.push(path.as_os_str());
    arg
}

/// Kemajuan konversi untuk ditampilkan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Progress {
    pub target: VideoFormat,
    /// `true` jika semua stream hanya disalin.
    pub copy_only: bool,
    /// Detik media yang sudah diproses.
    pub processed: f64,
    /// Durasi media dalam detik, jika diketahui.
    pub duration: Option<f64>,
    /// Kecepatan dibanding waktu nyata, mis. `2.0` = dua kali lebih cepat dari durasinya.
    pub speed: Option<f64>,
}

/// Teks kemajuan, mis. `Mengubah ke AVI · 45% · sisa 1:20`, dan nilai 0..1 untuk batang kemajuan.
pub fn describe(progress: &Progress) -> (String, f32) {
    let name = progress.target.name();
    let mut pieces = vec![if progress.copy_only {
        format!("Menyimpan sebagai {name}")
    } else {
        format!("Mengubah ke {name}")
    }];
    let fraction = match progress.duration.filter(|duration| *duration > 0.0) {
        Some(duration) => {
            let fraction = (progress.processed / duration).clamp(0.0, 1.0);
            pieces.push(format!("{}%", (fraction * 100.0).floor() as u64));
            if let Some(speed) = progress.speed.filter(|speed| *speed > 0.0)
                && fraction < 1.0
            {
                let remaining = ((duration - progress.processed).max(0.0) / speed).ceil();
                pieces.push(format!("sisa {}", format_duration(remaining as u64)));
            }
            fraction as f32
        }
        None => {
            pieces.push(format!(
                "{} diproses",
                format_duration(progress.processed as u64)
            ));
            0.0
        }
    };
    (pieces.join(" · "), fraction)
}

/// Kenapa konversi berhenti.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConvertError {
    Cancelled,
    /// Pesannya siap ditampilkan ke pengguna.
    Failed(String),
}

/// Satu baris keluaran FFmpeg.
enum Line {
    /// stdout: laporan `-progress`.
    Progress(String),
    /// stderr: pesan error.
    Error(String),
}

/// Menjalankan FFmpeg dengan `args` (dari [`Plan::ffmpeg_args`]) sampai selesai.
///
/// `on_progress(detik_diproses, kecepatan)` dipanggil setiap ada laporan (sekitar 2 kali per detik).
pub fn run(
    ffmpeg: &Path,
    args: &[OsString],
    target: VideoFormat,
    cancel: &CancelHandle,
    on_progress: &mut dyn FnMut(f64, Option<f64>),
) -> Result<(), ConvertError> {
    let mut command = Command::new(ffmpeg);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process::run_in_background(&mut command);
    let mut child = command.spawn().map_err(|error| {
        ConvertError::Failed(format!(
            "FFmpeg tidak bisa dijalankan: {error}. Coba pasang ulang lewat Pengaturan."
        ))
    })?;
    if cancel.set_running(child.id()) {
        process::kill_tree(child.id());
    }

    let (sender, receiver) = mpsc::channel::<Line>();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(process::spawn_line_reader(
            stdout,
            sender.clone(),
            Line::Progress,
        ));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(process::spawn_line_reader(
            stderr,
            sender.clone(),
            Line::Error,
        ));
    }
    drop(sender); // Channel tertutup sendiri setelah kedua pembaca selesai.

    let mut parser = ProgressParser::default();
    let mut errors: Vec<String> = Vec::new();
    for line in receiver {
        match line {
            Line::Progress(line) => {
                if let Some(sample) = parser.feed(&line) {
                    on_progress(sample.processed, sample.speed);
                }
            }
            Line::Error(line) => {
                let line = line.trim();
                if !line.is_empty() {
                    if errors.len() == KEPT_ERROR_LINES {
                        errors.remove(0);
                    }
                    errors.push(line.to_string());
                }
            }
        }
    }
    for reader in readers {
        let _ = reader.join();
    }
    let status = child.wait();
    cancel.clear_running();

    if cancel.is_cancelled() {
        return Err(ConvertError::Cancelled);
    }
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(ConvertError::Failed(friendly_error(
            &errors,
            target,
            status.code(),
        ))),
        Err(error) => Err(ConvertError::Failed(format!(
            "Gagal menunggu FFmpeg selesai: {error}"
        ))),
    }
}

/// Satu laporan lengkap dari `-progress`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Sample {
    processed: f64,
    speed: Option<f64>,
}

/// Mengumpulkan baris `kunci=nilai` dari `-progress`. Satu laporan diakhiri baris `progress=`.
#[derive(Debug, Default)]
struct ProgressParser {
    processed_us: Option<u64>,
    speed: Option<f64>,
}

impl ProgressParser {
    fn feed(&mut self, line: &str) -> Option<Sample> {
        let (key, value) = line.split_once('=')?;
        let value = value.trim();
        match key.trim() {
            // Keduanya dalam mikrodetik (nama `out_time_ms` keliru sejak dulu di FFmpeg).
            "out_time_us" | "out_time_ms" => {
                if let Ok(micros) = value.parse::<u64>() {
                    self.processed_us = Some(micros);
                }
            }
            "speed" => {
                self.speed = value
                    .trim_end_matches('x')
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|speed| speed.is_finite() && *speed > 0.0);
            }
            "progress" => {
                return Some(Sample {
                    processed: self.processed_us.unwrap_or(0) as f64 / 1_000_000.0,
                    speed: self.speed,
                });
            }
            _ => {}
        }
        None
    }
}

/// Mengubah pesan error FFmpeg menjadi pesan yang mudah dipahami.
fn friendly_error(lines: &[String], target: VideoFormat, exit_code: Option<i32>) -> String {
    let name = target.name();
    let all = lines.join("\n").to_lowercase();
    let has = |needles: &[&str]| needles.iter().any(|needle| all.contains(needle));
    if has(&["unknown encoder", "encoder not found"]) {
        return format!(
            "FFmpeg yang terpasang tidak punya encoder untuk {name}. Pasang ulang FFmpeg lewat Pengaturan → Pasang / perbarui..."
        );
    }
    if has(&["no space left", "not enough space"]) {
        return format!("Ruang disk tidak cukup untuk mengubah ke {name}.");
    }
    if has(&["permission denied", "access is denied"]) {
        return "DANload tidak punya izin menulis ke folder unduhan. Pilih folder lain di Pengaturan."
            .to_string();
    }
    match lines.last() {
        Some(line) => {
            let detail: String = line.chars().take(220).collect();
            format!("Gagal mengubah ke {name}: {detail}")
        }
        None => match exit_code {
            Some(code) => format!("Gagal mengubah ke {name} (FFmpeg berhenti dengan kode {code})."),
            None => format!("Gagal mengubah ke {name} (FFmpeg berhenti tanpa pesan)."),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn info(video: Option<(&str, &str)>, audio: Option<&str>) -> MediaInfo {
        MediaInfo {
            video: video.map(|(codec, pixel_format)| StreamInfo {
                codec: codec.into(),
                pixel_format: Some(pixel_format.into()),
                ..StreamInfo::default()
            }),
            audio: audio.map(|codec| StreamInfo {
                codec: codec.into(),
                ..StreamInfo::default()
            }),
            duration: Some(10.0),
            bit_rate: None,
        }
    }

    #[test]
    fn parses_ffprobe_json() {
        // Keluaran asli ffprobe 6.1 untuk file MKV hasil gabungan yt-dlp, ditambah gambar sampul.
        let json = r#"{
            "programs": [],
            "streams": [
                { "codec_name": "mjpeg", "codec_type": "video", "pix_fmt": "yuvj420p",
                  "disposition": { "attached_pic": 1 } },
                { "codec_name": "vp9", "codec_type": "video", "pix_fmt": "yuv420p",
                  "disposition": { "attached_pic": 0 } },
                { "codec_name": "opus", "codec_type": "audio", "disposition": { "attached_pic": 0 } }
            ],
            "format": { "duration": "212.091000" }
        }"#;
        let info = parse_probe(json).unwrap();
        assert_eq!(info.video.unwrap().codec, "vp9");
        assert_eq!(info.audio.unwrap().codec, "opus");
        assert_eq!(info.duration, Some(212.091));

        // Ukuran gambar dan bitrate (dipakai label kualitas di Pustaka).
        let json = r#"{
            "streams": [
                { "codec_name": "h264", "codec_type": "video", "width": 1080, "height": 1920,
                  "bit_rate": "2500000", "disposition": { "attached_pic": 0 } },
                { "codec_name": "mp3", "codec_type": "audio", "bit_rate": "192000" }
            ],
            "format": { "duration": "12.5", "bit_rate": "N/A" }
        }"#;
        let info = parse_probe(json).unwrap();
        let video = info.video.unwrap();
        assert_eq!((video.width, video.height), (Some(1080), Some(1920)));
        assert_eq!(video.bit_rate, Some(2_500_000));
        assert_eq!(info.audio.unwrap().bit_rate, Some(192_000));
        assert_eq!(info.bit_rate, None);

        let empty = parse_probe(r#"{ "streams": [] }"#).unwrap();
        assert_eq!(empty, MediaInfo::default());
        assert!(parse_probe("bukan json").is_err());
    }

    #[test]
    fn webm_copies_youtube_streams_and_encodes_the_rest() {
        let youtube = plan(
            VideoFormat::Webm,
            Some(&info(Some(("vp9", "yuv420p")), Some("opus"))),
        );
        assert!(youtube.copy_only());
        let av1 = plan(
            VideoFormat::Webm,
            Some(&info(Some(("av1", "yuv420p")), Some("aac"))),
        );
        assert_eq!((av1.video, av1.audio), (Action::Copy, Action::Encode));
        let h264 = plan(
            VideoFormat::Webm,
            Some(&info(Some(("h264", "yuv420p")), Some("aac"))),
        );
        assert_eq!((h264.video, h264.audio), (Action::Encode, Action::Encode));
    }

    #[test]
    fn mov_copies_only_compatible_h264() {
        let ok = plan(
            VideoFormat::Mov,
            Some(&info(Some(("h264", "yuv420p")), Some("aac"))),
        );
        assert!(ok.copy_only());
        // H.264 4:4:4 tidak didukung perangkat Apple: diubah ulang.
        let high444 = plan(
            VideoFormat::Mov,
            Some(&info(Some(("h264", "yuv444p")), Some("aac"))),
        );
        assert_eq!(high444.video, Action::Encode);
        let vp9 = plan(
            VideoFormat::Mov,
            Some(&info(Some(("vp9", "yuv420p")), Some("opus"))),
        );
        assert_eq!((vp9.video, vp9.audio), (Action::Encode, Action::Encode));
    }

    #[test]
    fn avi_and_mpeg_always_encode_video() {
        let source = info(Some(("h264", "yuv420p")), Some("mp3"));
        let avi = plan(VideoFormat::Avi, Some(&source));
        assert_eq!((avi.video, avi.audio), (Action::Encode, Action::Copy));
        let mpeg = plan(VideoFormat::Mpeg, Some(&source));
        assert_eq!((mpeg.video, mpeg.audio), (Action::Encode, Action::Encode));
        // Tanpa audio: tidak ada yang perlu diubah di sisi audio.
        let silent = plan(
            VideoFormat::Mpeg,
            Some(&info(Some(("h264", "yuv420p")), None)),
        );
        assert_eq!(silent.audio, Action::Copy);
        // ffprobe gagal: semua diubah ulang demi aman.
        let unknown = plan(VideoFormat::Webm, None);
        assert_eq!(
            (unknown.video, unknown.audio),
            (Action::Encode, Action::Encode)
        );
    }

    #[test]
    fn ffmpeg_args_are_complete_and_safe() {
        let plan = Plan {
            target: VideoFormat::Mpeg,
            video: Action::Encode,
            audio: Action::Encode,
        };
        let args: Vec<String> = plan
            .ffmpeg_args(
                Path::new("/tmp/job/hasil/concat: judul [id].mkv"),
                Path::new("/tmp/job/concat: judul [id].mpg"),
            )
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|pair| pair == ["-progress", "pipe:1"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-i", "file:/tmp/job/hasil/concat: judul [id].mkv"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "mpeg2video"]));
        assert!(args.windows(2).any(|pair| pair == ["-c:a", "mp2"]));
        assert!(args.windows(2).any(|pair| pair == ["-f", "vob"]));
        assert_eq!(args.last().unwrap(), "file:/tmp/job/concat: judul [id].mpg");

        let copy = Plan {
            target: VideoFormat::Webm,
            video: Action::Copy,
            audio: Action::Copy,
        };
        let args: Vec<String> = copy
            .ffmpeg_args(&PathBuf::from("a.mkv"), &PathBuf::from("a.webm"))
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "copy"]));
        assert!(args.windows(2).any(|pair| pair == ["-c:a", "copy"]));
        assert!(!args.contains(&"-vf".to_string()));
        assert!(!args.contains(&"vob".to_string()));
    }

    #[test]
    fn parses_progress_reports() {
        // Potongan keluaran asli `ffmpeg -progress pipe:1` (versi 6.1).
        let lines = [
            "frame=0",
            "out_time_us=N/A",
            "out_time_ms=N/A",
            "speed=N/A",
            "progress=continue",
            "frame=94",
            "out_time_us=3133333",
            "out_time_ms=3133333",
            "out_time=00:00:03.133333",
            "speed= 6.1x",
            "progress=continue",
            "out_time_us=6024813",
            "speed=5.84x",
            "progress=end",
        ];
        let mut parser = ProgressParser::default();
        let samples: Vec<Sample> = lines.iter().filter_map(|line| parser.feed(line)).collect();
        assert_eq!(samples.len(), 3);
        assert_eq!(
            samples[0],
            Sample {
                processed: 0.0,
                speed: None
            }
        );
        assert!((samples[1].processed - 3.133333).abs() < 1e-9);
        assert_eq!(samples[1].speed, Some(6.1));
        assert!((samples[2].processed - 6.024813).abs() < 1e-9);
    }

    #[test]
    fn progress_text() {
        let progress = Progress {
            target: VideoFormat::Avi,
            copy_only: false,
            processed: 45.0,
            duration: Some(100.0),
            speed: Some(0.5),
        };
        let (text, fraction) = describe(&progress);
        assert_eq!(text, "Mengubah ke AVI · 45% · sisa 1:50");
        assert!((fraction - 0.45).abs() < 1e-6);

        let copy = Progress {
            target: VideoFormat::Webm,
            copy_only: true,
            processed: 100.0,
            duration: Some(100.0),
            speed: Some(40.0),
        };
        assert_eq!(describe(&copy).0, "Menyimpan sebagai WEBM · 100%");

        let unknown = Progress {
            duration: None,
            processed: 75.0,
            ..progress
        };
        assert_eq!(
            describe(&unknown),
            ("Mengubah ke AVI · 1:15 diproses".into(), 0.0)
        );
    }

    #[test]
    fn friendly_errors() {
        let encoder = vec!["Unknown encoder 'libmp3lame'".to_string()];
        assert!(friendly_error(&encoder, VideoFormat::Avi, Some(1)).contains("encoder untuk AVI"));
        let disk = vec![
            "[vob @ 0x1] Error writing trailer".to_string(),
            "av_interleaved_write_frame(): No space left on device".to_string(),
        ];
        assert!(friendly_error(&disk, VideoFormat::Mpeg, Some(1)).contains("Ruang disk"));
        let other = vec!["Sesuatu yang aneh".to_string()];
        assert_eq!(
            friendly_error(&other, VideoFormat::Mov, Some(1)),
            "Gagal mengubah ke MOV: Sesuatu yang aneh"
        );
        assert!(friendly_error(&[], VideoFormat::Webm, Some(69)).contains("kode 69"));
    }
}
