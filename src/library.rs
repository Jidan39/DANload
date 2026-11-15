//! Pustaka (CLAUDE.md Bagian 2.8): daftar file yang sudah diunduh lewat DANload.
//!
//! - Datanya disimpan di `%APPDATA%\DANload\library.json` (bukan di folder unduhan), jadi tetap
//!   ada walau folder unduhan diganti. Thumbnail kecil disimpan di `%LOCALAPPDATA%\DANload\thumbs\`.
//! - [`Library`]: isi Pustaka (muat, tambah, ubah, hapus). [`Saver`]: menyimpan di thread lain,
//!   supaya UI tidak tersendat saat file ditulis.
//! - [`query`]: cari, filter, dan urutkan. [`rename_file`]: ganti nama file di disk.
//! - [`check_files`]: memeriksa file mana yang sudah tidak ada (dipindah/dihapus di luar DANload).
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

use std::cmp::Ordering as CmpOrdering;
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::{self, DownloadMode, or_default};
use crate::convert::MediaInfo;
use crate::filename;
use crate::formats::{AudioFormat, AudioQuality, OutputFormat, VideoFormat};
use crate::platform::Platform;
use crate::system::LocalTime;

/// Versi susunan `library.json`. Naikkan jika susunannya berubah tidak cocok dengan yang lama.
const FILE_VERSION: u32 = 1;

/// Nama subfolder thumbnail di `%LOCALAPPDATA%\DANload\`.
const THUMBNAIL_DIR_NAME: &str = "thumbs";

/// Nama bulan untuk tanggal di Pustaka.
const MONTHS: [&str; 12] = [
    "Januari",
    "Februari",
    "Maret",
    "April",
    "Mei",
    "Juni",
    "Juli",
    "Agustus",
    "September",
    "Oktober",
    "November",
    "Desember",
];
const SHORT_MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des",
];

/// Satu file di Pustaka.
///
/// Semua kolom selain `id` dan `path` punya nilai bawaan, supaya `library.json` dari versi lama
/// (atau yang diubah manual) tetap terbaca.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Nomor unik, tidak pernah dipakai ulang.
    pub id: u64,
    /// Lokasi file sekarang (ikut diperbarui saat nama diganti dari DANload).
    pub path: PathBuf,
    /// Judul asli dari platform (bisa berbeda dari nama file jika nama diatur pengguna).
    #[serde(default)]
    pub title: String,
    #[serde(default, deserialize_with = "or_default")]
    pub platform: Platform,
    #[serde(default, deserialize_with = "or_default")]
    pub mode: DownloadMode,
    /// Format saat diunduh, dengan nama yang sama seperti di config.json (mis. `mp4`, `mp3`).
    #[serde(default)]
    pub format: String,
    /// Kualitas sebenarnya hasil unduhan, mis. `1080p` atau `192 kbps`. Kosong jika tidak diketahui.
    #[serde(default)]
    pub quality: String,
    /// Ukuran file dalam byte.
    #[serde(default)]
    pub size: u64,
    /// Durasi dalam detik.
    #[serde(default)]
    pub duration: Option<f64>,
    /// Waktu selesai diunduh, dalam detik sejak 1 Januari 1970 (UTC).
    #[serde(default)]
    pub downloaded_at: i64,
    /// Link yang ditempel pengguna saat mengunduh.
    #[serde(default)]
    pub source_url: String,
    /// Nama file thumbnail di folder thumbnail ([`thumbnail_dir`]), jika ada.
    #[serde(default)]
    pub thumbnail: Option<String>,
}

