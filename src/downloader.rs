//! Menjalankan yt-dlp untuk satu unduhan dan membaca kemajuannya (CLAUDE.md Bagian 2.10).
//!
//! Cara kerja:
//! 1. yt-dlp dijalankan dengan `--progress-template` dan `--print`, supaya mencetak baris
//!    khusus berawalan `DANLOAD|` yang mudah dibaca program (bukan teks untuk manusia).
//! 2. Baris dari stdout dan stderr dibaca di dua thread kecil. Progress unduhan keluar di
//!    stdout, sedangkan tahap FFmpeg (gabung, konversi) keluar di stderr.
//! 3. File sementara (`.part`) ditaruh di `.danload-tmp` di dalam folder unduhan, dan
//!    folder itu selalu dihapus setelah unduhan berhasil, gagal, atau dibatalkan.
//!
//! Modul ini tidak bergantung pada GUI. [`run`] lambat: **jangan dipanggil dari UI thread**.

use std::ffi::OsString;
use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use crate::formats::OutputFormat;
use crate::process;

/// Jeda minimal antar laporan progress ke UI (CLAUDE.md Bagian 2.6).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Awalan baris yang dicetak khusus untuk DANload.
const LINE_PREFIX: &str = "DANLOAD|";

/// Nama folder sementara di dalam folder unduhan.
pub const TEMP_DIR_NAME: &str = ".danload-tmp";

/// Template progress unduhan. `NA` dicetak yt-dlp untuk nilai yang tidak diketahui.
const DOWNLOAD_TEMPLATE: &str = "download:DANLOAD|dl|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(info.vcodec)s|%(info.acodec)s";

/// Template progress tahap pemrosesan (FFmpeg dan lainnya).
const POSTPROCESS_TEMPLATE: &str =
    "postprocess:DANLOAD|pp|%(progress.status)s|%(progress.postprocessor)s";

/// Nama file hasil: judul + ID video, seperti bawaan yt-dlp. ID mencegah dua video
/// berjudul sama dianggap file yang sama.
const OUTPUT_TEMPLATE: &str = "%(title)s [%(id)s].%(ext)s";

/// Lokasi program yang dibutuhkan untuk mengunduh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub yt_dlp: PathBuf,
    /// Folder berisi `ffmpeg` dan `ffprobe`.
    pub ffmpeg_dir: PathBuf,
    pub deno: PathBuf,
}

/// Satu permintaan unduhan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub format: OutputFormat,
    /// Folder tujuan (sudah termasuk subfolder Video/Audio jika dipakai).
    pub folder: PathBuf,
}

/// Bagian yang sedang diunduh. YouTube sering menyediakan video dan audio terpisah,
/// yang diunduh satu per satu lalu digabung FFmpeg.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Video,
    Audio,
    Whole,
}

/// Tahap pemrosesan setelah unduhan selesai.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Merging,
    ConvertingAudio,
    Metadata,
    Fixing,
    Moving,
    Other,
}

/// Laporan kemajuan untuk ditampilkan.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// Judul video sudah diketahui.
    Title(String),
    /// Sedang mengunduh. `total` dan nilai lain bisa `None` jika tidak diketahui.
    Downloading {
        part: Part,
        downloaded: u64,
        total: Option<u64>,
        /// Byte per detik.
        speed: Option<f64>,
        /// Perkiraan sisa waktu dalam detik.
        eta: Option<u64>,
    },
    /// Unduhan selesai, sedang diproses (gabung, konversi, pindah file).
    Processing(Step),
}

/// Hasil akhir sebuah unduhan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Berhasil. `file`: lokasi file hasil, jika dilaporkan yt-dlp.
    Done { file: Option<PathBuf> },
    /// Gagal; pesannya siap ditampilkan ke pengguna.
    Failed(String),
    /// Dibatalkan pengguna.
    Cancelled,
}

/// Penghubung untuk membatalkan unduhan dari thread lain (mis. tombol Batal di UI).
#[derive(Debug, Clone, Default)]
pub struct CancelHandle {
    inner: Arc<CancelState>,
}

#[derive(Debug, Default)]
struct CancelState {
    cancelled: AtomicBool,
    /// `true` setelah [`run`] selesai, termasuk membersihkan folder sementara.
    finished: AtomicBool,
    /// PID yt-dlp selama prosesnya berjalan.
    pid: Mutex<Option<u32>>,
}

impl CancelHandle {
    /// Minta unduhan berhenti. Tidak menunggu: prosesnya dihentikan di thread lain,
    /// jadi aman dipanggil dari UI thread.
    pub fn cancel(&self) {
        if let Some(pid) = self.request_cancel() {
            std::thread::spawn(move || process::kill_tree(pid));
        }
    }

