//! Menjalankan yt-dlp untuk satu unduhan dan membaca kemajuannya (CLAUDE.md Bagian 2.10).
//!
//! Cara kerja:
//! 1. yt-dlp dijalankan dengan `--progress-template` dan `--print`, supaya mencetak baris
//!    khusus berawalan `DANLOAD|` yang mudah dibaca program (bukan teks untuk manusia).
//! 2. Baris dari stdout dan stderr dibaca di dua thread kecil. Progress unduhan keluar di
//!    stdout, sedangkan tahap FFmpeg (gabung, konversi) keluar di stderr.
//! 3. yt-dlp menaruh hasilnya di folder sementara (`.danload-tmp\<job>\hasil\<ID>\Judul.ext`).
//!    Satu link bisa menghasilkan beberapa video (mis. postingan Instagram berisi beberapa
//!    video); semuanya dikumpulkan.
//! 4. Untuk WEBM, MOV, AVI, dan MPEG, tiap file lalu dibuat formatnya oleh langkah konversi
//!    DANload (`src/convert.rs`).
//! 5. Tiap file dipindah ke folder unduhan dengan nama pilihan pengguna atau judul videonya.
//!    Nama yang sudah dipakai mendapat nomor, mis. `Judul (2).mp4` (`src/filename.rs`); file
//!    lain tidak pernah ditimpa.
//! 6. Untuk Pustaka (sejak Fase 6): durasi dan kualitas sebenarnya dibaca dengan ffprobe, dan
//!    thumbnail dari platform (diunduh yt-dlp di sebelah filenya) dikecilkan ke folder thumbnail.
//! 7. Folder sementara selalu dihapus setelah unduhan berhasil, gagal, atau dibatalkan.
//!
//! Modul ini tidak bergantung pada GUI. [`run`] lambat: **jangan dipanggil dari UI thread**.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

use crate::convert::{self, ConvertError};
use crate::filename::{self, MAX_NAME_CHARS};
use crate::formats::{OutputFormat, VideoFormat};
use crate::library;
use crate::process;

/// Jeda minimal antar laporan progress ke UI (CLAUDE.md Bagian 2.6).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Awalan baris yang dicetak khusus untuk DANload.
const LINE_PREFIX: &str = "DANLOAD|";

/// Nama folder sementara di dalam folder unduhan.
pub const TEMP_DIR_NAME: &str = ".danload-tmp";

/// Subfolder di folder sementara tempat yt-dlp menaruh hasilnya.
const STAGING_DIR_NAME: &str = "hasil";

/// Paling banyak sekian video dari satu link (satu postingan Instagram berisi paling banyak
/// 20 item). Pengaman supaya link yang ternyata berisi banyak video tidak mengunduh semuanya.
/// Playlist YouTube tidak lewat sini: isinya dibaca `src/playlist.rs`, lalu tiap video yang
/// dipilih menjadi unduhan sendiri di antrean.
const MAX_ITEMS: u32 = 20;

/// Template progress unduhan. `NA` dicetak yt-dlp untuk nilai yang tidak diketahui.
const DOWNLOAD_TEMPLATE: &str = "download:DANLOAD|dl|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s|%(info.vcodec)s|%(info.acodec)s";

/// Template progress tahap pemrosesan (FFmpeg dan lainnya).
const POSTPROCESS_TEMPLATE: &str =
    "postprocess:DANLOAD|pp|%(progress.status)s|%(progress.postprocessor)s";

/// Giliran konversi berat (mengubah ulang video dengan FFmpeg): hanya satu per waktu, supaya
/// beberapa unduhan bersamaan tidak membuat komputer berat (keputusan pemilik projek,
/// 2026-11-16). Langkah yang hanya menyalin stream ringan, jadi tidak perlu menunggu.
static CONVERSION_TURN: Mutex<()> = Mutex::new(());

/// Seberapa sering giliran konversi dicek saat menunggu.
const TURN_POLL: Duration = Duration::from_millis(200);

/// Nama file di folder sementara: subfolder per video (ID-nya), lalu judulnya (dipotong
/// [`MAX_NAME_CHARS`] karakter). Subfolder mencegah dua video berjudul sama saling timpa,
/// mis. isi satu postingan Instagram yang semuanya berjudul "Video by <akun>".
fn output_template() -> String {
    format!("%(id)s/%(title).{MAX_NAME_CHARS}s.%(ext)s")
}

/// Lokasi program yang dibutuhkan untuk mengunduh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
    pub deno: PathBuf,
}

impl Tools {
    /// Folder berisi FFmpeg, untuk `--ffmpeg-location` yt-dlp.
    fn ffmpeg_dir(&self) -> &Path {
        self.ffmpeg.parent().unwrap_or(Path::new("."))
    }
}

/// Satu permintaan unduhan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub url: String,
    pub format: OutputFormat,
    /// Folder tujuan (sudah termasuk subfolder Video/Audio jika dipakai).
    pub folder: PathBuf,
    /// Nama file pilihan pengguna, tanpa ekstensi (sudah diperiksa
    /// [`filename::check_custom_name`]). `None` = mengikuti judul video.
    pub file_name: Option<String>,
    /// Folder thumbnail Pustaka. `None` = tanpa thumbnail (yt-dlp tidak mengunduhnya).
    pub thumbnails: Option<PathBuf>,
}

/// Bagian yang sedang diunduh. YouTube sering menyediakan video dan audio terpisah,
/// yang diunduh satu per satu lalu digabung FFmpeg.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Video,
    Audio,
    Whole,
}

/// Tahap pemrosesan oleh yt-dlp setelah unduhan selesai.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Merging,
    ConvertingAudio,
    Remuxing,
    Metadata,
    Fixing,
    Moving,
    Other,
}

/// Posisi video yang sedang diproses, untuk link berisi beberapa video
/// (mis. postingan Instagram berisi 3 video: `index` 1 sampai 3, `total` 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub index: u32,
    pub total: u32,
}

/// Laporan kemajuan untuk ditampilkan.
#[derive(Debug, Clone, PartialEq)]
pub enum Update {
    /// Judul video sudah diketahui.
    Title(String),
    /// Mulai memproses video ke-sekian dari link berisi beberapa video. Laporan berikutnya
    /// (unduh, proses, konversi) berlaku untuk video ini.
    Item(Item),
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
    /// Unduhan selesai, sedang diproses yt-dlp (gabung, ambil audio, pindah file).
    Processing(Step),
    /// Langkah konversi DANload sedang berjalan.
    Converting(convert::Progress),
    /// Menunggu unduhan lain selesai mengonversi (hanya satu konversi berat per waktu).
    WaitingForConversion,
}

/// Satu file yang berhasil disimpan, beserta datanya untuk Pustaka.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedFile {
    /// Lokasi akhir di folder unduhan.
    pub path: PathBuf,
    /// Judul video dari platform (nama file bisa berbeda jika diatur pengguna).
    pub title: String,
    /// Thumbnail kecil di folder thumbnail Pustaka, jika berhasil dibuat.
    pub thumbnail: Option<PathBuf>,
    /// Durasi dalam detik (dari ffprobe).
    pub duration: Option<f64>,
    /// Kualitas sebenarnya, mis. `1080p` atau `192 kbps` ([`library::quality_label`]).
    pub quality: String,
}

/// Hasil akhir sebuah unduhan.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Berhasil. `files`: file hasil (minimal satu), urut seperti di link-nya.
    /// `note`: keterangan jika sebagian item dilewati (mis. foto di postingan Instagram).
    Done {
        files: Vec<SavedFile>,
        note: Option<String>,
    },
    /// Gagal; pesannya siap ditampilkan ke pengguna.
    Failed(String),
    /// Dibatalkan pengguna.
    Cancelled,
}

/// Penghubung untuk membatalkan unduhan (atau pembacaan playlist) dari thread lain, mis. tombol
/// Batal di UI. Saat DANload ditutup, semua dibatalkan bersamaan lalu ditunggu sampai
/// [`CancelHandle::is_finished`] (lihat `stop_everything` di `src/app.rs`).
#[derive(Debug, Clone, Default)]
pub struct CancelHandle {
    inner: Arc<CancelState>,
}

#[derive(Debug, Default)]
struct CancelState {
    cancelled: AtomicBool,
    /// `true` setelah [`run`] selesai, termasuk membersihkan folder sementara.
    finished: AtomicBool,
    /// PID program yang sedang berjalan (yt-dlp atau FFmpeg).
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

    /// `true` setelah unduhan benar-benar selesai (berhasil, gagal, atau dibatalkan).
    pub fn is_finished(&self) -> bool {
        self.inner.finished.load(Ordering::SeqCst)
    }