impl Item {
    /// Nama file tanpa ekstensi, mis. `Judul Video`.
    pub fn name(&self) -> String {
        self.path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Ekstensi file tanpa titik, mis. `mp4`.
    pub fn extension(&self) -> String {
        self.path
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    /// Format video saat diunduh, jika item ini video dengan format yang dikenal.
    pub fn video_format(&self) -> Option<VideoFormat> {
        (self.mode == DownloadMode::Video)
            .then(|| serde_json::from_value(Value::String(self.format.clone())).ok())
            .flatten()
    }

    /// Format audio saat diunduh, jika item ini audio dengan format yang dikenal.
    pub fn audio_format(&self) -> Option<AudioFormat> {
        (self.mode == DownloadMode::Audio)
            .then(|| serde_json::from_value(Value::String(self.format.clone())).ok())
            .flatten()
    }

    /// Nama format untuk ditampilkan, mis. `MP4` atau `MPEG`. Jika formatnya tidak dikenal,
    /// dipakai ekstensi filenya.
    pub fn format_name(&self) -> String {
        self.video_format()
            .map(|format| format.name().to_string())
            .or_else(|| self.audio_format().map(|format| format.name().to_string()))
            .unwrap_or_else(|| self.extension().to_uppercase())
    }
}

/// Isi Pustaka.
#[derive(Debug, Default)]
pub struct Library {
    items: Vec<Item>,
    /// Item yang tidak terbaca (mis. dibuat DANload versi lebih baru). Tidak ditampilkan, tetapi
    /// ikut disimpan apa adanya supaya datanya tidak hilang.
    unreadable: Vec<Value>,
    next_id: u64,
    /// `false` jika file lama tidak bisa dibaca atau dicadangkan: jangan ditimpa.
    read_only: bool,
}

/// Susunan `library.json`.
#[derive(Serialize, Deserialize)]
struct LibraryFile<T> {
    version: u32,
    items: Vec<T>,
}

impl Library {
    /// Membaca Pustaka dari `path`. File yang belum ada berarti Pustaka kosong.
    ///
    /// Jika isinya rusak, file itu dicadangkan sebagai `library-rusak-<waktu>.json` (tidak
    /// dihapus), lalu Pustaka dimulai kosong. Teks kedua berisi pesan untuk pengguna, jika ada.
    pub fn load(path: &Path) -> (Library, Option<String>) {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return (Library::default(), None);
            }
            Err(error) => {
                let library = Library {
                    read_only: true,
                    ..Library::default()
                };
                return (
                    library,
                    Some(format!(
                        "Data Pustaka tidak bisa dibaca ({error}). Perubahan di Pustaka tidak akan disimpan sampai DANload dibuka ulang."
                    )),
                );
            }
        };
        match serde_json::from_str::<LibraryFile<Value>>(&text) {
            Ok(file) => (Library::from_values(file.items), None),
            Err(_) => {
                if let Some(name) = back_up_damaged(path, "library-rusak") {
                    (
                        Library::default(),
                        Some(format!(
                            "Data Pustaka rusak, jadi Pustaka dimulai dari kosong. Data lama disimpan sebagai {name}."
                        )),
                    )
                } else {
                    let library = Library {
                        read_only: true,
                        ..Library::default()
                    };
                    (
                        library,
                        Some(
                            "Data Pustaka rusak dan tidak bisa dicadangkan. Perubahan di Pustaka tidak akan disimpan."
                                .to_string(),
                        ),
                    )
                }
            }
        }
    }

    /// Menyusun Pustaka dari isi `items` di file. Item yang tidak terbaca disimpan terpisah,
    /// dan nomor kembar (mis. file diubah manual) diganti nomor baru.
    fn from_values(values: Vec<Value>) -> Library {
        let mut library = Library::default();
        let mut seen = HashSet::new();
        let mut duplicates = Vec::new();
        for value in values {
            match serde_json::from_value::<Item>(value.clone()) {
                Ok(item) if seen.insert(item.id) => library.items.push(item),
                Ok(item) => duplicates.push(item),
                Err(_) => library.unreadable.push(value),
            }
        }
        library.next_id = library
            .items
            .iter()
            .map(|item| item.id + 1)
            .max()
            .unwrap_or(1);
        for item in duplicates {
            library.add(item);
        }
        library
    }

    /// Semua item, urut sesuai waktu masuk ke Pustaka.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|item| item.id == id)
    }

    /// Menambahkan `item` dengan nomor baru (nomor di `item` diabaikan). Mengembalikan nomornya.
    pub fn add(&mut self, mut item: Item) -> u64 {
        // Nomor berdasarkan waktu (milidetik) supaya tetap unik walau item terakhir pernah dihapus.
        let id = self.next_id.max(now_millis());
        self.next_id = id + 1;
        item.id = id;
        self.items.push(item);
        id
    }

    /// Mengubah item bernomor `id`. `false` jika tidak ada.
    pub fn update(&mut self, id: u64, change: impl FnOnce(&mut Item)) -> bool {
        match self.items.iter_mut().find(|item| item.id == id) {
            Some(item) => {
                change(item);
                true
            }
            None => false,
        }
    }

    /// Menghapus item bernomor `ids` dari Pustaka (filenya tidak disentuh).
    /// Mengembalikan item yang dihapus.
    pub fn remove(&mut self, ids: &HashSet<u64>) -> Vec<Item> {
        let (removed, kept) = std::mem::take(&mut self.items)
            .into_iter()
            .partition(|item| ids.contains(&item.id));
        self.items = kept;
        removed
    }

    /// Menghapus item lain (selain `keep`) yang menunjuk file `path`. Dipakai saat file baru
    /// mendapat lokasi yang sama dengan item lama yang filenya sudah hilang (DANload tidak pernah
    /// menimpa file, jadi file item lama itu pasti sudah tidak ada): satu file, satu item.
    /// Mengembalikan item yang dihapus.
    pub fn remove_others_at(&mut self, path: &Path, keep: u64) -> Vec<Item> {
        let stale: HashSet<u64> = self
            .items
            .iter()
            .filter(|item| item.id != keep && item.path == path)
            .map(|item| item.id)
            .collect();
        if stale.is_empty() {
            return Vec::new();
        }
        self.remove(&stale)
    }

    /// Isi `library.json`. `None` jika Pustaka tidak boleh disimpan.
    pub fn to_json(&self) -> Option<Vec<u8>> {
        if self.read_only {
            return None;
        }
        let mut items: Vec<Value> = self
            .items
            .iter()
            .filter_map(|item| serde_json::to_value(item).ok())
            .collect();
        items.extend(self.unreadable.iter().cloned());
        serde_json::to_vec_pretty(&LibraryFile {
            version: FILE_VERSION,
            items,
        })
        .ok()
    }
}