    /// Hentikan unduhan dan tunggu sampai folder sementaranya bersih, paling lama `timeout`.
    /// Dipakai saat aplikasi ditutup di tengah unduhan.
    pub fn cancel_and_wait(&self, timeout: Duration) {
        if let Some(pid) = self.request_cancel() {
            process::kill_tree(pid);
        }
        let started = Instant::now();
        while !self.is_finished() && started.elapsed() < timeout {
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// `true` setelah unduhan benar-benar selesai (berhasil, gagal, atau dibatalkan).
    pub fn is_finished(&self) -> bool {
        self.inner.finished.load(Ordering::SeqCst)
    }

    fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Menandai batal, lalu mengembalikan PID yang harus dihentikan (jika sedang berjalan).
    fn request_cancel(&self) -> Option<u32> {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        *self.pid()
    }

    /// Mencatat PID yang sedang berjalan. Mengembalikan `true` jika pembatalan sudah
    /// diminta sebelumnya, sehingga proses ini harus langsung dihentikan.
    fn set_running(&self, pid: u32) -> bool {
        let mut slot = self.pid();
        *slot = Some(pid);
        self.is_cancelled()
    }

    fn clear_running(&self) {
        *self.pid() = None;
    }

    fn pid(&self) -> MutexGuard<'_, Option<u32>> {
        // Mutex hanya "rusak" jika thread lain panik saat memegangnya; isinya tetap bisa dipakai.
        self.inner
            .pid
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Folder sementara untuk satu unduhan, di dalam folder tujuan.
pub fn temp_dir_for(folder: &Path, job_id: u64) -> PathBuf {
    folder
        .join(TEMP_DIR_NAME)
        .join(format!("{}-{job_id}", std::process::id()))
}

/// Menyusun argumen yt-dlp. Argumen diberikan sebagai daftar terpisah (bukan satu
/// string shell), jadi URL dan path berisi spasi atau karakter aneh tetap aman.
pub fn build_args(request: &Request, tools: &Tools, temp_dir: &Path) -> Vec<OsString> {
    let fixed = [
        // Abaikan file konfigurasi yt-dlp milik pengguna, supaya keluarannya bisa ditebak.
        "--ignore-config",
        // Keluaran UTF-8, supaya judul dan nama file berhuruf non-latin terbaca benar.
        "--encoding",
        "utf-8",
        "--color",
        "never",
        // Satu baris per laporan progress (bukan menimpa baris yang sama).
        "--newline",
        // `--print` membuat yt-dlp "diam"; progress tetap diminta muncul.
        "--progress",
        // Playlist baru didukung di Fase 7: unduh videonya saja.
        "--no-playlist",
        "--windows-filenames",
        // Tanggal file = waktu diunduh, bukan tanggal upload (memudahkan urutan di Pustaka).
        "--no-mtime",
        "-o",
        OUTPUT_TEMPLATE,
        "--progress-template",
        DOWNLOAD_TEMPLATE,
        "--progress-template",
        POSTPROCESS_TEMPLATE,
        "--print",
        "before_dl:DANLOAD|title|%(title)s",
        "--print",
        "after_move:DANLOAD|file|%(filepath)s",
    ];
    let mut args: Vec<OsString> = fixed.iter().map(OsString::from).collect();
    args.push("-P".into());
    args.push(prefixed("home:", &request.folder));
    args.push("-P".into());
    args.push(prefixed("temp:", temp_dir));
    args.push("--ffmpeg-location".into());
    args.push(tools.ffmpeg_dir.clone().into_os_string());
    // yt-dlp memisahkan "deno:PATH" di titik dua pertama saja, jadi path Windows aman.
    args.push("--js-runtimes".into());
    args.push(prefixed("deno:", &tools.deno));
    args.extend(request.format.args().into_iter().map(OsString::from));
    // `--` menandai akhir opsi: URL tidak akan pernah dibaca sebagai opsi.
    args.push("--".into());
    args.push(request.url.trim().into());
    args
}

fn prefixed(prefix: &str, path: &Path) -> OsString {
    let mut value = OsString::from(prefix);
    value.push(path.as_os_str());
    value
}

/// Menjalankan satu unduhan sampai selesai.
///
/// - `job_id`: nomor unik unduhan ini (untuk nama folder sementara).
/// - `cancel`: dipakai pemanggil untuk membatalkan dari thread lain.
/// - `on_update`: dipanggil setiap ada kemajuan (paling sering sekitar 10 kali per detik).
pub fn run(
    request: &Request,
    tools: &Tools,
    job_id: u64,
    cancel: &CancelHandle,
    on_update: &mut dyn FnMut(Update),
) -> Outcome {
    let temp_dir = temp_dir_for(&request.folder, job_id);
    let outcome = run_in(request, tools, &temp_dir, cancel, on_update);
    remove_temp_dir(&temp_dir);
    cancel.inner.finished.store(true, Ordering::SeqCst);
    outcome
}

fn run_in(
    request: &Request,
    tools: &Tools,
    temp_dir: &Path,
    cancel: &CancelHandle,
    on_update: &mut dyn FnMut(Update),
) -> Outcome {
    if let Err(error) =
        fs::create_dir_all(&request.folder).and_then(|()| fs::create_dir_all(temp_dir))
    {
        return Outcome::Failed(format!(
            "Folder unduhan tidak bisa dibuat atau ditulisi ({}): {error}. Pilih folder lain di Pengaturan.",
            request.folder.display()
        ));
    }
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }

    let mut command = Command::new(&tools.yt_dlp);
    command
        .args(build_args(request, tools, temp_dir))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process::run_in_background(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Outcome::Failed(format!(
                "yt-dlp tidak bisa dijalankan: {error}. Coba pasang ulang lewat Pengaturan."
            ));
        }
    };
    if cancel.set_running(child.id()) {
        process::kill_tree(child.id());
    }

    // Baca stdout dan stderr bersamaan di dua thread; barisnya dikumpulkan lewat channel.
    let (sender, receiver) = mpsc::channel::<String>();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(spawn_line_reader(stdout, sender.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(spawn_line_reader(stderr, sender.clone()));
    }
    drop(sender); // Channel tertutup sendiri setelah kedua pembaca selesai.

    let mut output = OutputState::default();
    loop {
        match receiver.recv_timeout(PROGRESS_INTERVAL) {
            Ok(line) => output.handle_line(&line, on_update),
            Err(RecvTimeoutError::Timeout) => output.flush_if_due(on_update),
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    output.flush(on_update);
    for reader in readers {
        let _ = reader.join();
    }
    let status = child.wait();
    cancel.clear_running();

    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    match status {
        Ok(status) if status.success() => Outcome::Done { file: output.file },
        Ok(status) => Outcome::Failed(output.failure_message(status.code())),
        Err(error) => Outcome::Failed(format!("Gagal menunggu yt-dlp selesai: {error}")),
    }
}

/// Membaca `stream` baris demi baris dan mengirimkannya lewat `sender`.
fn spawn_line_reader(
    stream: impl Read + Send + 'static,
    sender: Sender<String>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for chunk in BufReader::new(stream).split(b'\n') {
            let Ok(bytes) = chunk else { break };
            let line = String::from_utf8_lossy(&bytes)
                .trim_end_matches('\r')
                .to_string();
            if sender.send(line).is_err() {
                break;
            }
        }
    })
}

/// Menghapus folder sementara milik unduhan ini, lalu `.danload-tmp` jika sudah kosong.
fn remove_temp_dir(temp_dir: &Path) {
    let _ = fs::remove_dir_all(temp_dir);
    if let Some(parent) = temp_dir.parent() {
        let _ = fs::remove_dir(parent); // Hanya berhasil jika kosong.
    }
}

/// Keadaan yang dikumpulkan dari keluaran yt-dlp.
#[derive(Default)]
struct OutputState {
    /// Progress terbaru yang belum dikirim (dibatasi frekuensinya).
    pending: Option<Update>,
    last_sent: Option<Instant>,
    /// Setelah pemrosesan dimulai, laporan unduhan yang datang terlambat diabaikan.
    processing_started: bool,
    file: Option<PathBuf>,
    first_error: Option<String>,
    last_other_line: Option<String>,
}

impl OutputState {
    fn handle_line(&mut self, line: &str, on_update: &mut dyn FnMut(Update)) {
        match parse_line(line) {
            Some(Event::Title(title)) => {
                self.flush(on_update);
                on_update(Update::Title(title));
            }
            Some(Event::Progress { update, finished }) => {
                if self.processing_started {
                    return;
                }
                self.pending = Some(update);
                if finished || self.is_due() {
                    self.flush(on_update);
                }
            }
            Some(Event::Step(step)) => {
                self.flush(on_update);
                self.processing_started = true;
                on_update(Update::Processing(step));
            }
            Some(Event::File(path)) => self.file = Some(path),
            None => self.note_other_line(line),
        }
    }

    fn note_other_line(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        if line.starts_with("ERROR:") {
            if self.first_error.is_none() {
                self.first_error = Some(line.to_string());
            }
        } else {
            self.last_other_line = Some(line.to_string());
        }
    }

    fn is_due(&self) -> bool {
        self.last_sent
            .is_none_or(|sent| sent.elapsed() >= PROGRESS_INTERVAL)
    }

    fn flush_if_due(&mut self, on_update: &mut dyn FnMut(Update)) {
        if self.is_due() {
            self.flush(on_update);
        }
    }

    fn flush(&mut self, on_update: &mut dyn FnMut(Update)) {
        if let Some(update) = self.pending.take() {
            on_update(update);
            self.last_sent = Some(Instant::now());
        }
    }

    fn failure_message(&self, exit_code: Option<i32>) -> String {
        match (&self.first_error, &self.last_other_line) {
            (Some(error), _) => friendly_error(error),
            (None, Some(line)) => friendly_error(line),
            (None, None) => match exit_code {
                Some(code) => format!("yt-dlp berhenti tanpa pesan (kode {code})."),
                None => "yt-dlp berhenti tanpa pesan.".to_string(),
            },
        }
    }
}

/// Satu baris keluaran yang dimengerti DANload.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Title(String),
    Progress { update: Update, finished: bool },
    Step(Step),
    File(PathBuf),
}

/// Membaca satu baris berawalan `DANLOAD|`. Baris lain menghasilkan `None`.
fn parse_line(line: &str) -> Option<Event> {
    let rest = line.trim_end().strip_prefix(LINE_PREFIX)?;
    let (kind, data) = rest.split_once('|')?;
    match kind {
        "title" => Some(Event::Title(data.trim().to_string())),
        "file" => {
            let path = data.trim();
            (!path.is_empty() && path != "NA").then(|| Event::File(PathBuf::from(path)))
        }
        "pp" => {
            let (status, name) = data.split_once('|')?;
            (status == "started").then(|| Event::Step(step_for(name.trim())))
        }
        "dl" => {
            let fields: Vec<&str> = data.split('|').collect();
            let [
                status,
                downloaded,
                total,
                estimate,
                speed,
                eta,
                vcodec,
                acodec,
            ] = fields.as_slice()
            else {
                return None;
            };
            if *status != "downloading" && *status != "finished" {
                return None;
            }
            Some(Event::Progress {
                update: Update::Downloading {
                    part: part_for(vcodec, acodec),
                    downloaded: number(downloaded).map_or(0, |value| value as u64),
                    total: number(total)
                        .or_else(|| number(estimate))
                        .map(|value| value as u64),
                    speed: number(speed),
                    eta: number(eta).map(|value| value as u64),
                },
                finished: *status == "finished",
            })
        }
        _ => None,
    }
}

/// Angka dari keluaran yt-dlp; `NA` atau teks lain menjadi `None`.
fn number(text: &str) -> Option<f64> {
    let value: f64 = text.trim().parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

/// Bagian yang diunduh, dari codec video/audionya (`none` = tidak ada).
fn part_for(vcodec: &str, acodec: &str) -> Part {
    let known = |codec: &str| !codec.is_empty() && codec != "NA";
    match (vcodec.trim(), acodec.trim()) {
        ("none", audio) if known(audio) => Part::Audio,
        (video, "none") if known(video) => Part::Video,
        _ => Part::Whole,
    }
}

/// Tahap pemrosesan dari nama postprocessor yt-dlp.
fn step_for(name: &str) -> Step {
    match name {
        "Merger" => Step::Merging,
        "ExtractAudio" => Step::ConvertingAudio,
        "Metadata" | "FFmpegMetadata" | "EmbedThumbnail" => Step::Metadata,
        "MoveFiles" | "MoveFilesAfterDownload" => Step::Moving,
        other if other.starts_with("Fixup") => Step::Fixing,
        _ => Step::Other,
    }
}

/// Teks untuk sebuah tahap pemrosesan.
pub fn describe_step(step: Step) -> &'static str {
    match step {
        Step::Merging => "Menggabungkan video dan audio...",
        Step::ConvertingAudio => "Mengubah ke MP3...",
        Step::Metadata => "Menambahkan info judul...",
        Step::Fixing => "Merapikan file...",
        Step::Moving => "Memindahkan file ke folder unduhan...",
        Step::Other => "Memproses file...",
    }
}

/// Mengubah baris error yt-dlp menjadi pesan yang mudah dipahami.
///
/// Pencocokan memakai seluruh baris (huruf kecil); jika tidak ada yang cocok,
/// pesan aslinya ditampilkan tanpa awalan teknis.
pub fn friendly_error(line: &str) -> String {
    let lower = line.to_lowercase();
    // Urutan penting: yang lebih spesifik diletakkan di atas.
    let known: &[(&[&str], &str)] = &[
        (
            &["private video", "video is private"],
            "Video ini privat, jadi tidak bisa diunduh.",
        ),
        (
            &[
                "confirm your age",
                "age-restricted",
                "inappropriate for some users",
            ],
            "Video ini dibatasi umur dan butuh login, jadi belum bisa diunduh.",
        ),
        (
            &["not a bot"],
            "YouTube meminta verifikasi bahwa kamu bukan bot. Coba lagi beberapa saat lagi.",
        ),
        (
            &["members-only", "join this channel", "channel's members"],
            "Video ini khusus anggota channel.",
        ),
        (
            &["live event will begin", "premieres in", "is upcoming"],
            "Siaran atau premiere ini belum dimulai.",
        ),
        (
            &[
                "not available in your country",
                "geo restrict",
                "geo-restrict",
            ],
            "Video ini tidak tersedia di negaramu.",
        ),
        (
            &[
                "video unavailable",
                "video is unavailable",
                "has been removed",
                "no longer available",
            ],
            "Video tidak tersedia. Mungkin sudah dihapus atau dibatasi pemiliknya.",
        ),
        (
            &["requested format is not available"],
            "Kualitas yang dipilih tidak tersedia untuk video ini. Coba pilih kualitas lain.",
        ),
        (&["unsupported url"], "Link ini tidak didukung."),
        (
            &["no space left", "errno 28", "not enough space"],
            "Ruang disk tidak cukup.",
        ),
        (
            &["permission denied", "errno 13", "access is denied"],
            "DANload tidak punya izin menulis ke folder unduhan. Pilih folder lain di Pengaturan.",
        ),
        (
            &[
                "challenge solving failed",
                "signature solving failed",
                "javascript runtime",
                "js runtime",
            ],
            "yt-dlp gagal memproses video YouTube ini. Coba perbarui komponen lewat Pengaturan → Pasang / perbarui...",
        ),
        (
            &["http error 429", "too many requests"],
            "Terlalu banyak permintaan ke server. Tunggu sebentar lalu coba lagi.",
        ),
        (
            &["http error 404"],
            "Video tidak ditemukan (kode 404). Periksa kembali link-nya.",
        ),
        (
            &["http error 403"],
            "Server menolak permintaan (kode 403). Coba perbarui komponen lewat Pengaturan.",
        ),
        (
            &[
                "unable to download webpage",
                "failed to resolve",
                "getaddrinfo",
                "timed out",
                "connection",
                "network is unreachable",
                "name or service not known",
                "urlopen error",
            ],
            "Tidak bisa terhubung ke server. Periksa koneksi internet lalu coba lagi.",
        ),
    ];
    for (needles, message) in known {
        if needles.iter().any(|needle| lower.contains(needle)) {
            return (*message).to_string();
        }
    }
    let detail: String = strip_error_prefix(line).chars().take(220).collect();
    format!("Unduhan gagal: {detail}")
}

/// Membuang awalan teknis, mis. `ERROR: [youtube] dQw4w9WgXcQ: ` → sisa pesannya.
fn strip_error_prefix(line: &str) -> &str {
    let mut rest = line.trim();
    rest = rest.strip_prefix("ERROR:").unwrap_or(rest).trim_start();
    if rest.starts_with('[')
        && let Some(end) = rest.find("] ")
    {
        rest = &rest[end + 2..];
        if let Some((_id, message)) = rest.split_once(": ") {
            rest = message;
        }
    }
    rest.trim()
}

/// Teks kemajuan unduhan, mis. `Mengunduh video · 45% · 3,2 MB/s · sisa 0:12`.
/// Mengembalikan juga nilai 0..1 untuk batang kemajuan.
pub fn describe_download(
    part: Part,
    downloaded: u64,
    total: Option<u64>,
    speed: Option<f64>,
    eta: Option<u64>,
) -> (String, f32) {
    let mut pieces = vec![
        match part {
            Part::Video => "Mengunduh video",
            Part::Audio => "Mengunduh audio",
            Part::Whole => "Mengunduh",
        }
        .to_string(),
    ];
    let fraction = match total.filter(|total| *total > 0) {
        Some(total) => {
            let fraction = (downloaded as f64 / total as f64).clamp(0.0, 1.0);
            pieces.push(format!("{}%", (fraction * 100.0).floor() as u64));
            fraction as f32
        }
        None => {
            pieces.push(format_size(downloaded));
            0.0
        }
    };
    if let Some(speed) = speed.filter(|speed| *speed > 0.0) {
        pieces.push(format!("{}/s", format_size(speed as u64)));
    }
    if let Some(eta) = eta.filter(|_| fraction < 1.0) {
        pieces.push(format!("sisa {}", format_duration(eta)));
    }
    (pieces.join(" · "), fraction)
}

/// Ukuran yang mudah dibaca, mis. `850 KB`, `3,2 MB`, `1,1 GB` (desimal pakai koma).
pub fn format_size(bytes: u64) -> String {
    let value = bytes as f64;
    let text = if value >= 1e9 {
        format!("{:.1} GB", value / 1e9)
    } else if value >= 1e6 {
        format!("{:.1} MB", value / 1e6)
    } else {
        format!("{:.0} KB", value / 1e3)
    };
    text.replace('.', ",")
}

/// Durasi seperti jam digital, mis. `0:12`, `4:05`, `1:02:03`.
pub fn format_duration(seconds: u64) -> String {
    let (hours, minutes, secs) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes}:{secs:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_progress_lines_from_real_yt_dlp() {
        // Baris asli dari yt-dlp 2026.10.26 (diuji terhadap server lokal).
        let line = "DANLOAD|dl|downloading|261120|676269|NA|2849232.9677671115|0|NA|NA";
        assert_eq!(
            parse_line(line),
            Some(Event::Progress {
                update: Update::Downloading {
                    part: Part::Whole,
                    downloaded: 261_120,
                    total: Some(676_269),
                    speed: Some(2_849_232.967_767_111_5),
                    eta: Some(0),
                },
                finished: false,
            })
        );
        let first = parse_line("DANLOAD|dl|downloading|1024|676269|NA|NA|NA|NA|NA");
        assert!(matches!(
            first,
            Some(Event::Progress {
                update: Update::Downloading {
                    speed: None,
                    eta: None,
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn total_falls_back_to_estimate_and_parts_are_detected() {
        let video = parse_line("DANLOAD|dl|downloading|500|NA|1000.0|10.5|3|avc1.640028|none");
        assert_eq!(
            video,
            Some(Event::Progress {
                update: Update::Downloading {
                    part: Part::Video,
                    downloaded: 500,
                    total: Some(1000),
                    speed: Some(10.5),
                    eta: Some(3),
                },
                finished: false,
            })
        );
        let audio = parse_line("DANLOAD|dl|finished|900|900|NA|NA|NA|none|mp4a.40.2");
        assert!(matches!(
            audio,
            Some(Event::Progress {
                update: Update::Downloading {
                    part: Part::Audio,
                    ..
                },
                finished: true
            })
        ));
    }

    #[test]
    fn parses_title_file_and_steps() {
        assert_eq!(
            parse_line("DANLOAD|title|Judul | dengan garis | tegak"),
            Some(Event::Title("Judul | dengan garis | tegak".to_string()))
        );
        assert_eq!(
            parse_line("DANLOAD|file|/tmp/out/vidéo ü 日本 [abc].mp3\r"),
            Some(Event::File(PathBuf::from(
                "/tmp/out/vidéo ü 日本 [abc].mp3"
            )))
        );
        assert_eq!(
            parse_line("DANLOAD|pp|started|ExtractAudio"),
            Some(Event::Step(Step::ConvertingAudio))
        );
        assert_eq!(parse_line("DANLOAD|pp|finished|ExtractAudio"), None);
        assert_eq!(
            parse_line("DANLOAD|pp|started|Merger"),
            Some(Event::Step(Step::Merging))
        );
        assert_eq!(
            parse_line("DANLOAD|pp|started|FixupM4a"),
            Some(Event::Step(Step::Fixing))
        );
        assert_eq!(parse_line("[download] 45.3% of 10.00MiB"), None);
        assert_eq!(parse_line("DANLOAD|dl|downloading|1|2"), None);
    }

    #[test]
    fn friendly_errors_for_common_cases() {
        let cases = [
            (
                "ERROR: [youtube] abc: Private video. Sign in if you've been granted access",
                "privat",
            ),
            (
                "ERROR: [youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users.",
                "umur",
            ),
            (
                "ERROR: [youtube] abc: Sign in to confirm you're not a bot.",
                "bukan bot",
            ),
            (
                "ERROR: [youtube] abc: Video unavailable. This video has been removed by the uploader",
                "tidak tersedia",
            ),
            (
                "ERROR: [generic] tidak-ada: Unable to download webpage: HTTP Error 404: File not found (caused by <HTTPError 404: File not found>)",
                "404",
            ),
            (
                "ERROR: [generic] vidéo ü 日本: Requested format is not available. Use --list-formats for a list of available formats",
                "Kualitas",
            ),
            (
                "ERROR: [youtube] abc: Unable to download API page: <urlopen error [Errno 11001] getaddrinfo failed>",
                "koneksi internet",
            ),
            (
                "ERROR: unable to write data: [Errno 28] No space left on device",
                "disk",
            ),
        ];
        for (line, expected) in cases {
            let message = friendly_error(line);
            assert!(message.contains(expected), "{line}\n→ {message}");
        }
    }

    #[test]
    fn unknown_errors_keep_the_message_without_prefix() {
        assert_eq!(
            friendly_error("ERROR: [youtube] dQw4w9WgXcQ: Sesuatu yang aneh terjadi"),
            "Unduhan gagal: Sesuatu yang aneh terjadi"
        );
        assert_eq!(
            friendly_error("ERROR: Postprocessing: kodek tidak dikenal"),
            "Unduhan gagal: Postprocessing: kodek tidak dikenal"
        );
    }

    #[test]
    fn progress_text() {
        let (text, fraction) = describe_download(
            Part::Video,
            45_000_000,
            Some(100_000_000),
            Some(3_200_000.0),
            Some(12),
        );
        assert_eq!(text, "Mengunduh video · 45% · 3,2 MB/s · sisa 0:12");
        assert!((fraction - 0.45).abs() < 1e-6);

        let (text, fraction) = describe_download(Part::Whole, 850_000, None, None, None);
        assert_eq!(text, "Mengunduh · 850 KB");
        assert_eq!(fraction, 0.0);
        assert_eq!(format_duration(3723), "1:02:03");
        assert_eq!(format_size(1_100_000_000), "1,1 GB");
    }

    #[test]
    fn args_put_url_last_after_double_dash() {
        let request = Request {
            url: "  https://youtu.be/abc  ".into(),
            format: OutputFormat::Mp3 { bitrate_kbps: 192 },
            folder: PathBuf::from("D:\\Unduhan Saya\\Audio"),
        };
        let tools = Tools {
            yt_dlp: PathBuf::from("C:\\bin\\yt-dlp.exe"),
            ffmpeg_dir: PathBuf::from("C:\\bin"),
            deno: PathBuf::from("C:\\Program Files\\deno.exe"),
        };
        let args = build_args(
            &request,
            &tools,
            Path::new("D:\\Unduhan Saya\\Audio\\.danload-tmp\\1"),
        );
        let args: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[args.len() - 2..], ["--", "https://youtu.be/abc"]);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-P", "home:D:\\Unduhan Saya\\Audio"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--js-runtimes", "deno:C:\\Program Files\\deno.exe"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--ffmpeg-location", "C:\\bin"])
        );
        assert!(args.contains(&"--ignore-config".to_string()));
    }

    #[test]
    fn late_download_lines_are_ignored_after_processing_starts() {
        let mut state = OutputState::default();
        let mut updates = Vec::new();
        let mut collect = |update: Update| updates.push(update);
        state.handle_line("DANLOAD|pp|started|Merger", &mut collect);
        state.handle_line("DANLOAD|dl|finished|10|10|NA|NA|NA|none|opus", &mut collect);
        state.flush(&mut collect);
        assert_eq!(updates, vec![Update::Processing(Step::Merging)]);
    }

    // ----- Uji menyeluruh dengan yt-dlp palsu (skrip shell, hanya di Linux/macOS) -----

    #[cfg(unix)]
    fn fake_tools(name: &str, script: &str) -> (Tools, PathBuf) {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("danload-dl-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let program = dir.join("yt-dlp");
        fs::write(&program, script).unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        let tools = Tools {
            yt_dlp: program,
            ffmpeg_dir: dir.clone(),
            deno: dir.join("deno"),
        };
        (tools, dir)
    }

    /// Bagian awal skrip palsu: membaca folder `home:` dan `temp:` dari argumen `-P`.
    #[cfg(unix)]
    const READ_PATHS: &str = r#"#!/bin/sh
home=""; temp=""; prev=""
for arg in "$@"; do
  if [ "$prev" = "-P" ]; then
    case "$arg" in home:*) home="${arg#home:}";; temp:*) temp="${arg#temp:}";; esac
  fi
  prev="$arg"
done
"#;

    #[cfg(unix)]
    #[test]
    fn run_success_reports_progress_file_and_cleans_temp() {
        let script = format!(
            "{READ_PATHS}{}",
            r#"echo "DANLOAD|title|Video Uji é"
touch "$temp/Video Uji é [abc].f137.mp4.part"
for n in 100 200 300; do echo "DANLOAD|dl|downloading|$n|300|NA|1000.0|1|avc1|none"; done
echo "DANLOAD|dl|finished|300|300|NA|NA|NA|avc1|none"
# Seperti yt-dlp asli: ada jeda antara selesai unduh dan mulai FFmpeg. Tanpa jeda ini,
# urutan stdout/stderr bisa tertukar (dua saluran terpisah) dan test jadi tidak pasti.
sleep 0.3
echo "DANLOAD|pp|started|Merger" >&2
echo "isi" > "$home/Video Uji é [abc].mp4"
echo "DANLOAD|file|$home/Video Uji é [abc].mp4"
"#
        );
        let (tools, dir) = fake_tools("berhasil", &script);
        let folder = dir.join("Unduhan Saya").join("Video");
        let request = Request {
            url: "https://youtu.be/abc".into(),
            format: OutputFormat::Mp4 { max_height: None },
            folder: folder.clone(),
        };
        let mut updates = Vec::new();
        let handle = CancelHandle::default();

        let outcome = run(&request, &tools, 1, &handle, &mut |update| {
            updates.push(update)
        });

        let expected = folder.join("Video Uji é [abc].mp4");
        assert_eq!(
            outcome,
            Outcome::Done {
                file: Some(expected.clone())
            }
        );
        assert!(expected.is_file());
        assert_eq!(updates.first(), Some(&Update::Title("Video Uji é".into())));
        assert!(updates.contains(&Update::Processing(Step::Merging)));
        assert!(updates.iter().any(|update| matches!(
            update,
            Update::Downloading {
                part: Part::Video,
                downloaded: 300,
                total: Some(300),
                ..
            }
        )));
        assert!(
            !folder.join(TEMP_DIR_NAME).exists(),
            "folder sementara harus terhapus"
        );
        assert!(handle.is_finished());
    }

    #[cfg(unix)]
    #[test]
    fn run_failure_gives_friendly_message() {
        let script = "#!/bin/sh\necho 'WARNING: sesuatu' >&2\necho 'ERROR: [youtube] abc: Private video. Sign in if you have access' >&2\nexit 1\n";
        let (tools, dir) = fake_tools("gagal", script);
        let request = Request {
            url: "https://youtu.be/abc".into(),
            format: OutputFormat::Mp3 { bitrate_kbps: 192 },
            folder: dir.join("out"),
        };

        let outcome = run(&request, &tools, 2, &CancelHandle::default(), &mut |_| {});

        assert_eq!(
            outcome,
            Outcome::Failed("Video ini privat, jadi tidak bisa diunduh.".into())
        );
        assert!(!dir.join("out").join(TEMP_DIR_NAME).exists());
    }

    #[cfg(unix)]
    #[test]
    fn run_cancel_stops_whole_process_group() {
        // Skrip menjalankan proses anak (seperti yt-dlp menjalankan FFmpeg), lalu mencatat
        // PID anak itu supaya test bisa memastikan anaknya ikut berhenti.
        let script = format!(
            "{READ_PATHS}{}",
            r#"sleep 300 &
echo $! > "$(dirname "$0")/anak.pid"
touch "$temp/besar.mp4.part"
i=0
while true; do i=$((i+1)); echo "DANLOAD|dl|downloading|$i|1000000|NA|NA|NA|NA|NA"; sleep 0.05; done
"#
        );
        let (tools, dir) = fake_tools("batal", &script);
        let folder = dir.join("out");
        let request = Request {
            url: "https://youtu.be/abc".into(),
            format: OutputFormat::Mp4 {
                max_height: Some(720),
            },
            folder: folder.clone(),
        };
        let handle = CancelHandle::default();
        let canceller = handle.clone();

        let started = Instant::now();
        let outcome = run(&request, &tools, 3, &handle, &mut |update| {
            if matches!(update, Update::Downloading { .. }) {
                canceller.cancel();
            }
        });

        assert_eq!(outcome, Outcome::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(
            !folder.join(TEMP_DIR_NAME).exists(),
            "file .part harus ikut terhapus"
        );
        let child_pid = fs::read_to_string(dir.join("anak.pid")).unwrap();
        let child_alive = || {
            fs::read_to_string(format!("/proc/{}/stat", child_pid.trim()))
                .map(|stat| !stat.contains(") Z "))
                .unwrap_or(false)
        };
        let deadline = Instant::now() + Duration::from_secs(3);
        while child_alive() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !child_alive(),
            "proses anak (seperti FFmpeg) harus ikut berhenti"
        );
    }

    #[cfg(unix)]
    #[test]
    fn unwritable_folder_fails_before_starting() {
        let (tools, dir) = fake_tools("folder", "#!/bin/sh\nexit 0\n");
        let blocker = dir.join("berkas-biasa");
        fs::write(&blocker, "bukan folder").unwrap();
        let request = Request {
            url: "https://youtu.be/abc".into(),
            format: OutputFormat::Mp3 { bitrate_kbps: 192 },
            folder: blocker.join("anak"),
        };

        let outcome = run(&request, &tools, 4, &CancelHandle::default(), &mut |_| {});

        assert!(matches!(outcome, Outcome::Failed(message) if message.contains("Folder unduhan")));
    }

    /// Uji dengan yt-dlp sungguhan. Tidak jalan otomatis; jalankan dengan, misalnya:
    /// `DANLOAD_TEST_YTDLP=... DANLOAD_TEST_FFMPEG_DIR=... DANLOAD_TEST_DENO=...
    ///  DANLOAD_TEST_URL=... cargo test real_yt_dlp -- --ignored --nocapture`
    #[test]
    #[ignore = "butuh yt-dlp asli dan URL uji"]
    fn real_yt_dlp_end_to_end() {
        let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} belum diisi"));
        let tools = Tools {
            yt_dlp: env("DANLOAD_TEST_YTDLP").into(),
            ffmpeg_dir: env("DANLOAD_TEST_FFMPEG_DIR").into(),
            deno: env("DANLOAD_TEST_DENO").into(),
        };
        let folder = std::env::temp_dir().join("danload-uji-asli");
        for format in [
            OutputFormat::Mp3 { bitrate_kbps: 128 },
            OutputFormat::Mp4 {
                max_height: Some(360),
            },
        ] {
            let request = Request {
                url: env("DANLOAD_TEST_URL"),
                format,
                folder: folder.clone(),
            };
            let mut count = 0;
            let outcome = run(
                &request,
                &tools,
                50,
                &CancelHandle::default(),
                &mut |update| {
                    count += 1;
                    if !matches!(update, Update::Downloading { .. }) {
                        println!("  {update:?}");
                    }
                },
            );
            println!("{} → {outcome:?} ({count} laporan)", format.label());
            assert!(matches!(outcome, Outcome::Done { file: Some(ref file) } if file.is_file()));
        }
        assert!(!folder.join(TEMP_DIR_NAME).exists());
    }

    #[test]
    fn cancel_before_start_is_remembered() {
        let handle = CancelHandle::default();
        handle.cancel();
        assert!(handle.is_cancelled());
        assert!(handle.set_running(12345));
        handle.clear_running();
    }
}