    /// Menandai pekerjaannya sudah selesai, termasuk bersih-bersih (lihat [`Self::is_finished`]).
    pub(crate) fn mark_finished(&self) {
        self.inner.finished.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// Menandai batal, lalu mengembalikan PID yang harus dihentikan (jika sedang berjalan).
    fn request_cancel(&self) -> Option<u32> {
        self.inner.cancelled.store(true, Ordering::SeqCst);
        *self.pid()
    }

    /// Mencatat PID yang sedang berjalan. Mengembalikan `true` jika pembatalan sudah
    /// diminta sebelumnya, sehingga proses ini harus langsung dihentikan.
    pub(crate) fn set_running(&self, pid: u32) -> bool {
        let mut slot = self.pid();
        *slot = Some(pid);
        self.is_cancelled()
    }

    pub(crate) fn clear_running(&self) {
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
///
/// `staging`: tempat yt-dlp menaruh file hasilnya, sebelum diberi nama akhir oleh DANload.
pub fn build_args(
    request: &Request,
    tools: &Tools,
    temp_dir: &Path,
    staging: &Path,
) -> Vec<OsString> {
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
        // Dari link video+playlist, unduh videonya saja. Playlist dibaca terpisah
        // (`src/playlist.rs`) lalu tiap videonya diunduh sebagai unduhan sendiri.
        "--no-playlist",
        "--windows-filenames",
        // Tanggal file = waktu diunduh, bukan tanggal upload (memudahkan urutan di Pustaka).
        "--no-mtime",
        // Jangan menulis file milik postingan/playlist-nya (mis. thumbnail postingan), hanya
        // milik tiap video.
        "--no-write-playlist-metafiles",
        "--progress-template",
        DOWNLOAD_TEMPLATE,
        "--progress-template",
        POSTPROCESS_TEMPLATE,
        // Judul ditulis sebagai teks JSON: judul Facebook (teks postingan) bisa berisi baris baru.
        "--print",
        "before_dl:DANLOAD|title|%(title)j",
        // Nomor video untuk link berisi beberapa video (`NA` untuk video tunggal).
        "--print",
        "before_dl:DANLOAD|item|%(playlist_index)s|%(n_entries)s",
        // Lokasi file hasil dan judulnya, sebagai objek JSON.
        "--print",
        "after_move:DANLOAD|file|%(.{filepath,title})j",
    ];
    let mut args: Vec<OsString> = fixed.iter().map(OsString::from).collect();
    if request.thumbnails.is_some() {
        // Thumbnail untuk Pustaka. Jika gagal diambil, yt-dlp hanya memberi peringatan.
        args.push("--write-thumbnail".into());
    }
    args.push("--playlist-items".into());
    args.push(format!(":{MAX_ITEMS}").into());
    args.push("-o".into());
    args.push(output_template().into());
    args.push("-P".into());
    args.push(prefixed("home:", staging));
    args.push("-P".into());
    args.push(prefixed("temp:", temp_dir));
    args.push("--ffmpeg-location".into());
    args.push(tools.ffmpeg_dir().as_os_str().to_owned());
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
    cancel.mark_finished();
    outcome
}

fn run_in(
    request: &Request,
    tools: &Tools,
    temp_dir: &Path,
    cancel: &CancelHandle,
    on_update: &mut dyn FnMut(Update),
) -> Outcome {
    let staging = temp_dir.join(STAGING_DIR_NAME);
    if let Err(error) = fs::create_dir_all(&request.folder).and_then(|()| create_dirs(&staging)) {
        return Outcome::Failed(format!(
            "Folder unduhan tidak bisa dibuat atau ditulisi ({}): {error}. Pilih folder lain di Pengaturan.",
            request.folder.display()
        ));
    }
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    match run_yt_dlp(request, tools, temp_dir, &staging, cancel, on_update) {
        Ok(downloaded) => finish_files(downloaded, request, tools, temp_dir, cancel, on_update),
        Err(outcome) => outcome,
    }
}

/// Seperti `fs::create_dir_all`, tetapi dicoba sekali lagi jika folder induknya hilang di tengah
/// jalan. Dengan beberapa unduhan bersamaan, unduhan lain yang baru selesai menghapus
/// `.danload-tmp` jika folder itu (sesaat) kosong.
fn create_dirs(path: &Path) -> std::io::Result<()> {
    match fs::create_dir_all(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir_all(path),
        result => result,
    }
}

/// Satu file hasil yt-dlp di folder sementara.
#[derive(Debug, Clone, PartialEq)]
struct DownloadedFile {
    path: PathBuf,
    /// Judul videonya, jika dicetak yt-dlp.
    title: Option<String>,
}

/// Hasil yt-dlp yang masih harus diselesaikan DANload.
struct Downloaded {
    /// File hasil di folder sementara, urut seperti di link-nya.
    files: Vec<DownloadedFile>,
    /// Jumlah item di link (untuk link berisi beberapa video), termasuk yang gagal.
    total: Option<u32>,
    /// Pesan error yt-dlp jika ada item yang gagal (sebagian file tetap berhasil).
    failure: Option<String>,
}

/// Menjalankan yt-dlp sampai selesai. `Err` berisi hasil akhir jika tidak ada file yang
/// bisa diselesaikan (gagal atau dibatalkan).
fn run_yt_dlp(
    request: &Request,
    tools: &Tools,
    temp_dir: &Path,
    staging: &Path,
    cancel: &CancelHandle,
    on_update: &mut dyn FnMut(Update),
) -> Result<Downloaded, Outcome> {
    let mut command = Command::new(&tools.yt_dlp);
    command
        .args(build_args(request, tools, temp_dir, staging))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process::run_in_background(&mut command);
    let mut child = command.spawn().map_err(|error| {
        Outcome::Failed(format!(
            "yt-dlp tidak bisa dijalankan: {error}. Coba pasang ulang lewat Pengaturan."
        ))
    })?;
    if cancel.set_running(child.id()) {
        process::kill_tree(child.id());
    }

    // Baca stdout dan stderr bersamaan di dua thread; barisnya dikumpulkan lewat channel.
    let (sender, receiver) = mpsc::channel::<String>();
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(process::spawn_line_reader(
            stdout,
            sender.clone(),
            String::from,
        ));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(process::spawn_line_reader(
            stderr,
            sender.clone(),
            String::from,
        ));
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
        return Err(Outcome::Cancelled);
    }
    let failure = match status {
        Ok(status) if status.success() => None,
        Ok(status) => Some(output.failure_message(status.code())),
        Err(error) => Some(format!("Gagal menunggu yt-dlp selesai: {error}")),
    };
    if output.files.is_empty() {
        return Err(Outcome::Failed(failure.unwrap_or_else(|| {
            "yt-dlp selesai, tetapi tidak ada file yang dihasilkan.".to_string()
        })));
    }
    Ok(Downloaded {
        files: output.files,
        total: output.total,
        failure,
    })
}

/// Menyelesaikan file hasil yt-dlp satu per satu: dikonversi jika perlu, lalu dipindah ke
/// folder unduhan dengan nama akhirnya.
///
/// Pembatalan dicek sebelum tiap file. File yang sudah dipindah ke folder unduhan tetap
/// dilaporkan (hasilnya `Done`), walaupun pembatalan datang sesudahnya, mis. saat "Batal semua":
/// file itu sudah utuh, jadi harus tercatat di Pustaka, bukan tertinggal tanpa catatan.
fn finish_files(
    downloaded: Downloaded,
    request: &Request,
    tools: &Tools,
    temp_dir: &Path,
    cancel: &CancelHandle,
    on_update: &mut dyn FnMut(Update),
) -> Outcome {
    let count = u32::try_from(downloaded.files.len()).unwrap_or(u32::MAX);
    let mut saved = Vec::new();
    let mut problems = Vec::new();
    // `true` jika berhenti di tengah karena dibatalkan (masih ada file yang belum diproses).
    let mut stopped = false;
    for (index, file) in (1..).zip(&downloaded.files) {
        let source = &file.path;
        if cancel.is_cancelled() {
            stopped = true;
            break;
        }
        if count > 1 {
            on_update(Update::Item(Item {
                index,
                total: count,
            }));
        }
        let ready = match request.format.conversion() {
            Some(target) => {
                match convert_file(source, target, tools, temp_dir, index, cancel, on_update) {
                    Ok(path) => path,
                    Err(ConvertError::Cancelled) => {
                        stopped = true;
                        break;
                    }
                    Err(ConvertError::Failed(message)) => {
                        problems.push(message);
                        continue;
                    }
                }
            }
            None => source.clone(),
        };
        // Nama akhir: pilihan pengguna, atau judul video (nama file dari yt-dlp, sudah aman
        // untuk Windows). Beberapa video dengan nama sama menjadi `nama`, `nama (2)`, dst.
        let stem: OsString = match &request.file_name {
            Some(name) => name.into(),
            None => source
                .file_stem()
                .unwrap_or(OsStr::new("video"))
                .to_os_string(),
        };
        match filename::move_to_unique_name(&ready, &request.folder, &stem) {
            Ok(path) => saved.push(describe_saved(path, file, request, tools)),
            Err(error) => problems.push(format!(
                "File hasil tidak bisa dipindahkan ke folder unduhan: {error}"
            )),
        }
    }
    if saved.is_empty() && cancel.is_cancelled() {
        return Outcome::Cancelled;
    }

    let reason = problems.into_iter().next().or(downloaded.failure);
    if saved.is_empty() {
        return Outcome::Failed(
            reason.unwrap_or_else(|| "Tidak ada file yang berhasil disimpan.".to_string()),
        );
    }
    if stopped {
        let note = format!(
            "Dibatalkan setelah {} dari {count} file tersimpan.",
            saved.len()
        );
        return Outcome::Done {
            files: saved,
            note: Some(note),
        };
    }
    let note = reason.map(|reason| {
        let items = downloaded
            .total
            .unwrap_or(count)
            .max(u32::try_from(saved.len()).unwrap_or(u32::MAX));
        let skipped = items.saturating_sub(u32::try_from(saved.len()).unwrap_or(u32::MAX));
        if skipped > 0 {
            format!("{skipped} item dilewati: {reason}")
        } else {
            format!("Sebagian item dilewati: {reason}")
        }
    });
    Outcome::Done { files: saved, note }
}

/// Melengkapi data file yang sudah tersimpan di `path` untuk Pustaka: durasi dan kualitas
/// sebenarnya (ffprobe), serta thumbnail kecil. Gagal membaca salah satunya tidak membuat
/// unduhan gagal; datanya saja yang kosong.
fn describe_saved(
    path: PathBuf,
    file: &DownloadedFile,
    request: &Request,
    tools: &Tools,
) -> SavedFile {
    let info = convert::probe(&tools.ffprobe, &path).ok();
    let thumbnail = request
        .thumbnails
        .as_deref()
        .and_then(|dir| save_thumbnail(&file.path, dir, &tools.ffmpeg));
    let title = file.title.clone().unwrap_or_else(|| {
        file.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    SavedFile {
        title,
        duration: info.as_ref().and_then(|info| info.duration),
        quality: library::quality_label(request.format, info.as_ref()),
        thumbnail,
        path,
    }
}

/// Thumbnail yang diunduh yt-dlp untuk `source` (gambar di folder yang sama di folder
/// sementara), dikecilkan dengan FFmpeg menjadi JPEG baru di `dir`. `None` jika tidak ada.
fn save_thumbnail(source: &Path, dir: &Path, ffmpeg: &Path) -> Option<PathBuf> {
    let is_image = |path: &Path| {
        path.extension().is_some_and(|extension| {
            ["jpg", "jpeg", "png", "webp"]
                .iter()
                .any(|image| extension.eq_ignore_ascii_case(image))
        })
    };
    let image = fs::read_dir(source.parent()?)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.as_path() != source && is_image(path))?;
    fs::create_dir_all(dir).ok()?;
    let target = library::new_thumbnail_path(dir);
    match convert::make_thumbnail(ffmpeg, &image, &target) {
        Ok(()) => Some(target),
        Err(_) => {
            let _ = fs::remove_file(&target);
            None
        }
    }
}

/// Membuat versi `target` dari `source` (hasil yt-dlp). Mengembalikan lokasi file hasilnya,
/// masih di folder sementara; jika `source` sudah berformat `target` (mis. VP9 + Opus langsung
/// digabung yt-dlp ke WEBM), `source` itu sendiri tanpa FFmpeg.
fn convert_file(
    source: &Path,
    target: VideoFormat,
    tools: &Tools,
    temp_dir: &Path,
    index: u32,
    cancel: &CancelHandle,
    on_update: &mut dyn FnMut(Update),
) -> Result<PathBuf, ConvertError> {
    if source
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(target.extension()))
    {
        return Ok(source.to_path_buf());
    }
    // Nama pendek: aman dari batas panjang path Windows dan dari nama kembar.
    let converted = temp_dir.join(format!("konversi-{index}.{}", target.extension()));

    // Kalau ffprobe gagal, konversi tetap dicoba dengan mengubah ulang semua stream.
    let info = convert::probe(&tools.ffprobe, source).ok();
    if info.as_ref().is_some_and(|info| info.video.is_none()) {
        return Err(ConvertError::Failed(format!(
            "File yang diunduh tidak berisi gambar video, jadi tidak bisa dijadikan {}. Coba mode Audio.",
            target.name()
        )));
    }
    let plan = convert::plan(target, info.as_ref());
    // Dipegang sampai konversi selesai (dilepas otomatis di akhir fungsi ini).
    let _turn = if plan.copy_only() {
        None
    } else {
        Some(wait_for_turn(&CONVERSION_TURN, cancel, on_update)?)
    };
    let mut progress = convert::Progress {
        target,
        copy_only: plan.copy_only(),
        processed: 0.0,
        duration: info.as_ref().and_then(|info| info.duration),
        speed: None,
    };
    on_update(Update::Converting(progress));
    if cancel.is_cancelled() {
        return Err(ConvertError::Cancelled);
    }

    let args = plan.ffmpeg_args(source, &converted);
    convert::run(
        &tools.ffmpeg,
        &args,
        target,
        cancel,
        &mut |processed, speed| {
            progress.processed = processed;
            progress.speed = speed;
            on_update(Update::Converting(progress));
        },
    )?;
    if cancel.is_cancelled() {
        return Err(ConvertError::Cancelled);
    }
    Ok(converted)
}

/// Menunggu giliran `turn` (lihat [`CONVERSION_TURN`]). Selama menunggu, UI diberi tahu sekali
/// lewat [`Update::WaitingForConversion`]; pembatalan tetap dicek.
fn wait_for_turn(
    turn: &'static Mutex<()>,
    cancel: &CancelHandle,
    on_update: &mut dyn FnMut(Update),
) -> Result<MutexGuard<'static, ()>, ConvertError> {
    let mut announced = false;
    loop {
        match turn.try_lock() {
            Ok(guard) => return Ok(guard),
            // Unduhan lain panik saat memegang giliran; gilirannya tetap bisa dipakai.
            Err(TryLockError::Poisoned(poisoned)) => return Ok(poisoned.into_inner()),
            Err(TryLockError::WouldBlock) => {
                if cancel.is_cancelled() {
                    return Err(ConvertError::Cancelled);
                }
                if !announced {
                    announced = true;
                    on_update(Update::WaitingForConversion);
                }
                std::thread::sleep(TURN_POLL);
            }
        }
    }
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
    /// Setelah pemrosesan sebuah video dimulai, laporan unduhannya yang datang terlambat
    /// diabaikan. Direset saat video berikutnya dimulai.
    processing_started: bool,
    /// File hasil (lokasi setelah dipindah yt-dlp), urut seperti di link-nya.
    files: Vec<DownloadedFile>,
    /// Jumlah item di link berisi beberapa video.
    total: Option<u32>,
    first_error: Option<String>,
    last_other_line: Option<String>,
}