/// Menyimpan file JSON (`library.json`, juga `queue.json` sejak Fase 7) di thread tersendiri.
///
/// Setiap [`Saver::save`] mengirim isi file terbaru. Jika beberapa versi menumpuk, hanya yang
/// terakhir yang ditulis. Saat `Saver` dibuang (aplikasi ditutup), penyimpanan terakhir ditunggu.
pub struct Saver {
    sender: Option<mpsc::Sender<Vec<u8>>>,
    worker: Option<JoinHandle<()>>,
}

impl Saver {
    /// `on_error` dipanggil (di thread penyimpan) dengan pesan error-nya jika gagal menulis.
    pub fn start(path: PathBuf, on_error: impl Fn(String) + Send + 'static) -> Saver {
        let (sender, receiver) = mpsc::channel::<Vec<u8>>();
        let worker = std::thread::spawn(move || {
            while let Ok(mut bytes) = receiver.recv() {
                while let Ok(newer) = receiver.try_recv() {
                    bytes = newer;
                }
                if let Err(error) = write_atomic(&path, &bytes) {
                    on_error(error.to_string());
                }
            }
        });
        Saver {
            sender: Some(sender),
            worker: Some(worker),
        }
    }

    pub fn save(&self, bytes: Vec<u8>) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(bytes);
        }
    }
}

impl Drop for Saver {
    fn drop(&mut self) {
        // Menutup channel membuat thread penyimpan selesai setelah menulis versi terakhir.
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Mencadangkan file JSON yang rusak sebagai `<prefix>-<waktu>.json` di folder yang sama, supaya
/// datanya tidak hilang (file tidak pernah dihapus). Mengembalikan nama cadangannya, atau `None`
/// jika file tidak bisa dipindah maupun disalin.
pub(crate) fn back_up_damaged(path: &Path, prefix: &str) -> Option<String> {
    let backup = path.with_file_name(format!("{prefix}-{}.json", now_millis()));
    let backed_up = fs::rename(path, &backup).is_ok() || fs::copy(path, &backup).is_ok();
    backed_up.then(|| {
        backup
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    })
}

/// Menulis file lewat file sementara lalu rename, supaya file JSON (`library.json`,
/// `queue.json`) tidak pernah setengah tertulis kalau komputer mati di tengah penulisan.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder)?;
    }
    let temp_path = path.with_extension("json.tmp");
    fs::write(&temp_path, bytes)?;
    filename::rename_with_retry(&temp_path, path)
}

// ---------------------------------------------------------------------------
// Cari, filter, urutkan
// ---------------------------------------------------------------------------

/// Filter jenis file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    #[default]
    All,
    Video,
    Audio,
}

/// Urutan daftar, sesuai urutan pilihan di menu "Urutkan".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    Newest,
    Oldest,
    Name,
    Largest,
}

impl SortOrder {
    pub const ALL: [SortOrder; 4] = [
        SortOrder::Newest,
        SortOrder::Oldest,
        SortOrder::Name,
        SortOrder::Largest,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SortOrder::Newest => "Terbaru",
            SortOrder::Oldest => "Terlama",
            SortOrder::Name => "Nama (A–Z)",
            SortOrder::Largest => "Ukuran terbesar",
        }
    }

    /// Dari indeks menu di UI; indeks yang tidak dikenal dianggap "Terbaru".
    pub fn from_index(index: i32) -> Self {
        usize::try_from(index)
            .ok()
            .and_then(|index| SortOrder::ALL.get(index).copied())
            .unwrap_or_default()
    }
}

/// Pencarian dan filter yang sedang berlaku.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Query {
    /// Kata yang dicari di nama file dan judul (tidak membedakan huruf besar/kecil).
    pub text: String,
    pub kind: KindFilter,
    pub platform: Option<Platform>,
    pub sort: SortOrder,
}

/// Posisi item di `items` yang cocok dengan `query`, sudah diurutkan.
pub fn query(items: &[Item], query: &Query) -> Vec<usize> {
    let words: Vec<String> = query
        .text
        .to_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let mut found: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches(item, query, &words))
        .map(|(index, _)| index)
        .collect();
    found.sort_by(|a, b| compare(&items[*a], &items[*b], query.sort));
    found
}

fn matches(item: &Item, query: &Query, words: &[String]) -> bool {
    let kind_ok = match query.kind {
        KindFilter::All => true,
        KindFilter::Video => item.mode == DownloadMode::Video,
        KindFilter::Audio => item.mode == DownloadMode::Audio,
    };
    if !kind_ok
        || query
            .platform
            .is_some_and(|platform| platform != item.platform)
    {
        return false;
    }
    if words.is_empty() {
        return true;
    }
    let file_name = item
        .path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let haystack = format!("{file_name} {}", item.title).to_lowercase();
    words.iter().all(|word| haystack.contains(word.as_str()))
}

fn compare(a: &Item, b: &Item, sort: SortOrder) -> CmpOrdering {
    match sort {
        SortOrder::Newest => (b.downloaded_at, b.id).cmp(&(a.downloaded_at, a.id)),
        SortOrder::Oldest => (a.downloaded_at, a.id).cmp(&(b.downloaded_at, b.id)),
        SortOrder::Name => natural_cmp(&a.name(), &b.name()).then(a.id.cmp(&b.id)),
        SortOrder::Largest => (b.size, b.id).cmp(&(a.size, a.id)),
    }
}