impl OutputState {
    fn handle_line(&mut self, line: &str, on_update: &mut dyn FnMut(Update)) {
        match parse_line(line) {
            Some(Event::Title(title)) => {
                self.flush(on_update);
                self.processing_started = false;
                on_update(Update::Title(title));
            }
            Some(Event::Item(item)) => {
                self.flush(on_update);
                self.processing_started = false;
                self.total = Some(item.total);
                if item.total > 1 {
                    on_update(Update::Item(item));
                }
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
            Some(Event::File(file)) => self.files.push(file),
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
    /// Mulai video ke-sekian dari link berisi beberapa video.
    Item(Item),
    Progress {
        update: Update,
        finished: bool,
    },
    Step(Step),
    File(DownloadedFile),
}

/// Membaca satu baris berawalan `DANLOAD|`. Baris lain menghasilkan `None`.
fn parse_line(line: &str) -> Option<Event> {
    let rest = line.trim_end().strip_prefix(LINE_PREFIX)?;
    let (kind, data) = rest.split_once('|')?;
    match kind {
        "title" => Some(Event::Title(json_text(data))),
        "item" => {
            let (index, total) = data.split_once('|')?;
            let index = number(index)? as u32;
            let total = number(total)? as u32;
            (index >= 1 && total >= 1).then_some(Event::Item(Item { index, total }))
        }
        "file" => parse_file(data.trim()).map(Event::File),
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

/// Teks yang dicetak yt-dlp dengan `%(...)j` (string JSON, mis. `"Judul \u00e9"`). Teks biasa
/// (tanpa tanda kutip) diterima apa adanya.
fn json_text(data: &str) -> String {
    let data = data.trim();
    if data.starts_with('"')
        && let Ok(text) = serde_json::from_str::<String>(data)
    {
        return text;
    }
    data.to_string()
}

/// Isi baris `file`: objek JSON `{"filepath": ..., "title": ...}`, atau lokasi file saja.
fn parse_file(data: &str) -> Option<DownloadedFile> {
    #[derive(serde::Deserialize)]
    struct Printed {
        filepath: Option<String>,
        title: Option<String>,
    }
    if data.starts_with('{') {
        let printed: Printed = serde_json::from_str(data).ok()?;
        let path = printed.filepath.filter(|path| !path.trim().is_empty())?;
        return Some(DownloadedFile {
            path: PathBuf::from(path),
            title: printed.title.filter(|title| !title.trim().is_empty()),
        });
    }
    (!data.is_empty() && data != "NA").then(|| DownloadedFile {
        path: PathBuf::from(data),
        title: None,
    })
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
        "VideoRemuxer" | "VideoConvertor" => Step::Remuxing,
        "Metadata" | "FFmpegMetadata" | "EmbedThumbnail" => Step::Metadata,
        "MoveFiles" | "MoveFilesAfterDownload" => Step::Moving,
        other if other.starts_with("Fixup") => Step::Fixing,
        _ => Step::Other,
    }
}

/// Teks untuk sebuah tahap pemrosesan unduhan berformat `format`.
pub fn describe_step(step: Step, format: OutputFormat) -> String {
    match step {
        Step::Merging => "Menggabungkan video dan audio...".to_string(),
        Step::ConvertingAudio => format!("Mengubah ke {}...", format.name()),
        Step::Remuxing => format!("Menyimpan sebagai {}...", format.name()),
        Step::Metadata => "Menambahkan info judul...".to_string(),
        Step::Fixing => "Merapikan file...".to_string(),
        Step::Moving => "Memindahkan file...".to_string(),
        Step::Other => "Memproses file...".to_string(),
    }
}

/// Mengubah baris error yt-dlp menjadi pesan yang mudah dipahami.
///
/// Pencocokan memakai seluruh baris (huruf kecil); jika tidak ada yang cocok,
/// pesan aslinya ditampilkan tanpa awalan teknis.
pub fn friendly_error(line: &str) -> String {
    known_error(line).unwrap_or_else(|| format!("Unduhan gagal: {}", error_detail(line)))
}

/// Pesan yang mudah dipahami untuk error yt-dlp yang dikenal, atau `None`.
pub(crate) fn known_error(line: &str) -> Option<String> {
    let lower = line.to_lowercase();
    // Urutan penting: yang lebih spesifik diletakkan di atas.
    let known: &[(&[&str], &str)] = &[
        // ----- Playlist (sejak Fase 7) -----
        (
            &["playlist is private"],
            "Playlist ini privat, jadi isinya tidak bisa dibaca.",
        ),
        (
            &["playlist does not exist", "playlist type is unviewable"],
            "Playlist tidak ditemukan atau tidak bisa dibuka. Mungkin privat, sudah dihapus, atau butuh login.",
        ),
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
        // Pesan Facebook ini sering berisi "isn't available", jadi dicek sebelum entri umum.
        (
            &["the video is not available, facebook said"],
            "Video ini tidak tersedia menurut Facebook. Mungkin sudah dihapus atau privat.",
        ),
        (
            &[
                "video unavailable",
                "video is unavailable",
                "has been removed",
                "no longer available",
                "isn't available",
                "may have been removed",
            ],
            "Video tidak tersedia. Mungkin sudah dihapus atau dibatasi pemiliknya.",
        ),
        // ----- Instagram dan Facebook (teks dari kode sumber yt-dlp 2026.10.26) -----
        // Pesan-pesan ini sering ikut menyarankan `--cookies`, jadi harus dicek sebelum
        // pesan umum "butuh login" di bawah.
        (
            &["rate-limit", "rate limit"],
            "Instagram sedang membatasi akses tanpa login. Tunggu beberapa saat (bisa sampai beberapa jam), lalu coba lagi.",
        ),
        (
            &["who follow this account"],
            "Akun ini privat; postingannya hanya bisa dilihat pengikutnya.",
        ),
        (
            &["empty media response"],
            "Instagram tidak memberikan data postingan ini. Pastikan postingan bisa dibuka di browser tanpa login, lalu coba lagi nanti.",
        ),
        (
            &["there is no video in this post", "no video formats found"],
            "Tidak ada video di link ini (mungkin hanya berisi foto).",
        ),
        (
            &["cannot parse data"],
            "Facebook tidak memberikan data video ini. Mungkin videonya privat atau butuh login, atau yt-dlp perlu diperbarui lewat Pengaturan.",
        ),
        (
            &[
                "only available for registered users",
                "you need to log in",
                "log in to",
                "login required",
                "--cookies",
            ],
            "Konten ini butuh login (akun), jadi belum bisa diunduh DANload.",
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
    known
        .iter()
        .find(|(needles, _)| needles.iter().any(|needle| lower.contains(needle)))
        .map(|(_, message)| (*message).to_string())
}

/// Isi pesan error tanpa awalan teknis, paling banyak 220 karakter.
pub(crate) fn error_detail(line: &str) -> String {
    strip_error_prefix(line).chars().take(220).collect()
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

/// Menambahkan nomor video untuk link berisi beberapa video, mis.
/// `Mengunduh video · 45%` → `Mengunduh video (2/3) · 45%`. Tanpa `item` (atau jika
/// link-nya hanya berisi satu video), teksnya tidak diubah.
pub fn with_item(text: &str, item: Option<Item>) -> String {
    match item.filter(|item| item.total > 1) {
        Some(Item { index, total }) => match text.split_once(" · ") {
            Some((head, rest)) => format!("{head} ({index}/{total}) · {rest}"),
            None => match text.strip_suffix("...") {
                Some(head) => format!("{head} ({index}/{total})..."),
                None => format!("{text} ({index}/{total})"),
            },
        },
        None => text.to_string(),
    }
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
    use crate::deps;
    use crate::formats::AudioFormat;

    fn mp4(max_height: Option<u32>) -> OutputFormat {
        OutputFormat::Video {
            format: VideoFormat::Mp4,
            max_height,
        }
    }

    fn mp3() -> OutputFormat {
        OutputFormat::Audio {
            format: AudioFormat::Mp3,
            bitrate_kbps: 192,
        }
    }

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
    fn parses_title_item_file_and_steps() {
        assert_eq!(
            parse_line("DANLOAD|title|Judul | dengan garis | tegak"),
            Some(Event::Title("Judul | dengan garis | tegak".to_string()))
        );
        assert_eq!(
            parse_line("DANLOAD|item|2|3"),
            Some(Event::Item(Item { index: 2, total: 3 }))
        );
        // Video tunggal: yt-dlp mencetak NA.
        assert_eq!(parse_line("DANLOAD|item|NA|NA"), None);
        assert_eq!(parse_line("DANLOAD|item|0|3"), None);
        // Baris asli yt-dlp 2026.10.26: `|` di judul sudah diganti `｜` oleh --windows-filenames.
        let plain = |path: &str| {
            Some(Event::File(DownloadedFile {
                path: PathBuf::from(path),
                title: None,
            }))
        };
        assert_eq!(
            parse_line("DANLOAD|file|/tmp/job/hasil/abc/Uji é 日本 ｜ garis.mkv"),
            plain("/tmp/job/hasil/abc/Uji é 日本 ｜ garis.mkv")
        );
        assert_eq!(
            parse_line("DANLOAD|file|/tmp/out/vidéo ü 日本 [abc].mp3\r"),
            plain("/tmp/out/vidéo ü 日本 [abc].mp3")
        );
        assert_eq!(parse_line("DANLOAD|file|NA"), None);
        // Baris asli yt-dlp 2026.10.26 (`%(.{filepath,title})j`): huruf non-ASCII ditulis \uXXXX.
        assert_eq!(
            parse_line(
                r#"DANLOAD|file|{"filepath": "/tmp/t6/hasil/item1/Video by akun_uji \u00e9.mp4", "title": "Liburan \ud83c\udfd6\ufe0f | baris\nkedua"}"#
            ),
            Some(Event::File(DownloadedFile {
                path: PathBuf::from("/tmp/t6/hasil/item1/Video by akun_uji é.mp4"),
                title: Some("Liburan 🏖️ | baris\nkedua".to_string()),
            }))
        );
        assert_eq!(
            parse_line(r#"DANLOAD|file|{"title": "tanpa lokasi"}"#),
            None
        );
        // Judul sebagai teks JSON (bisa berisi baris baru), atau teks biasa.
        assert_eq!(
            parse_line(r#"DANLOAD|title|"9,7 rb tayangan \u00b7 352 reaksi\nAkun""#),
            Some(Event::Title(
                "9,7 rb tayangan · 352 reaksi\nAkun".to_string()
            ))
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
            parse_line("DANLOAD|pp|started|VideoRemuxer"),
            Some(Event::Step(Step::Remuxing))
        );
        assert_eq!(
            parse_line("DANLOAD|pp|started|FixupM4a"),
            Some(Event::Step(Step::Fixing))
        );
        assert_eq!(parse_line("[download] 45.3% of 10.00MiB"), None);
        assert_eq!(parse_line("DANLOAD|dl|downloading|1|2"), None);
    }

    #[test]
    fn step_text_names_the_format() {
        let flac = OutputFormat::Audio {
            format: AudioFormat::Flac,
            bitrate_kbps: 192,
        };
        assert_eq!(
            describe_step(Step::ConvertingAudio, flac),
            "Mengubah ke FLAC..."
        );
        assert_eq!(
            describe_step(
                Step::Remuxing,
                OutputFormat::Video {
                    format: VideoFormat::Mkv,
                    max_height: None
                }
            ),
            "Menyimpan sebagai MKV..."
        );
    }

    #[test]
    fn item_number_is_added_to_progress_text() {
        let second = Some(Item { index: 2, total: 3 });
        assert_eq!(
            with_item("Mengunduh video · 45% · sisa 0:12", second),
            "Mengunduh video (2/3) · 45% · sisa 0:12"
        );
        assert_eq!(
            with_item("Menggabungkan video dan audio...", second),
            "Menggabungkan video dan audio (2/3)..."
        );
        assert_eq!(with_item("Menyiapkan", second), "Menyiapkan (2/3)");
        // Video tunggal: teks tidak berubah.
        assert_eq!(with_item("Mengunduh · 10%", None), "Mengunduh · 10%");
        assert_eq!(
            with_item("Mengunduh · 10%", Some(Item { index: 1, total: 1 })),
            "Mengunduh · 10%"
        );
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
    fn friendly_errors_for_facebook_and_instagram() {
        // Teks dari kode sumber yt-dlp 2026.10.26, dirangkai seperti yang dicetaknya.
        let login_hint = "Use --cookies-from-browser or --cookies for the authentication. See  https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp  for how to manually pass cookies";
        let cases = [
            (
                format!("ERROR: [Instagram] C0abc: The webpage request was redirected to the login page. You have exceeded the rate-limit for accessing posts anonymously. {login_hint}"),
                "Instagram sedang membatasi akses",
            ),
            (
                format!("ERROR: [Instagram] C0abcdefghijklmnopqrstuvwxyz0123456789: This content is only available for registered users who follow this account. {login_hint}"),
                "Akun ini privat",
            ),
            (
                format!("ERROR: [Instagram] C0abc: Instagram sent an empty media response. Check if this post is accessible in your browser without being logged-in. If it is not, then u{}. Otherwise, if the post is accessible in browser without being logged-in, please report this issue on  https://github.com/yt-dlp/yt-dlp/issues?q= ", &login_hint[1..]),
                "Instagram tidak memberikan data",
            ),
            (
                "ERROR: [Instagram] C0abc: There is no video in this post".to_string(),
                "Tidak ada video",
            ),
            (
                "ERROR: [Instagram] 3456: No video formats found!; please report this issue on  https://github.com/yt-dlp/yt-dlp/issues?q= , filling out the appropriate issue template. Confirm you are on the latest version using  yt-dlp -U".to_string(),
                "Tidak ada video",
            ),
            (
                format!("ERROR: [Instagram] C0abc: Restricted Video: You must be 18 years old or over to see this video. {login_hint}"),
                "butuh login",
            ),
            (
                "ERROR: [Instagram] C0abc: Post isn't available: The link you followed may be broken, or the page may have been removed.".to_string(),
                "Video tidak tersedia",
            ),
            (
                "ERROR: [facebook] 1234567890: Cannot parse data; please report this issue on  https://github.com/yt-dlp/yt-dlp/issues?q= , filling out the appropriate issue template. Confirm you are on the latest version using  yt-dlp -U".to_string(),
                "Facebook tidak memberikan data",
            ),
            (
                "ERROR: [facebook] 1234567890: The video is not available, Facebook said: \"This content isn't available right now\"".to_string(),
                "tidak tersedia menurut Facebook",
            ),
            (
                "ERROR: [facebook] 1234567890: This video is only available for registered users. Use --cookies, --cookies-from-browser, --username and --password, --netrc-cmd, or --netrc (facebook) to provide account credentials. See  https://github.com/yt-dlp/yt-dlp/wiki/FAQ#how-do-i-pass-cookies-to-yt-dlp  for how to manually pass cookies".to_string(),
                "butuh login",
            ),
            (
                "ERROR: Unsupported URL: https://www.facebook.com/halaman.saja".to_string(),
                "Link ini tidak didukung",
            ),
            // YouTube tetap mendapat pesannya sendiri walau ikut menyarankan --cookies.
            (
                format!("ERROR: [youtube] abc: Sign in to confirm you're not a bot. {login_hint}"),
                "bukan bot",
            ),
            (
                format!("ERROR: [youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users. {login_hint}"),
                "dibatasi umur",
            ),
        ];
        for (line, expected) in cases {
            let message = friendly_error(&line);
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

    fn windows_tools() -> Tools {
        Tools {
            yt_dlp: PathBuf::from("C:\\bin\\yt-dlp.exe"),
            ffmpeg: PathBuf::from("C:\\bin\\ffmpeg.exe"),
            ffprobe: PathBuf::from("C:\\bin\\ffprobe.exe"),
            deno: PathBuf::from("C:\\Program Files\\deno.exe"),
        }
    }

    #[test]
    fn args_put_url_last_after_double_dash() {
        let request = Request {
            url: "  https://youtu.be/abc  ".into(),
            format: mp3(),
            folder: PathBuf::from("D:\\Unduhan Saya\\Audio"),
            file_name: None,
            thumbnails: None,
        };
        let args = build_args(
            &request,
            &windows_tools(),
            Path::new("D:\\Unduhan Saya\\Audio\\.danload-tmp\\1"),
            Path::new("D:\\Unduhan Saya\\Audio\\.danload-tmp\\1\\hasil"),
        );
        let args: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args[args.len() - 2..], ["--", "https://youtu.be/abc"]);
        // yt-dlp menaruh hasil di folder sementara; nama akhirnya diberikan DANload.
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-P", "home:D:\\Unduhan Saya\\Audio\\.danload-tmp\\1\\hasil"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-o", "%(id)s/%(title).100s.%(ext)s"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--playlist-items", ":20"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-P", "temp:D:\\Unduhan Saya\\Audio\\.danload-tmp\\1"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--js-runtimes", "deno:C:\\Program Files\\deno.exe"])
        );
        assert!(
            args.contains(&"before_dl:DANLOAD|item|%(playlist_index)s|%(n_entries)s".to_string())
        );
        assert!(args.contains(&"--ignore-config".to_string()));
    }

    #[cfg(windows)]
    #[test]
    fn ffmpeg_location_is_the_folder() {
        let request = Request {
            url: "https://youtu.be/abc".into(),
            format: mp3(),
            folder: PathBuf::from("D:\\Unduhan"),
            file_name: None,
            thumbnails: None,
        };
        let args = build_args(
            &request,
            &windows_tools(),
            Path::new("D:\\Unduhan\\.danload-tmp\\1"),
            &request.folder,
        );
        let args: Vec<String> = args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--ffmpeg-location", "C:\\bin"])
        );
    }

    /// Pembatalan di tengah langkah akhir: file yang sudah ada di folder unduhan tetap dilaporkan
    /// (supaya tercatat di Pustaka), file berikutnya tidak diproses.
    #[cfg(unix)]
    #[test]
    fn files_saved_before_cancel_are_kept() {
        let (tools, dir) = fake_tools("batal-akhir", "#!/bin/sh\nexit 0\n");
        let run_case = |cancel_at: Option<u32>| {
            let case = dir.join(format!("kasus-{cancel_at:?}"));
            let staging = case.join("hasil");
            let folder = case.join("unduhan");
            fs::create_dir_all(&staging).unwrap();
            fs::create_dir_all(&folder).unwrap();
            let files = (1..=2)
                .map(|n| {
                    let path = staging.join(format!("Video {n}.mp4"));
                    fs::write(&path, format!("isi {n}")).unwrap();
                    DownloadedFile {
                        path,
                        title: Some(format!("Video {n}")),
                    }
                })
                .collect();
            let request = Request {
                url: "https://contoh".into(),
                format: OutputFormat::Video {
                    format: VideoFormat::Mp4,
                    max_height: None,
                },
                folder: folder.clone(),
                file_name: None,
                thumbnails: None,
            };
            let cancel = CancelHandle::default();
            if cancel_at == Some(0) {
                cancel.cancel();
            }
            let canceller = cancel.clone();
            let downloaded = Downloaded {
                files,
                total: Some(2),
                failure: None,
            };
            let outcome = finish_files(
                downloaded,
                &request,
                &tools,
                &case,
                &cancel,
                &mut |update| {
                    // Batal dikirim saat item ke-`n` mulai diproses.
                    if let Update::Item(item) = update
                        && Some(item.index) == cancel_at
                    {
                        canceller.cancel();
                    }
                },
            );
            let mut names: Vec<String> = fs::read_dir(&folder)
                .map(|entries| {
                    entries
                        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            names.sort();
            (outcome, names)
        };

        // Batal sebelum apa pun dipindah: dibatalkan, folder unduhan kosong.
        let (outcome, names) = run_case(Some(0));
        assert_eq!(outcome, Outcome::Cancelled);
        assert!(names.is_empty(), "{names:?}");

        // Batal saat file pertama diproses: file pertama tetap tersimpan dan dilaporkan.
        let (outcome, names) = run_case(Some(1));
        assert_eq!(names, ["Video 1.mp4"]);
        let Outcome::Done { files, note } = outcome else {
            panic!("file yang sudah tersimpan harus dilaporkan: {outcome:?}");
        };
        assert_eq!(files.len(), 1);
        assert_eq!(
            note.as_deref(),
            Some("Dibatalkan setelah 1 dari 2 file tersimpan.")
        );

        // Batal saat file terakhir diproses: semuanya sudah tersimpan, jadi selesai biasa.
        let (outcome, names) = run_case(Some(2));
        assert_eq!(names, ["Video 1.mp4", "Video 2.mp4"]);
        assert!(matches!(outcome, Outcome::Done { files, note: None } if files.len() == 2));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn heavy_conversions_take_turns() {
        static TURN: Mutex<()> = Mutex::new(());
        let first = wait_for_turn(&TURN, &CancelHandle::default(), &mut |_| {}).unwrap();

        // Unduhan kedua menunggu sampai giliran dilepas, dan memberi tahu UI sekali saja.
        let (sender, receiver) = mpsc::channel();
        let waiter = std::thread::spawn(move || {
            let mut updates = Vec::new();
            let guard = wait_for_turn(&TURN, &CancelHandle::default(), &mut |update| {
                updates.push(update);
                let _ = sender.send(());
            });
            drop(guard);
            updates
        });
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        std::thread::sleep(TURN_POLL * 2);
        assert!(!waiter.is_finished(), "harus menunggu giliran");
        drop(first);
        assert_eq!(waiter.join().unwrap(), [Update::WaitingForConversion]);

        // Batal saat menunggu: berhenti tanpa mendapat giliran.
        let holder = wait_for_turn(&TURN, &CancelHandle::default(), &mut |_| {}).unwrap();
        let cancel = CancelHandle::default();
        let canceller = cancel.clone();
        let waiter =
            std::thread::spawn(move || wait_for_turn(&TURN, &cancel, &mut |_| {}).map(drop));
        std::thread::sleep(TURN_POLL);
        canceller.cancel();
        assert!(matches!(
            waiter.join().unwrap(),
            Err(ConvertError::Cancelled)
        ));
        drop(holder);
        assert!(wait_for_turn(&TURN, &CancelHandle::default(), &mut |_| {}).is_ok());
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

    #[test]
    fn several_items_are_collected_and_each_reports_progress() {
        let mut state = OutputState::default();
        let mut updates = Vec::new();
        for line in [
            "DANLOAD|title|Video by akun",
            "DANLOAD|item|1|3",
            "DANLOAD|dl|finished|300|300|NA|NA|NA|avc1|mp4a",
            "DANLOAD|pp|started|Merger",
            "DANLOAD|file|/tmp/hasil/1/Video by akun.mp4",
            "DANLOAD|title|Video by akun",
            "DANLOAD|item|2|3",
            "DANLOAD|dl|downloading|100|300|NA|NA|NA|avc1|mp4a",
            "DANLOAD|file|/tmp/hasil/2/Video by akun.mp4",
            "ERROR: [Instagram] 3: No video formats found!",
        ] {
            state.handle_line(line, &mut |update| updates.push(update));
        }
        state.flush(&mut |update| updates.push(update));

        let paths: Vec<&Path> = state.files.iter().map(|file| file.path.as_path()).collect();
        assert_eq!(
            paths,
            [
                Path::new("/tmp/hasil/1/Video by akun.mp4"),
                Path::new("/tmp/hasil/2/Video by akun.mp4")
            ]
        );
        assert_eq!(state.total, Some(3));
        assert!(updates.contains(&Update::Item(Item { index: 2, total: 3 })));
        // Progress video ke-2 tetap dilaporkan walau video ke-1 sudah sampai tahap proses.
        assert!(updates.iter().any(|update| matches!(
            update,
            Update::Downloading {
                downloaded: 100,
                ..
            }
        )));
        assert_eq!(
            state.failure_message(Some(1)),
            "Tidak ada video di link ini (mungkin hanya berisi foto)."
        );
    }

    #[test]
    fn cancel_before_start_is_remembered() {
        let handle = CancelHandle::default();
        handle.cancel();
        assert!(handle.is_cancelled());
        assert!(handle.set_running(12345));
        handle.clear_running();
    }

    // ----- Uji menyeluruh dengan program palsu (skrip shell, hanya di Linux/macOS) -----

    /// Membuat folder uji berisi yt-dlp palsu (`script`) dan ffprobe/ffmpeg palsu.
    #[cfg(unix)]
    fn fake_tools(name: &str, script: &str) -> (Tools, PathBuf) {
        let dir = std::env::temp_dir().join(format!("danload-dl-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        write_program(&dir.join("yt-dlp"), script);
        write_program(&dir.join("ffprobe"), FAKE_FFPROBE);
        write_program(&dir.join("ffmpeg"), FAKE_FFMPEG);
        let tools = Tools {
            yt_dlp: dir.join("yt-dlp"),
            ffmpeg: dir.join("ffmpeg"),
            ffprobe: dir.join("ffprobe"),
            deno: dir.join("deno"),
        };
        (tools, dir)
    }

    #[cfg(unix)]
    fn write_program(path: &Path, script: &str) {
        use std::os::unix::fs::PermissionsExt;
        fs::write(path, script).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
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

    /// ffprobe palsu: isi JSON-nya diambil dari file `probe.json` di sebelahnya.
    #[cfg(unix)]
    const FAKE_FFPROBE: &str = "#!/bin/sh\ncat \"$(dirname \"$0\")/probe.json\"\n";

    /// ffmpeg palsu: mencatat argumennya, melaporkan kemajuan seperti `-progress pipe:1`,
    /// lalu membuat file keluaran (argumen terakhir, tanpa awalan `file:`).
    /// Perilakunya diatur lewat file `ffmpeg-mode` di sebelahnya: `ok`, `lambat`, atau `gagal`.
    #[cfg(unix)]
    const FAKE_FFMPEG: &str = r#"#!/bin/sh
dir="$(dirname "$0")"
printf '%s\n' "$@" > "$dir/ffmpeg-args.txt"
for last in "$@"; do :; done
out="${last#file:}"
mode="$(cat "$dir/ffmpeg-mode" 2>/dev/null || echo ok)"
if [ "$mode" = "gagal" ]; then
  echo "Unknown encoder 'libmp3lame'" >&2
  exit 1
fi
if [ "$mode" = "lambat" ]; then
  : > "$out"
  t=0
  while true; do t=$((t+100000)); printf 'out_time_us=%s\nspeed=0.1x\nprogress=continue\n' $t; sleep 0.05; done
fi
for t in 2500000 5000000 7500000; do printf 'out_time_us=%s\nspeed=2.0x\nprogress=continue\n' $t; sleep 0.05; done
echo "hasil konversi" > "$out"
printf 'out_time_us=10000000\nspeed=2.0x\nprogress=end\n'
"#;

    /// yt-dlp palsu yang "menggabungkan" ke MKV di folder `home:` (dipakai jalur konversi).
    #[cfg(unix)]
    fn merging_script() -> String {
        format!(
            "{READ_PATHS}{}",
            r#"echo "DANLOAD|title|Video Uji é"
for n in 100 200 300; do echo "DANLOAD|dl|downloading|$n|300|NA|1000.0|1|vp9|none"; done
echo "DANLOAD|dl|finished|300|300|NA|NA|NA|vp9|none"
sleep 0.3
echo "DANLOAD|pp|started|Merger" >&2
mkdir -p "$home/abc"
echo "isi mkv" > "$home/abc/Video Uji é.mkv"
echo "DANLOAD|file|$home/abc/Video Uji é.mkv"
"#
        )
    }

    #[cfg(unix)]
    const VP9_OPUS_PROBE: &str = r#"{"streams":[{"codec_type":"video","codec_name":"vp9","pix_fmt":"yuv420p"},{"codec_type":"audio","codec_name":"opus"}],"format":{"duration":"10.000000"}}"#;

    #[cfg(unix)]
    const SUCCESS_SCRIPT: &str = r#"echo "DANLOAD|title|Video Uji é"
echo "DANLOAD|item|NA|NA"
touch "$temp/Video Uji é.f137.mp4.part"
for n in 100 200 300; do echo "DANLOAD|dl|downloading|$n|300|NA|1000.0|1|avc1|none"; done
echo "DANLOAD|dl|finished|300|300|NA|NA|NA|avc1|none"
# Seperti yt-dlp asli: ada jeda antara selesai unduh dan mulai FFmpeg. Tanpa jeda ini,
# urutan stdout/stderr bisa tertukar (dua saluran terpisah) dan test jadi tidak pasti.
sleep 0.3
echo "DANLOAD|pp|started|Merger" >&2
mkdir -p "$home/abc"
echo "isi" > "$home/abc/Video Uji é.mp4"
echo "DANLOAD|file|$home/abc/Video Uji é.mp4"
"#;

    #[cfg(unix)]
    fn request_for(folder: &Path, format: OutputFormat, file_name: Option<&str>) -> Request {
        Request {
            url: "https://youtu.be/abc".into(),
            format,
            folder: folder.to_path_buf(),
            file_name: file_name.map(str::to_string),
            thumbnails: None,
        }
    }

    /// Lokasi file hasil dan keterangan dari unduhan yang berhasil (`None` jika tidak berhasil).
    fn done_paths(outcome: &Outcome) -> Option<(Vec<PathBuf>, Option<String>)> {
        match outcome {
            Outcome::Done { files, note } => Some((
                files.iter().map(|file| file.path.clone()).collect(),
                note.clone(),
            )),
            _ => None,
        }
    }

    /// Nama file di `folder`, urut abjad.
    #[cfg(unix)]
    fn names_in(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(folder)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    #[test]
    fn run_success_reports_progress_file_and_cleans_temp() {
        let (tools, dir) = fake_tools("berhasil", &format!("{READ_PATHS}{SUCCESS_SCRIPT}"));
        let folder = dir.join("Unduhan Saya").join("Video");
        let request = request_for(&folder, mp4(None), None);
        let mut updates = Vec::new();
        let handle = CancelHandle::default();

        let outcome = run(&request, &tools, 1, &handle, &mut |update| {
            updates.push(update)
        });

        // Nama file = judul video, tanpa ID.
        let expected = folder.join("Video Uji é.mp4");
        assert_eq!(done_paths(&outcome), Some((vec![expected.clone()], None)));
        let Outcome::Done { files, .. } = &outcome else {
            unreachable!()
        };
        // Tanpa folder thumbnail dan tanpa ffprobe yang berhasil: datanya kosong, tetap berhasil.
        assert_eq!(files[0].title, "Video Uji é");
        assert_eq!(files[0].thumbnail, None);
        assert_eq!(files[0].quality, "");
        assert_eq!(fs::read_to_string(&expected).unwrap(), "isi\n");
        assert_eq!(updates.first(), Some(&Update::Title("Video Uji é".into())));
        assert!(updates.contains(&Update::Processing(Step::Merging)));
        assert!(
            !updates
                .iter()
                .any(|update| matches!(update, Update::Item(_)))
        );
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
    fn same_name_gets_a_number_and_custom_name_is_used() {
        let (tools, dir) = fake_tools("nama-sama", &format!("{READ_PATHS}{SUCCESS_SCRIPT}"));
        let folder = dir.join("out");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("Video Uji é.mp4"), "lama").unwrap();
        let cancel = CancelHandle::default;

        let second = run(
            &request_for(&folder, mp4(None), None),
            &tools,
            2,
            &cancel(),
            &mut |_| {},
        );
        let third = run(
            &request_for(&folder, mp4(None), None),
            &tools,
            3,
            &cancel(),
            &mut |_| {},
        );
        let custom = run(
            &request_for(&folder, mp4(None), Some("Nama Pilihan")),
            &tools,
            4,
            &cancel(),
            &mut |_| {},
        );
        let custom_again = run(
            &request_for(&folder, mp4(None), Some("Nama Pilihan")),
            &tools,
            5,
            &cancel(),
            &mut |_| {},
        );

        let done = |name: &str| Some((vec![folder.join(name)], None));
        assert_eq!(done_paths(&second), done("Video Uji é (2).mp4"));
        assert_eq!(done_paths(&third), done("Video Uji é (3).mp4"));
        assert_eq!(done_paths(&custom), done("Nama Pilihan.mp4"));
        assert_eq!(done_paths(&custom_again), done("Nama Pilihan (2).mp4"));
        // File lama tidak pernah ditimpa.
        assert_eq!(
            fs::read_to_string(folder.join("Video Uji é.mp4")).unwrap(),
            "lama"
        );
        assert_eq!(
            names_in(&folder),
            [
                "Nama Pilihan (2).mp4",
                "Nama Pilihan.mp4",
                "Video Uji é (2).mp4",
                "Video Uji é (3).mp4",
                "Video Uji é.mp4"
            ]
        );
    }

    /// yt-dlp palsu untuk satu postingan berisi 3 item: dua video berjudul sama, lalu satu
    /// foto (yt-dlp asli: "No video formats found!" dan kode keluar 1).
    #[cfg(unix)]
    const CAROUSEL_SCRIPT: &str = r#"for i in 1 2; do
  echo "DANLOAD|title|Video by akun"
  echo "DANLOAD|item|$i|3"
  echo "DANLOAD|dl|finished|300|300|NA|NA|NA|avc1|mp4a"
  mkdir -p "$home/id$i"
  echo "video $i" > "$home/id$i/Video by akun.mkv"
  echo "DANLOAD|file|$home/id$i/Video by akun.mkv"
done
echo "ERROR: [Instagram] id3: No video formats found!; please report this issue" >&2
exit 1
"#;

    #[cfg(unix)]
    #[test]
    fn post_with_several_videos_keeps_all_of_them() {
        let (tools, dir) = fake_tools("carousel", &format!("{READ_PATHS}{CAROUSEL_SCRIPT}"));
        let folder = dir.join("out");
        let request = request_for(
            &folder,
            OutputFormat::Video {
                format: VideoFormat::Mkv,
                max_height: None,
            },
            None,
        );
        let mut updates = Vec::new();

        let outcome = run(
            &request,
            &tools,
            6,
            &CancelHandle::default(),
            &mut |update| updates.push(update),
        );

        assert_eq!(
            done_paths(&outcome),
            Some((
                vec![
                    folder.join("Video by akun.mkv"),
                    folder.join("Video by akun (2).mkv")
                ],
                Some(
                    "1 item dilewati: Tidak ada video di link ini (mungkin hanya berisi foto)."
                        .into()
                ),
            ))
        );
        assert_eq!(
            fs::read_to_string(folder.join("Video by akun.mkv")).unwrap(),
            "video 1\n"
        );
        assert_eq!(
            fs::read_to_string(folder.join("Video by akun (2).mkv")).unwrap(),
            "video 2\n"
        );
        assert!(updates.contains(&Update::Item(Item { index: 2, total: 3 })));
        assert!(!folder.join(TEMP_DIR_NAME).exists());
    }

    #[cfg(unix)]
    #[test]
    fn several_videos_are_each_converted_with_numbered_progress() {
        let (tools, dir) = fake_tools("carousel-avi", &format!("{READ_PATHS}{CAROUSEL_SCRIPT}"));
        fs::write(dir.join("probe.json"), VP9_OPUS_PROBE).unwrap();
        let folder = dir.join("out");
        let request = request_for(
            &folder,
            OutputFormat::Video {
                format: VideoFormat::Avi,
                max_height: None,
            },
            Some("Reels Liburan"),
        );
        let mut updates = Vec::new();

        let outcome = run(
            &request,
            &tools,
            7,
            &CancelHandle::default(),
            &mut |update| updates.push(update),
        );

        let Some((files, note)) = done_paths(&outcome) else {
            panic!("harus berhasil: {outcome:?}");
        };
        assert_eq!(
            files,
            vec![
                folder.join("Reels Liburan.avi"),
                folder.join("Reels Liburan (2).avi")
            ]
        );
        assert!(note.is_some_and(|note| note.starts_with("1 item dilewati")));
        assert_eq!(
            names_in(&folder),
            ["Reels Liburan (2).avi", "Reels Liburan.avi"]
        );
        // Saat konversi, nomornya dari jumlah file yang ada (2), bukan jumlah item (3).
        let converting_items: Vec<&Item> = updates
            .iter()
            .filter_map(|update| match update {
                Update::Item(item) if item.total == 2 => Some(item),
                _ => None,
            })
            .collect();
        assert_eq!(
            converting_items,
            [&Item { index: 1, total: 2 }, &Item { index: 2, total: 2 }]
        );
    }

    /// yt-dlp palsu yang ikut menulis thumbnail (`.webp`) di sebelah file hasilnya, seperti
    /// `--write-thumbnail`, dan mencatat argumennya.
    #[cfg(unix)]
    const THUMBNAIL_SCRIPT: &str = r#"printf '%s\n' "$@" > "$(dirname "$0")/yt-dlp-args.txt"
echo 'DANLOAD|title|"Video Uji \u00e9"'
mkdir -p "$home/abc"
echo "isi" > "$home/abc/Video Uji é.mp4"
echo "gambar" > "$home/abc/Video Uji é.webp"
printf 'DANLOAD|file|{"filepath": "%s/abc/Video Uji \\u00e9.mp4", "title": "Judul Asli \\u00e9"}\n' "$home"
"#;

    #[cfg(unix)]
    #[test]
    fn thumbnail_and_details_are_saved_for_the_library() {
        let (tools, dir) = fake_tools("thumbnail", &format!("{READ_PATHS}{THUMBNAIL_SCRIPT}"));
        fs::write(
            dir.join("probe.json"),
            r#"{"streams":[{"codec_type":"video","codec_name":"h264","width":1280,"height":720},{"codec_type":"audio","codec_name":"aac"}],"format":{"duration":"10.000000"}}"#,
        )
        .unwrap();
        let folder = dir.join("out");
        let thumbs = dir.join("cache").join("thumbs");
        let mut request = request_for(&folder, mp4(None), None);
        request.thumbnails = Some(thumbs.clone());

        let outcome = run(&request, &tools, 20, &CancelHandle::default(), &mut |_| {});

        let Outcome::Done { files, note: None } = &outcome else {
            panic!("harus berhasil: {outcome:?}");
        };
        let file = &files[0];
        // Nama file dari yt-dlp, judul asli dari baris JSON-nya.
        assert_eq!(file.path, folder.join("Video Uji é.mp4"));
        assert_eq!(file.title, "Judul Asli é");
        assert_eq!(file.quality, "720p");
        assert_eq!(file.duration, Some(10.0));
        let thumbnail = file.thumbnail.as_ref().expect("thumbnail harus dibuat");
        assert!(thumbnail.starts_with(&thumbs) && thumbnail.is_file());
        // Thumbnail dibuat dari gambar yt-dlp; gambar aslinya tidak ikut ke folder unduhan.
        let ffmpeg_args = fs::read_to_string(dir.join("ffmpeg-args.txt")).unwrap();
        assert!(ffmpeg_args.contains("Video Uji é.webp"), "{ffmpeg_args}");
        assert!(ffmpeg_args.contains("scale="), "{ffmpeg_args}");
        assert_eq!(names_in(&folder), ["Video Uji é.mp4"]);
        let yt_dlp_args = fs::read_to_string(dir.join("yt-dlp-args.txt")).unwrap();
        assert!(yt_dlp_args.lines().any(|arg| arg == "--write-thumbnail"));
    }

    #[cfg(unix)]
    #[test]
    fn failed_thumbnail_does_not_fail_the_download() {
        let (tools, dir) = fake_tools(
            "thumbnail-gagal",
            &format!("{READ_PATHS}{THUMBNAIL_SCRIPT}"),
        );
        fs::write(dir.join("ffmpeg-mode"), "gagal").unwrap();
        let folder = dir.join("out");
        let thumbs = dir.join("thumbs");
        let mut request = request_for(&folder, mp4(None), None);
        request.thumbnails = Some(thumbs.clone());

        let outcome = run(&request, &tools, 21, &CancelHandle::default(), &mut |_| {});

        let Outcome::Done { files, .. } = &outcome else {
            panic!("harus berhasil: {outcome:?}");
        };
        assert_eq!(files[0].thumbnail, None);
        // ffprobe palsu tanpa probe.json: kualitas dan durasi kosong, unduhan tetap berhasil.
        assert_eq!(files[0].quality, "");
        assert_eq!(files[0].duration, None);
        assert!(
            fs::read_dir(&thumbs).map_or(true, |mut entries| entries.next().is_none()),
            "tidak boleh ada thumbnail setengah jadi"
        );
        // Tanpa folder thumbnail: yt-dlp tidak diminta mengunduh thumbnail.
        let plain = request_for(&dir.join("out2"), mp4(None), None);
        run(&plain, &tools, 22, &CancelHandle::default(), &mut |_| {});
        let yt_dlp_args = fs::read_to_string(dir.join("yt-dlp-args.txt")).unwrap();
        assert!(!yt_dlp_args.lines().any(|arg| arg == "--write-thumbnail"));
    }

    #[cfg(unix)]
    #[test]
    fn run_failure_gives_friendly_message() {
        let script = "#!/bin/sh\necho 'WARNING: sesuatu' >&2\necho 'ERROR: [youtube] abc: Private video. Sign in if you have access' >&2\nexit 1\n";
        let (tools, dir) = fake_tools("gagal", script);
        let request = request_for(&dir.join("out"), mp3(), None);

        let outcome = run(&request, &tools, 8, &CancelHandle::default(), &mut |_| {});

        assert_eq!(
            outcome,
            Outcome::Failed("Video ini privat, jadi tidak bisa diunduh.".into())
        );
        assert!(!dir.join("out").join(TEMP_DIR_NAME).exists());
    }

    #[cfg(unix)]
    #[test]
    fn success_without_file_is_reported() {
        let (tools, dir) = fake_tools("tanpa-file", "#!/bin/sh\nexit 0\n");
        let request = request_for(&dir.join("out"), mp3(), None);
        let outcome = run(&request, &tools, 9, &CancelHandle::default(), &mut |_| {});
        assert!(
            matches!(&outcome, Outcome::Failed(message) if message.contains("tidak ada file")),
            "{outcome:?}"
        );
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
        let request = request_for(&folder, mp4(Some(720)), None);
        let handle = CancelHandle::default();
        let canceller = handle.clone();

        let started = Instant::now();
        let outcome = run(&request, &tools, 10, &handle, &mut |update| {
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
        let request = request_for(&blocker.join("anak"), mp3(), None);

        let outcome = run(&request, &tools, 11, &CancelHandle::default(), &mut |_| {});

        assert!(matches!(outcome, Outcome::Failed(message) if message.contains("Folder unduhan")));
    }

    #[cfg(unix)]
    #[test]
    fn conversion_copies_streams_and_moves_result() {
        let (tools, dir) = fake_tools("konversi", &merging_script());
        fs::write(dir.join("probe.json"), VP9_OPUS_PROBE).unwrap();
        let folder = dir.join("out");
        let request = request_for(
            &folder,
            OutputFormat::Video {
                format: VideoFormat::Webm,
                max_height: None,
            },
            None,
        );
        let mut updates = Vec::new();

        let outcome = run(
            &request,
            &tools,
            12,
            &CancelHandle::default(),
            &mut |update| updates.push(update),
        );

        let expected = folder.join("Video Uji é.webm");
        assert_eq!(done_paths(&outcome), Some((vec![expected.clone()], None)));
        assert_eq!(fs::read_to_string(&expected).unwrap(), "hasil konversi\n");
        // File MKV perantara tidak boleh ikut muncul di folder unduhan.
        assert_eq!(names_in(&folder), ["Video Uji é.webm"]);
        // VP9 + Opus cukup disalin ke WEBM.
        let ffmpeg_args = fs::read_to_string(dir.join("ffmpeg-args.txt")).unwrap();
        assert!(ffmpeg_args.contains("-c:v\ncopy\n"), "{ffmpeg_args}");
        assert!(ffmpeg_args.contains("-c:a\ncopy\n"), "{ffmpeg_args}");
        let conversions: Vec<&convert::Progress> = updates
            .iter()
            .filter_map(|update| match update {
                Update::Converting(progress) => Some(progress),
                _ => None,
            })
            .collect();
        assert!(conversions.iter().all(|progress| progress.copy_only));
        assert!(conversions.iter().any(|progress| progress.processed >= 7.5));
        assert_eq!(conversions[0].duration, Some(10.0));
    }

    #[cfg(unix)]
    #[test]
    fn conversion_failure_is_explained() {
        let (tools, dir) = fake_tools("konversi-gagal", &merging_script());
        fs::write(dir.join("probe.json"), VP9_OPUS_PROBE).unwrap();
        fs::write(dir.join("ffmpeg-mode"), "gagal").unwrap();
        let folder = dir.join("out");
        let request = request_for(
            &folder,
            OutputFormat::Video {
                format: VideoFormat::Avi,
                max_height: None,
            },
            None,
        );

        let outcome = run(&request, &tools, 13, &CancelHandle::default(), &mut |_| {});

        assert!(
            matches!(&outcome, Outcome::Failed(message) if message.contains("encoder untuk AVI")),
            "{outcome:?}"
        );
        assert!(!folder.join(TEMP_DIR_NAME).exists());
        assert!(names_in(&folder).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn cancel_during_conversion_cleans_up() {
        let (tools, dir) = fake_tools("konversi-batal", &merging_script());
        fs::write(dir.join("probe.json"), VP9_OPUS_PROBE).unwrap();
        fs::write(dir.join("ffmpeg-mode"), "lambat").unwrap();
        let folder = dir.join("out");
        let request = request_for(
            &folder,
            OutputFormat::Video {
                format: VideoFormat::Mpeg,
                max_height: None,
            },
            None,
        );
        let handle = CancelHandle::default();
        let canceller = handle.clone();

        let started = Instant::now();
        let outcome = run(&request, &tools, 14, &handle, &mut |update| {
            if matches!(update, Update::Converting(progress) if progress.processed > 0.0) {
                canceller.cancel();
            }
        });

        assert_eq!(outcome, Outcome::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(!folder.join(TEMP_DIR_NAME).exists());
        assert!(names_in(&folder).is_empty());
    }

    /// Kodek yang wajar untuk hasil format `format`: (video, audio). `None` = tidak diperiksa.
    fn accepted_codecs(
        format: OutputFormat,
    ) -> (Option<&'static [&'static str]>, &'static [&'static str]) {
        match format {
            OutputFormat::Video { format, .. } => match format {
                VideoFormat::Mp4 => (Some(&["h264", "vp9", "av1"]), &["aac", "opus"]),
                VideoFormat::Mkv => (Some(&["h264", "vp9", "av1"]), &["aac", "opus"]),
                VideoFormat::Webm => (Some(&["vp9", "av1"]), &["opus"]),
                VideoFormat::Mov => (Some(&["h264"]), &["aac"]),
                VideoFormat::Avi => (Some(&["mpeg4"]), &["mp3"]),
                VideoFormat::Mpeg => (Some(&["mpeg2video"]), &["mp2"]),
            },
            OutputFormat::Audio { format, .. } => (
                None,
                match format {
                    AudioFormat::Mp3 => &["mp3"],
                    AudioFormat::M4a => &["aac"],
                    AudioFormat::Opus => &["opus"],
                    AudioFormat::Ogg => &["vorbis"],
                    AudioFormat::Flac => &["flac"],
                    AudioFormat::Wav => &["pcm_s16le"],
                },
            ),
        }
    }

    /// Uji dengan yt-dlp dan FFmpeg sungguhan untuk semua format: unduh, konversi, lalu periksa
    /// kodek hasilnya dengan ffprobe dan pastikan seluruh isinya bisa dibaca FFmpeg tanpa error.
    /// Tidak jalan otomatis; jalankan dengan, misalnya:
    /// `DANLOAD_TEST_YTDLP=... DANLOAD_TEST_FFMPEG_DIR=... DANLOAD_TEST_DENO=...
    ///  DANLOAD_TEST_URL=... cargo test real_yt_dlp_end -- --ignored --nocapture`
    /// `DANLOAD_TEST_HEIGHT` (opsional, bawaan 360) mengatur resolusi maksimum video.
    #[test]
    #[ignore = "butuh yt-dlp dan FFmpeg asli serta URL uji"]
    fn real_yt_dlp_end_to_end() {
        let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} belum diisi"));
        let ffmpeg_dir = PathBuf::from(env("DANLOAD_TEST_FFMPEG_DIR"));
        let program = |name: &str| {
            ffmpeg_dir.join(if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.to_string()
            })
        };
        let tools = Tools {
            yt_dlp: env("DANLOAD_TEST_YTDLP").into(),
            ffmpeg: program("ffmpeg"),
            ffprobe: program("ffprobe"),
            deno: env("DANLOAD_TEST_DENO").into(),
        };
        let height = std::env::var("DANLOAD_TEST_HEIGHT")
            .ok()
            .and_then(|text| text.parse().ok())
            .unwrap_or(360);
        let folder = std::env::temp_dir().join("danload-uji-asli");
        let _ = fs::remove_dir_all(&folder);
        let thumbnails = std::env::temp_dir().join("danload-uji-asli-thumbs");
        let _ = fs::remove_dir_all(&thumbnails);
        let mut formats: Vec<OutputFormat> = VideoFormat::ALL
            .into_iter()
            .map(|format| OutputFormat::Video {
                format,
                max_height: Some(height),
            })
            .collect();
        formats.extend(
            AudioFormat::ALL
                .into_iter()
                .map(|format| OutputFormat::Audio {
                    format,
                    bitrate_kbps: 128,
                }),
        );
        let mut problems = Vec::new();
        for (job_id, format) in (1u64..).zip(formats) {
            let request = Request {
                url: env("DANLOAD_TEST_URL"),
                format,
                folder: folder.clone(),
                file_name: None,
                thumbnails: Some(thumbnails.clone()),
            };
            let mut count = 0;
            let mut converting = 0;
            let started = Instant::now();
            let outcome = run(
                &request,
                &tools,
                job_id,
                &CancelHandle::default(),
                &mut |update| {
                    count += 1;
                    match update {
                        Update::Downloading { .. } => {}
                        Update::Converting(progress) => {
                            converting += 1;
                            if converting == 1 {
                                println!("  {}", convert::describe(&progress).0);
                            }
                        }
                        other => println!("  {other:?}"),
                    }
                },
            );
            let files = match &outcome {
                Outcome::Done { files, note }
                    if !files.is_empty()
                        && files.iter().all(|file| {
                            file.path.is_file()
                                && file
                                    .path
                                    .extension()
                                    .is_some_and(|ext| ext == format.extension())
                        }) =>
                {
                    if let Some(note) = note {
                        println!("  catatan: {note}");
                    }
                    files.clone()
                }
                other => {
                    problems.push(format!("{}: {other:?}", format.label()));
                    continue;
                }
            };
            for file in &files {
                check_result(&tools, format, &file.path, &mut problems);
                // Data untuk Pustaka (Fase 6).
                if file.quality.is_empty() || file.duration.is_none() || file.title.is_empty() {
                    problems.push(format!("{}: data Pustaka kurang: {file:?}", format.label()));
                }
                if let Some(thumbnail) = &file.thumbnail
                    && !(thumbnail.is_file() && thumbnail.starts_with(&thumbnails))
                {
                    problems.push(format!("{}: thumbnail hilang: {file:?}", format.label()));
                }
            }
            if files.iter().all(|file| file.thumbnail.is_none()) {
                problems.push(format!("{}: tidak ada thumbnail", format.label()));
            }
            println!(
                "{} → {} file, {count} laporan ({converting} konversi), {:.1} detik · {}",
                format.label(),
                files.len(),
                started.elapsed().as_secs_f64(),
                files
                    .iter()
                    .map(|file| format!(
                        "{} · {:.1} dtk · thumbnail {}",
                        file.quality,
                        file.duration.unwrap_or_default(),
                        if file.thumbnail.is_some() {
                            "ada"
                        } else {
                            "tidak ada"
                        }
                    ))
                    .collect::<Vec<_>>()
                    .join(" | ")
            );
        }
        assert!(!folder.join(TEMP_DIR_NAME).exists());
        assert!(problems.is_empty(), "{}", problems.join("\n"));

        // Unduh ulang: nama file sama, jadi hasil baru diberi nomor; file lama tidak ditimpa.
        for format in [
            OutputFormat::Video {
                format: VideoFormat::Avi,
                max_height: Some(height),
            },
            OutputFormat::Audio {
                format: AudioFormat::Mp3,
                bitrate_kbps: 128,
            },
        ] {
            let request = Request {
                url: env("DANLOAD_TEST_URL"),
                format,
                folder: folder.clone(),
                file_name: None,
                thumbnails: None,
            };
            let before: Vec<OsString> = fs::read_dir(&folder)
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name())
                .collect();
            let outcome = run(&request, &tools, 99, &CancelHandle::default(), &mut |_| {});
            println!("ulang {} → {outcome:?}", format.label());
            let Outcome::Done { files, .. } = outcome else {
                panic!("unduh ulang gagal");
            };
            for file in &files {
                let name = file.path.file_name().unwrap_or_default();
                assert!(!before.iter().any(|old| old == name), "tertimpa: {file:?}");
                assert!(name.to_string_lossy().contains(" ("), "{file:?}");
            }
        }
        // Nama kustom.
        let request = Request {
            url: env("DANLOAD_TEST_URL"),
            format: OutputFormat::Audio {
                format: AudioFormat::M4a,
                bitrate_kbps: 128,
            },
            folder: folder.clone(),
            file_name: Some("Nama Kustom é".into()),
            thumbnails: None,
        };
        let outcome = run(&request, &tools, 100, &CancelHandle::default(), &mut |_| {});
        println!("nama kustom → {outcome:?}");
        let Outcome::Done { files, .. } = outcome else {
            panic!("unduhan dengan nama kustom gagal");
        };
        assert_eq!(files[0].path, folder.join("Nama Kustom é.m4a"));
        assert!(!folder.join(TEMP_DIR_NAME).exists());
    }

    /// Memeriksa satu file hasil uji asli: kodeknya sesuai format, seluruh isinya bisa dibaca
    /// FFmpeg tanpa error, durasinya masuk akal, dan FLAC-nya 16-bit.
    fn check_result(tools: &Tools, format: OutputFormat, file: &Path, problems: &mut Vec<String>) {
        let label = format!(
            "{} ({})",
            format.label(),
            file.file_name().unwrap_or_default().to_string_lossy()
        );
        let info = match convert::probe(&tools.ffprobe, file) {
            Ok(info) => info,
            Err(error) => {
                problems.push(format!("{label}: ffprobe gagal: {error}"));
                return;
            }
        };
        let (video_ok, audio_ok) = accepted_codecs(format);
        let video = info.video.as_ref().map(|stream| stream.codec.as_str());
        let audio = info.audio.as_ref().map(|stream| stream.codec.as_str());
        let decode = Command::new(&tools.ffmpeg)
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(file)
            .args(["-f", "null", "-"])
            .output()
            .expect("ffmpeg membaca hasil");
        let decode_errors = String::from_utf8_lossy(&decode.stderr).trim().to_string();
        println!(
            "  {label}: video {video:?} · audio {audio:?} · {:.1} s · {} KB",
            info.duration.unwrap_or(0.0),
            fs::metadata(file).map(|m| m.len() / 1000).unwrap_or(0)
        );
        if let Some(accepted) = video_ok
            && !video.is_some_and(|codec| accepted.contains(&codec))
        {
            problems.push(format!("{label}: video {video:?}"));
        }
        if !audio.is_some_and(|codec| audio_ok.contains(&codec)) {
            problems.push(format!("{label}: audio {audio:?}"));
        }
        if video_ok.is_none() && video.is_some() {
            problems.push(format!("{label}: file audio berisi video {video:?}"));
        }
        if !decode.status.success() || !decode_errors.is_empty() {
            problems.push(format!("{label}: tidak terbaca bersih: {decode_errors}"));
        }
        if info.duration.is_none_or(|seconds| seconds < 1.0) {
            problems.push(format!("{label}: durasi {:?}", info.duration));
        }
        if matches!(
            format,
            OutputFormat::Audio {
                format: AudioFormat::Flac,
                ..
            }
        ) {
            let bits = deps::run_capture(
                &tools.ffprobe,
                [
                    OsString::from("-v"),
                    "error".into(),
                    "-show_entries".into(),
                    "stream=bits_per_raw_sample".into(),
                    "-of".into(),
                    "csv=p=0".into(),
                    file.as_os_str().to_owned(),
                ],
                Duration::from_secs(30),
            )
            .unwrap_or_default();
            if bits.trim() != "16" {
                problems.push(format!("{label}: {} bit", bits.trim()));
            }
        }
    }

    /// Batal di tengah konversi dengan FFmpeg sungguhan: proses berhenti, tidak ada file
    /// setengah jadi, folder sementara bersih. Variabel lingkungannya sama dengan uji di atas.
    #[test]
    #[ignore = "butuh yt-dlp dan FFmpeg asli serta URL uji"]
    fn real_cancel_during_conversion() {
        let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} belum diisi"));
        let ffmpeg_dir = PathBuf::from(env("DANLOAD_TEST_FFMPEG_DIR"));
        let suffix = if cfg!(windows) { ".exe" } else { "" };
        let tools = Tools {
            yt_dlp: env("DANLOAD_TEST_YTDLP").into(),
            ffmpeg: ffmpeg_dir.join(format!("ffmpeg{suffix}")),
            ffprobe: ffmpeg_dir.join(format!("ffprobe{suffix}")),
            deno: env("DANLOAD_TEST_DENO").into(),
        };
        let folder = std::env::temp_dir().join("danload-uji-batal");
        let _ = fs::remove_dir_all(&folder);
        let request = Request {
            url: env("DANLOAD_TEST_URL"),
            format: OutputFormat::Video {
                format: VideoFormat::Mpeg,
                max_height: Some(1080),
            },
            folder: folder.clone(),
            file_name: None,
            thumbnails: None,
        };
        let handle = CancelHandle::default();
        let canceller = handle.clone();
        let mut cancelled_at = None;
        let outcome = run(&request, &tools, 1, &handle, &mut |update| {
            if let Update::Converting(progress) = update
                && progress.processed > 0.0
                && cancelled_at.is_none()
            {
                cancelled_at = Some(Instant::now());
                canceller.cancel();
            }
        });
        let stopped_after = cancelled_at.map(|at| at.elapsed());
        println!("{outcome:?}, berhenti {stopped_after:?} setelah Batal");
        assert_eq!(outcome, Outcome::Cancelled);
        assert!(stopped_after.is_some_and(|after| after < Duration::from_secs(5)));
        let leftovers: Vec<_> = fs::read_dir(&folder)
            .map(|entries| entries.flatten().map(|entry| entry.file_name()).collect())
            .unwrap_or_default();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }
}