/// Membandingkan nama seperti manusia: tanpa membedakan huruf besar/kecil, dan angka dibaca
/// sebagai bilangan, jadi `Video (2)` ada sebelum `Video (10)`.
fn natural_cmp(a: &str, b: &str) -> CmpOrdering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let mut left = a.chars().peekable();
    let mut right = b.chars().peekable();
    loop {
        match (left.peek().copied(), right.peek().copied()) {
            (None, None) => return CmpOrdering::Equal,
            (None, Some(_)) => return CmpOrdering::Less,
            (Some(_), None) => return CmpOrdering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take_number = |chars: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut digits = String::new();
                    while let Some(digit) = chars.peek().copied().filter(char::is_ascii_digit) {
                        digits.push(digit);
                        chars.next();
                    }
                    digits
                };
                let (x, y) = (take_number(&mut left), take_number(&mut right));
                let (x, y) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != CmpOrdering::Equal {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                left.next();
                right.next();
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Ganti nama dan file hilang
// ---------------------------------------------------------------------------

/// Lokasi baru jika file `path` diganti namanya menjadi `input` (tanpa ekstensi; ekstensi lama
/// selalu dipertahankan). Memakai aturan yang sama dengan kolom NAMA FILE
/// ([`filename::check_custom_name`]), dan menolak nama yang sudah dipakai file lain.
pub fn rename_target(path: &Path, input: &str) -> Result<PathBuf, String> {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = match filename::check_custom_name(input, &extension) {
        Ok(Some(stem)) => stem,
        Ok(None) => return Err("Nama file tidak boleh kosong.".to_string()),
        Err(message) => return Err(message),
    };
    let new_name = if extension.is_empty() {
        stem
    } else {
        format!("{stem}.{extension}")
    };
    let target = path.with_file_name(&new_name);
    if target != path && target.exists() && !same_name_ignoring_case(path, &target) {
        return Err(format!(
            "Nama \"{new_name}\" sudah dipakai file lain di folder itu."
        ));
    }
    Ok(target)
}

/// `true` jika nama file `a` dan `b` hanya berbeda huruf besar/kecil. Di Windows keduanya
/// dianggap nama yang sama (file yang sama).
fn same_name_ignoring_case(a: &Path, b: &Path) -> bool {
    cfg!(windows)
        && a.parent() == b.parent()
        && match (a.file_name(), b.file_name()) {
            (Some(a), Some(b)) => {
                a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
            }
            _ => false,
        }
}

/// Mengganti nama file `path` menjadi `input` (lihat [`rename_target`]). File lain tidak pernah
/// ditimpa. Mengembalikan lokasi barunya, atau pesan untuk pengguna.
pub fn rename_file(path: &Path, input: &str) -> Result<PathBuf, String> {
    let target = rename_target(path, input)?;
    if target == path {
        return Ok(target); // Namanya tidak berubah.
    }
    if !path.is_file() {
        return Err(
            "File tidak ditemukan. Mungkin sudah dipindah atau dihapus di luar DANload."
                .to_string(),
        );
    }
    if same_name_ignoring_case(path, &target) {
        // Hanya huruf besar/kecil yang berubah: bagi Windows itu file yang sama.
        return fs::rename(path, &target)
            .map(|()| target)
            .map_err(rename_error);
    }
    // "Pesan" nama baru dengan file kosong (gagal jika ternyata sudah ada), lalu timpa file kosong
    // itu dengan file aslinya. Cara yang sama dengan `filename::move_to_unique_name`.
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
    {
        Ok(placeholder) => drop(placeholder),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            let name = target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Err(format!(
                "Nama \"{name}\" sudah dipakai file lain di folder itu."
            ));
        }
        Err(error) => return Err(rename_error(error)),
    }
    match fs::rename(path, &target) {
        Ok(()) => Ok(target),
        Err(error) => {
            let _ = fs::remove_file(&target);
            Err(rename_error(error))
        }
    }
}

/// Pesan untuk pengguna saat ganti nama gagal.
fn rename_error(error: io::Error) -> String {
    // 32 = ERROR_SHARING_VIOLATION di Windows: file sedang dibuka program lain.
    if error.kind() == io::ErrorKind::PermissionDenied || error.raw_os_error() == Some(32) {
        return "File sedang dipakai program lain atau foldernya tidak bisa ditulisi. Tutup dulu programnya, lalu coba lagi."
            .to_string();
    }
    if error.kind() == io::ErrorKind::NotFound {
        return "File tidak ditemukan. Mungkin sudah dipindah atau dihapus di luar DANload."
            .to_string();
    }
    format!("Nama file gagal diganti: {error}")
}

/// Memeriksa file milik `items` (nomor, lokasi). Hasilnya: ukuran file sekarang, atau `None`
/// jika file sudah tidak ada. Bisa lambat untuk folder jaringan: **jalankan di thread lain**.
pub fn check_files(items: &[(u64, PathBuf)]) -> Vec<(u64, Option<u64>)> {
    items
        .iter()
        .map(|(id, path)| {
            let size = fs::metadata(path)
                .ok()
                .filter(|metadata| metadata.is_file())
                .map(|metadata| metadata.len());
            (*id, size)
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Lokasi data dan thumbnail
// ---------------------------------------------------------------------------

/// Folder thumbnail: `%LOCALAPPDATA%\DANload\thumbs\`. `None` jika folder data tidak diketahui.
pub fn thumbnail_dir() -> Option<PathBuf> {
    config::env_path("LOCALAPPDATA")
        .or_else(|| config::env_path("XDG_DATA_HOME"))
        .map(|base| base.join(config::APP_DIR_NAME).join(THUMBNAIL_DIR_NAME))
}

/// Lokasi untuk thumbnail baru di `dir`, dengan nama unik (waktu + nomor urut).
pub fn new_thumbnail_path(dir: &Path) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let number = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("{}-{number}.jpg", now_millis()))
}

/// Lokasi thumbnail bernama `name` di `dir`. `None` jika namanya bukan nama file thumbnail biasa
/// (mis. `library.json` diubah manual menjadi `..\file-lain`), supaya DANload tidak pernah
/// membaca atau menghapus file di luar folder thumbnail.
pub fn thumbnail_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let plain = Path::new(name).file_name().is_some_and(|file| file == name);
    (plain && name.ends_with(".jpg")).then(|| dir.join(name))
}

/// Menghapus thumbnail milik item yang dihapus dari Pustaka. Error diabaikan (hanya cache).
pub fn remove_thumbnail(dir: &Path, name: &str) {
    if let Some(path) = thumbnail_path(dir, name) {
        let _ = fs::remove_file(path);
    }
}

// ---------------------------------------------------------------------------
// Teks untuk ditampilkan
// ---------------------------------------------------------------------------

/// Kualitas sebenarnya hasil unduhan berformat `format`, dari info ffprobe:
/// `1080p` (sisi terpendek gambar, sama seperti menu resolusi), `192 kbps`, `Lossless`, atau
/// `Tanpa kompresi`. Kosong jika tidak diketahui.
pub fn quality_label(format: OutputFormat, info: Option<&MediaInfo>) -> String {
    match format {
        OutputFormat::Video { .. } => info
            .and_then(|info| info.video.as_ref())
            .and_then(|video| Some(video.width?.min(video.height?)))
            .map(|height| format!("{height}p"))
            .unwrap_or_default(),
        OutputFormat::Audio { format, .. } => match format.quality() {
            AudioQuality::Lossless | AudioQuality::Uncompressed => format
                .quality()
                .fixed_label()
                .unwrap_or_default()
                .to_string(),
            AudioQuality::Bitrate | AudioQuality::Original => info
                .and_then(|info| {
                    info.audio
                        .as_ref()
                        .and_then(|audio| audio.bit_rate)
                        .or(info.bit_rate)
                })
                .map(|bits| format!("{} kbps", (bits + 500) / 1000))
                .unwrap_or_default(),
        },
    }
}

/// Tanggal singkat untuk daftar: `15 Nov 19:40` (tahun ini), atau `15 Nov 2025`.
pub fn short_date(time: LocalTime, now: LocalTime) -> String {
    let month = SHORT_MONTHS
        .get(time.month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("?");
    if time.year == now.year {
        format!("{} {month} {:02}:{:02}", time.day, time.hour, time.minute)
    } else {
        format!("{} {month} {}", time.day, time.year)
    }
}

/// Tanggal lengkap untuk panel detail: `15 November 2026, 19:40`.
pub fn full_date(time: LocalTime) -> String {
    let month = MONTHS
        .get(time.month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("?");
    format!(
        "{} {month} {}, {:02}:{:02}",
        time.day, time.year, time.hour, time.minute
    )
}

/// Mode dan nama format untuk `Item` (nama sama seperti di config.json, mis. `mp4`).
pub fn format_key(format: OutputFormat) -> (DownloadMode, String) {
    let (mode, value) = match format {
        OutputFormat::Video { format, .. } => (DownloadMode::Video, serde_json::to_value(format)),
        OutputFormat::Audio { format, .. } => (DownloadMode::Audio, serde_json::to_value(format)),
    };
    let name = match value {
        Ok(Value::String(name)) => name,
        _ => format.extension().to_string(),
    };
    (mode, name)
}

/// Detik sejak 1 Januari 1970 (UTC), untuk `Item::downloaded_at`.
pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::StreamInfo;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("danload-pustaka-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn item(name: &str, platform: Platform, mode: DownloadMode, size: u64, at: i64) -> Item {
        Item {
            id: 0,
            path: PathBuf::from(format!("/unduhan/{name}")),
            title: format!("Judul {name}"),
            platform,
            mode,
            format: match mode {
                DownloadMode::Video => "mp4".into(),
                DownloadMode::Audio => "mp3".into(),
            },
            quality: String::new(),
            size,
            duration: Some(65.0),
            downloaded_at: at,
            source_url: "https://youtu.be/abc".into(),
            thumbnail: None,
        }
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = temp_dir("simpan");
        let path = dir.join("DANload").join("library.json");
        let mut library = Library::default();
        let first = library.add(item(
            "Lagu é 日本.mp3",
            Platform::YouTube,
            DownloadMode::Audio,
            10,
            5,
        ));
        let second = library.add(item(
            "Video.mp4",
            Platform::Instagram,
            DownloadMode::Video,
            20,
            6,
        ));
        assert!(second > first, "nomor baru selalu lebih besar");

        write_atomic(&path, &library.to_json().unwrap()).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        let (loaded, message) = Library::load(&path);
        assert_eq!(message, None);
        assert_eq!(loaded.items(), library.items());
        assert_eq!(loaded.get(second).unwrap().platform, Platform::Instagram);
        // Nomor berikutnya tetap lebih besar dari semua nomor lama.
        let mut loaded = loaded;
        let third = loaded.add(item(
            "Baru.mp4",
            Platform::YouTube,
            DownloadMode::Video,
            1,
            7,
        ));
        assert!(third > second);
    }

    #[test]
    fn missing_file_is_an_empty_library() {
        let dir = temp_dir("kosong");
        let (library, message) = Library::load(&dir.join("library.json"));
        assert!(library.items().is_empty());
        assert_eq!(message, None);
        assert!(library.to_json().is_some(), "boleh disimpan");
    }

    #[test]
    fn corrupt_file_is_backed_up_not_lost() {
        let dir = temp_dir("rusak");
        let path = dir.join("library.json");
        fs::write(&path, "{ ini bukan json").unwrap();
        let (library, message) = Library::load(&path);
        assert!(library.items().is_empty());
        assert!(message.unwrap().starts_with("Data Pustaka rusak"));
        assert!(library.to_json().is_some(), "boleh disimpan");
        let backups: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("library-rusak-"))
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            fs::read_to_string(dir.join(&backups[0])).unwrap(),
            "{ ini bukan json"
        );
    }

    #[test]
    fn unknown_values_are_tolerated_and_kept() {
        let json = r#"{
            "version": 1,
            "items": [
                { "id": 7, "path": "/a/Video.mp4", "platform": "tiktok", "mode": "video",
                  "format": "mp4", "kolom_baru": true },
                { "id": 7, "path": "/a/Kembar.mp4" },
                { "path": "/a/tanpa-nomor.mp4" }
            ]
        }"#;
        let dir = temp_dir("toleran");
        let path = dir.join("library.json");
        fs::write(&path, json).unwrap();
        let (library, message) = Library::load(&path);
        assert_eq!(message, None);
        // Platform tak dikenal → bawaan (YouTube); nomor kembar diganti nomor baru.
        assert_eq!(library.items().len(), 2);
        assert_eq!(library.items()[0].platform, Platform::YouTube);
        assert_ne!(library.items()[0].id, library.items()[1].id);
        // Item yang tidak terbaca ikut tersimpan lagi.
        let saved = String::from_utf8(library.to_json().unwrap()).unwrap();
        assert!(saved.contains("tanpa-nomor"));
    }

    #[test]
    fn formats_are_read_back_safely() {
        let mut video = item("Klip.mpg", Platform::YouTube, DownloadMode::Video, 1, 1);
        video.format = "mpeg".into();
        assert_eq!(video.video_format(), Some(VideoFormat::Mpeg));
        assert_eq!(video.format_name(), "MPEG");
        assert_eq!(video.audio_format(), None);
        video.format = "format-baru".into();
        assert_eq!(video.video_format(), None);
        assert_eq!(video.format_name(), "MPG");
        let audio = item("Lagu.flac", Platform::YouTube, DownloadMode::Audio, 1, 1);
        assert_eq!(audio.name(), "Lagu");
        assert_eq!(audio.extension(), "flac");
    }

    #[test]
    fn format_keys_match_config_names() {
        let mpeg = OutputFormat::Video {
            format: VideoFormat::Mpeg,
            max_height: None,
        };
        assert_eq!(format_key(mpeg), (DownloadMode::Video, "mpeg".to_string()));
        let ogg = OutputFormat::Audio {
            format: AudioFormat::Ogg,
            bitrate_kbps: 128,
        };
        assert_eq!(format_key(ogg), (DownloadMode::Audio, "ogg".to_string()));
        // Kebalikannya: nama itu dibaca lagi sebagai format yang sama.
        let mut saved = item("Klip.mpg", Platform::YouTube, DownloadMode::Video, 1, 1);
        saved.format = format_key(mpeg).1;
        assert_eq!(saved.video_format(), Some(VideoFormat::Mpeg));
    }

    #[test]
    fn remove_returns_removed_items() {
        let mut library = Library::default();
        let a = library.add(item("A.mp4", Platform::YouTube, DownloadMode::Video, 1, 1));
        let b = library.add(item("B.mp4", Platform::YouTube, DownloadMode::Video, 1, 2));
        let removed = library.remove(&HashSet::from([a]));
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, a);
        assert_eq!(library.items().len(), 1);
        assert!(library.get(b).is_some());
        assert!(library.update(b, |item| item.size = 99));
        assert_eq!(library.get(b).unwrap().size, 99);
        assert!(!library.update(a, |_| {}));

        // File baru di lokasi item lama yang filenya hilang: item lama dibuang.
        let old = library.add(item("C.mp4", Platform::YouTube, DownloadMode::Video, 1, 3));
        let new = library.add(item("C.mp4", Platform::YouTube, DownloadMode::Video, 2, 4));
        let path = library.get(new).unwrap().path.clone();
        let stale = library.remove_others_at(&path, new);
        assert_eq!(stale.iter().map(|item| item.id).collect::<Vec<_>>(), [old]);
        assert!(library.get(new).is_some() && library.get(old).is_none());
        assert!(library.remove_others_at(&path, new).is_empty());
    }

    fn names(items: &[Item], found: &[usize]) -> Vec<String> {
        found.iter().map(|index| items[*index].name()).collect()
    }

    #[test]
    fn query_filters_searches_and_sorts() {
        let items = vec![
            item(
                "Video (10).mp4",
                Platform::YouTube,
                DownloadMode::Video,
                300,
                3,
            ),
            item(
                "lagu santai.mp3",
                Platform::YouTube,
                DownloadMode::Audio,
                100,
                1,
            ),
            item(
                "Video (2).mp4",
                Platform::Instagram,
                DownloadMode::Video,
                200,
                2,
            ),
            item(
                "Reel Liburan.mp4",
                Platform::Facebook,
                DownloadMode::Video,
                50,
                4,
            ),
        ];
        let all = Query::default();
        assert_eq!(
            names(&items, &query(&items, &all)),
            ["Reel Liburan", "Video (10)", "Video (2)", "lagu santai"]
        );
        let by_name = Query {
            sort: SortOrder::Name,
            ..Query::default()
        };
        assert_eq!(
            names(&items, &query(&items, &by_name)),
            ["lagu santai", "Reel Liburan", "Video (2)", "Video (10)"]
        );
        let largest = Query {
            sort: SortOrder::Largest,
            ..Query::default()
        };
        assert_eq!(names(&items, &query(&items, &largest))[0], "Video (10)");
        let oldest = Query {
            sort: SortOrder::Oldest,
            ..Query::default()
        };
        assert_eq!(names(&items, &query(&items, &oldest))[0], "lagu santai");

        let audio = Query {
            kind: KindFilter::Audio,
            ..Query::default()
        };
        assert_eq!(names(&items, &query(&items, &audio)), ["lagu santai"]);
        let instagram_video = Query {
            kind: KindFilter::Video,
            platform: Some(Platform::Instagram),
            ..Query::default()
        };
        assert_eq!(
            names(&items, &query(&items, &instagram_video)),
            ["Video (2)"]
        );

        // Pencarian: semua kata harus ada, huruf besar/kecil tidak dibedakan, judul ikut dicari.
        let search = |text: &str| Query {
            text: text.into(),
            ..Query::default()
        };
        assert_eq!(
            names(&items, &query(&items, &search("VIDEO 2"))),
            ["Video (2)"]
        );
        assert_eq!(
            names(&items, &query(&items, &search("judul reel"))),
            ["Reel Liburan"]
        );
        assert_eq!(
            names(&items, &query(&items, &search(".mp3"))),
            ["lagu santai"]
        );
        assert!(query(&items, &search("tidak ada")).is_empty());
    }

    #[test]
    fn natural_order_reads_numbers() {
        assert_eq!(natural_cmp("Video (2)", "Video (10)"), CmpOrdering::Less);
        assert_eq!(natural_cmp("a", "B"), CmpOrdering::Less);
        assert_eq!(natural_cmp("x 007", "x 7"), CmpOrdering::Equal);
        assert_eq!(natural_cmp("abc", "ab"), CmpOrdering::Greater);
        assert_eq!(SortOrder::from_index(2), SortOrder::Name);
        assert_eq!(SortOrder::from_index(-1), SortOrder::Newest);
    }

    #[test]
    fn rename_keeps_extension_and_never_overwrites() {
        let dir = temp_dir("ganti-nama");
        let file = dir.join("Video Lama.mp4");
        fs::write(&file, "isi video").unwrap();
        fs::write(dir.join("Sudah Ada.mp4"), "file lain").unwrap();

        // Ekstensi yang ikut diketik dibuang; ekstensi lama dipertahankan.
        let renamed = rename_file(&file, "Liburan é 日本.mp4").unwrap();
        assert_eq!(renamed, dir.join("Liburan é 日本.mp4"));
        assert_eq!(fs::read_to_string(&renamed).unwrap(), "isi video");
        assert!(!file.exists());

        assert_eq!(
            rename_file(&renamed, "Sudah Ada"),
            Err("Nama \"Sudah Ada.mp4\" sudah dipakai file lain di folder itu.".to_string())
        );
        assert_eq!(
            fs::read_to_string(dir.join("Sudah Ada.mp4")).unwrap(),
            "file lain"
        );
        assert_eq!(
            rename_file(&renamed, "a?b"),
            Err("Nama file tidak boleh berisi karakter \\ / : * ? \" < > |".to_string())
        );
        assert_eq!(
            rename_file(&renamed, "   "),
            Err("Nama file tidak boleh kosong.".to_string())
        );
        assert!(
            rename_file(&renamed, "CON")
                .unwrap_err()
                .contains("dipakai oleh Windows")
        );
        // Nama tidak berubah: tidak ada yang dilakukan.
        assert_eq!(rename_file(&renamed, "Liburan é 日本").unwrap(), renamed);
        // File sudah hilang.
        assert!(
            rename_file(&dir.join("Hilang.mp4"), "Baru")
                .unwrap_err()
                .starts_with("File tidak ditemukan")
        );
        assert!(
            !dir.join("Baru.mp4").exists(),
            "file kosong pemesan nama tidak tertinggal"
        );
    }

    #[test]
    fn rename_target_checks_without_changing_anything() {
        let dir = temp_dir("cek-nama");
        let file = dir.join("Klip.webm");
        fs::write(&file, "x").unwrap();
        fs::write(dir.join("Lain.webm"), "y").unwrap();
        assert_eq!(rename_target(&file, "Baru"), Ok(dir.join("Baru.webm")));
        assert!(rename_target(&file, "Lain").is_err());
        assert_eq!(rename_target(&file, "Klip"), Ok(file.clone()));
        assert!(file.exists() && !dir.join("Baru.webm").exists());
    }

    #[test]
    fn missing_files_are_detected() {
        let dir = temp_dir("hilang");
        let present = dir.join("ada.mp3");
        fs::write(&present, "12345").unwrap();
        let result = check_files(&[
            (1, present),
            (2, dir.join("tidak-ada.mp3")),
            (3, dir.clone()),
        ]);
        assert_eq!(result, vec![(1, Some(5)), (2, None), (3, None)]);
    }

    #[test]
    fn thumbnail_names_stay_inside_their_folder() {
        let dir = Path::new("/cache/thumbs");
        assert_eq!(
            thumbnail_path(dir, "123-0.jpg"),
            Some(PathBuf::from("/cache/thumbs/123-0.jpg"))
        );
        assert_eq!(thumbnail_path(dir, "../library.json"), None);
        assert_eq!(thumbnail_path(dir, "../../x.jpg"), None);
        assert_eq!(thumbnail_path(dir, "sub/x.jpg"), None);
        let first = new_thumbnail_path(dir);
        let second = new_thumbnail_path(dir);
        assert_ne!(first, second);
        assert!(first.starts_with(dir));
    }

    #[test]
    fn quality_labels_use_real_values() {
        let video = OutputFormat::Video {
            format: VideoFormat::Mp4,
            max_height: None,
        };
        let vertical = MediaInfo {
            video: Some(StreamInfo {
                codec: "h264".into(),
                width: Some(1080),
                height: Some(1920),
                ..StreamInfo::default()
            }),
            ..MediaInfo::default()
        };
        assert_eq!(quality_label(video, Some(&vertical)), "1080p");
        assert_eq!(quality_label(video, None), "");

        let mp3 = OutputFormat::Audio {
            format: AudioFormat::Mp3,
            bitrate_kbps: 192,
        };
        let audio = MediaInfo {
            audio: Some(StreamInfo {
                codec: "mp3".into(),
                bit_rate: Some(191_600),
                ..StreamInfo::default()
            }),
            ..MediaInfo::default()
        };
        assert_eq!(quality_label(mp3, Some(&audio)), "192 kbps");
        // Opus di file .opus tidak mencatat bitrate per stream: pakai bitrate seluruh file.
        let opus = OutputFormat::Audio {
            format: AudioFormat::Opus,
            bitrate_kbps: 192,
        };
        let whole = MediaInfo {
            bit_rate: Some(131_200),
            ..MediaInfo::default()
        };
        assert_eq!(quality_label(opus, Some(&whole)), "131 kbps");
        let flac = OutputFormat::Audio {
            format: AudioFormat::Flac,
            bitrate_kbps: 192,
        };
        assert_eq!(quality_label(flac, None), "Lossless");
    }

    #[test]
    fn dates_are_written_in_indonesian() {
        let time = LocalTime {
            year: 2026,
            month: 11,
            day: 5,
            hour: 9,
            minute: 7,
        };
        let now = LocalTime { day: 16, ..time };
        assert_eq!(short_date(time, now), "5 Nov 09:07");
        let next_year = LocalTime { year: 2027, ..time };
        assert_eq!(short_date(time, next_year), "5 Nov 2026");
        assert_eq!(full_date(time), "5 November 2026, 09:07");
        let august = LocalTime { month: 8, ..time };
        assert_eq!(short_date(august, now), "5 Agu 09:07");
    }

    #[test]
    fn saver_writes_the_latest_version() {
        let dir = temp_dir("penyimpan");
        let path = dir.join("sub").join("library.json");
        let errors = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let saver = {
            let errors = std::sync::Arc::clone(&errors);
            Saver::start(path.clone(), move |message| {
                errors.lock().unwrap().push(message)
            })
        };
        for version in 1..=50 {
            saver.save(format!("versi {version}").into_bytes());
        }
        drop(saver); // Menunggu penyimpanan terakhir.
        assert_eq!(fs::read_to_string(&path).unwrap(), "versi 50");
        assert!(errors.lock().unwrap().is_empty());
    }
}
