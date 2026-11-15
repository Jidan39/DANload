//! Antrean unduhan (sejak Fase 7): unduhan yang menunggu giliran.
//!
//! - Paling banyak N unduhan berjalan bersamaan (dipilih di Pengaturan, 1–3), dan **Instagram
//!   hanya satu per waktu**: tanpa login, Instagram cepat membatasi akses kalau diminta beberapa
//!   sekaligus (keputusan pemilik projek, 2026-11-16).
//! - Antrean disimpan di `%APPDATA%\DANload\queue.json`, termasuk unduhan yang sedang berjalan,
//!   supaya bisa dilanjutkan saat DANload dibuka lagi. Unduhan yang terputus diulang dari awal.
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::downloader::Request;
use crate::formats::OutputFormat;
use crate::library;
use crate::platform::Platform;

/// Versi susunan `queue.json`.
const FILE_VERSION: u32 = 1;

/// Satu unduhan yang diminta pengguna.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    /// Link yang diunduh (untuk playlist: link tiap videonya).
    pub url: String,
    pub platform: Platform,
    pub format: OutputFormat,
    /// Folder tujuan saat unduhan ditambahkan (sudah termasuk subfolder Video/Audio).
    pub folder: PathBuf,
    /// Nama file pilihan pengguna, tanpa ekstensi. `None` = mengikuti judul video.
    #[serde(default)]
    pub file_name: Option<String>,
    /// Judul yang sudah diketahui (mis. dari isi playlist). Kosong = belum diketahui.
    #[serde(default)]
    pub title: String,
}

impl Job {
    /// Permintaan untuk downloader. `thumbnails`: folder thumbnail Pustaka.
    pub fn request(&self, thumbnails: Option<PathBuf>) -> Request {
        Request {
            url: self.url.clone(),
            format: self.format,
            folder: self.folder.clone(),
            file_name: self.file_name.clone(),
            thumbnails,
        }
    }

    /// Judul untuk ditampilkan: judul yang sudah diketahui, atau link-nya.
    pub fn display_title(&self) -> &str {
        if self.title.trim().is_empty() {
            self.url.trim()
        } else {
            self.title.trim()
        }
    }
}

/// Satu unduhan di antrean, dengan nomor unik untuk tombol-tombol di UI.
#[derive(Debug, Clone, PartialEq)]
pub struct Waiting {
    pub id: u64,
    pub job: Job,
}

/// Unduhan yang menunggu giliran, urut dari yang paling dulu ditambahkan.
#[derive(Debug, Default)]
pub struct Queue {
    waiting: VecDeque<Waiting>,
    last_id: u64,
    /// Dijeda: unduhan baru tidak dimulai (yang sedang berjalan tetap diselesaikan).
    paused: bool,
}

impl Queue {
    /// Menambahkan unduhan di belakang antrean. Mengembalikan nomornya.
    pub fn push_back(&mut self, job: Job) -> u64 {
        let id = self.new_id();
        self.waiting.push_back(Waiting { id, job });
        id
    }

    /// Menambahkan unduhan di depan antrean (mis. "Coba lagi"), supaya dimulai paling dulu.
    pub fn push_front(&mut self, job: Job) -> u64 {
        let id = self.new_id();
        self.waiting.push_front(Waiting { id, job });
        id
    }

    /// Mengeluarkan unduhan bernomor `id` dari antrean.
    pub fn remove(&mut self, id: u64) -> Option<Job> {
        let index = self.waiting.iter().position(|waiting| waiting.id == id)?;
        self.waiting.remove(index).map(|waiting| waiting.job)
    }

    /// Mengosongkan antrean. Mengembalikan jumlah unduhan yang dikeluarkan.
    pub fn clear(&mut self) -> usize {
        let count = self.waiting.len();
        self.waiting.clear();
        count
    }

    /// Unduhan yang menunggu, urut sesuai giliran.
    pub fn waiting(&self) -> impl ExactSizeIterator<Item = &Waiting> {
        self.waiting.iter()
    }

    pub fn len(&self) -> usize {
        self.waiting.len()
    }

    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Mengambil unduhan berikutnya yang boleh dimulai sekarang ([`can_start`]).
    ///
    /// `running`: platform tiap unduhan yang sedang berjalan; `max`: batas bersamaan.
    /// Jika unduhan terdepan belum boleh mulai (Instagram, sementara Instagram lain masih
    /// berjalan), unduhan berikutnya yang boleh mulai diambil lebih dulu, supaya slot tidak
    /// menganggur. `None` jika dijeda, slot penuh, atau belum ada yang boleh mulai.
    pub fn take_next(&mut self, running: &[Platform], max: usize) -> Option<Waiting> {
        if self.paused {
            return None;
        }
        let index = self
            .waiting
            .iter()
            .position(|waiting| can_start(waiting.job.platform, running, max))?;
        self.waiting.remove(index)
    }

    fn new_id(&mut self) -> u64 {
        self.last_id += 1;
        self.last_id
    }
}

/// Aturan giliran: masih ada slot (`running` lebih sedikit dari `max`), dan Instagram hanya
/// satu per waktu.
pub fn can_start(platform: Platform, running: &[Platform], max: usize) -> bool {
    running.len() < max
        && !(platform == Platform::Instagram && running.contains(&Platform::Instagram))
}

/// Susunan `queue.json` saat dibaca. Unduhan dibaca satu per satu supaya satu yang tidak
/// terbaca (mis. format dari versi DANload yang lebih baru) tidak menggagalkan semuanya.
#[derive(Deserialize)]
struct LoadedFile {
    #[serde(default)]
    paused: bool,
    #[serde(default)]
    jobs: Vec<Value>,
}

/// Susunan `queue.json` saat ditulis.
#[derive(Serialize)]
struct SavedFile<'a> {
    version: u32,
    paused: bool,
    jobs: Vec<&'a Job>,
}

/// Membaca antrean dari `path`. File yang belum ada berarti antrean kosong.
///
/// File rusak dicadangkan sebagai `queue-rusak-<waktu>.json` (tidak dihapus). Teks kedua berisi
/// pesan untuk pengguna, jika ada.
pub fn load(path: &Path) -> (Queue, Option<String>) {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return (Queue::default(), None),
        Err(error) => {
            return (
                Queue::default(),
                Some(format!("Antrean tersimpan tidak bisa dibaca ({error}).")),
            );
        }
    };
    match serde_json::from_str::<LoadedFile>(&text) {
        Ok(file) => {
            let mut queue = Queue {
                paused: file.paused,
                ..Queue::default()
            };
            let mut skipped = 0;
            for value in file.jobs {
                match serde_json::from_value::<Job>(value) {
                    Ok(job) => {
                        queue.push_back(job);
                    }
                    Err(_) => skipped += 1,
                }
            }
            let message = (skipped > 0)
                .then(|| format!("{skipped} unduhan di antrean tidak bisa dibaca dan dilewati."));
            (queue, message)
        }
        Err(_) => {
            let message = match library::back_up_damaged(path, "queue-rusak") {
                Some(name) => format!(
                    "Antrean tersimpan rusak, jadi antrean dimulai kosong. Data lama disimpan sebagai {name}."
                ),
                None => "Antrean tersimpan rusak, jadi antrean dimulai kosong.".to_string(),
            };
            (Queue::default(), Some(message))
        }
    }
}

/// Isi `queue.json`: unduhan yang sedang berjalan (`running`, disimpan di depan supaya diulang
/// lebih dulu saat DANload dibuka lagi), lalu isi antrean.
pub fn to_json<'a>(
    running: impl IntoIterator<Item = &'a Job>,
    queue: &'a Queue,
) -> Option<Vec<u8>> {
    let jobs = running
        .into_iter()
        .chain(queue.waiting.iter().map(|waiting| &waiting.job))
        .collect();
    serde_json::to_vec_pretty(&SavedFile {
        version: FILE_VERSION,
        paused: queue.paused,
        jobs,
    })
    .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::{AudioFormat, VideoFormat};

    fn job(name: &str, platform: Platform) -> Job {
        Job {
            url: format!("https://contoh/{name}"),
            platform,
            format: OutputFormat::Video {
                format: VideoFormat::Mp4,
                max_height: Some(1080),
            },
            folder: PathBuf::from("/unduhan"),
            file_name: None,
            title: name.to_string(),
        }
    }

    fn titles(queue: &Queue) -> Vec<String> {
        queue
            .waiting()
            .map(|waiting| waiting.job.title.clone())
            .collect()
    }

    #[test]
    fn jobs_start_in_order_up_to_the_limit() {
        let mut queue = Queue::default();
        for name in ["a", "b", "c", "d"] {
            queue.push_back(job(name, Platform::YouTube));
        }
        let mut running = Vec::new();
        while let Some(next) = queue.take_next(&running, 3) {
            running.push(next.job.platform);
            assert_eq!(next.job.title, ["a", "b", "c"][running.len() - 1]);
        }
        assert_eq!(running.len(), 3, "paling banyak 3 bersamaan");
        assert_eq!(titles(&queue), ["d"]);
        assert!(queue.take_next(&running[..2], 1).is_none(), "slot penuh");
        assert_eq!(queue.take_next(&running[..2], 3).unwrap().job.title, "d");
    }

    #[test]
    fn instagram_runs_one_at_a_time_without_blocking_others() {
        let mut queue = Queue::default();
        queue.push_back(job("ig1", Platform::Instagram));
        queue.push_back(job("ig2", Platform::Instagram));
        queue.push_back(job("yt", Platform::YouTube));
        queue.push_back(job("fb", Platform::Facebook));

        let mut running = vec![queue.take_next(&[], 3).unwrap().job.platform];
        assert_eq!(running, [Platform::Instagram]);
        // ig2 harus menunggu, tetapi YouTube dan Facebook di belakangnya boleh mulai.
        let next = queue.take_next(&running, 3).unwrap();
        assert_eq!(next.job.title, "yt");
        running.push(next.job.platform);
        assert_eq!(queue.take_next(&running, 3).unwrap().job.title, "fb");
        assert_eq!(titles(&queue), ["ig2"]);
        assert!(queue.take_next(&[Platform::Instagram], 3).is_none());
        assert_eq!(
            queue.take_next(&[Platform::YouTube], 3).unwrap().job.title,
            "ig2"
        );
        assert!(can_start(Platform::Instagram, &[Platform::YouTube], 3));
        assert!(!can_start(Platform::Instagram, &[Platform::Instagram], 3));
        assert!(!can_start(Platform::YouTube, &[Platform::YouTube; 3], 3));
    }

    #[test]
    fn paused_queue_starts_nothing() {
        let mut queue = Queue::default();
        queue.push_back(job("a", Platform::YouTube));
        queue.set_paused(true);
        assert!(queue.take_next(&[], 3).is_none());
        queue.set_paused(false);
        assert!(queue.take_next(&[], 3).is_some());
    }

    #[test]
    fn retry_goes_first_and_items_can_be_removed() {
        let mut queue = Queue::default();
        let a = queue.push_back(job("a", Platform::YouTube));
        queue.push_back(job("b", Platform::YouTube));
        let again = queue.push_front(job("ulang", Platform::YouTube));
        assert_eq!(titles(&queue), ["ulang", "a", "b"]);
        assert_ne!(again, a);
        assert_eq!(queue.remove(a).unwrap().title, "a");
        assert_eq!(queue.remove(a), None);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.clear(), 2);
        assert!(queue.is_empty());
    }

    #[test]
    fn saved_queue_puts_running_jobs_first() {
        let dir = std::env::temp_dir().join(format!("danload-antrean-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("DANload").join("queue.json");
        let mut queue = Queue::default();
        queue.push_back(job("menunggu é", Platform::Facebook));
        queue.set_paused(true);
        let mut audio = job("berjalan", Platform::YouTube);
        audio.format = OutputFormat::Audio {
            format: AudioFormat::Ogg,
            bitrate_kbps: 256,
        };
        audio.file_name = Some("Nama Pilihan".into());

        let bytes = to_json([&audio], &queue).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &bytes).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.contains(r#""mode": "audio""#), "{text}");

        let (loaded, message) = load(&path);
        assert_eq!(message, None);
        assert!(loaded.is_paused());
        let jobs: Vec<&Job> = loaded.waiting().map(|waiting| &waiting.job).collect();
        assert_eq!(jobs, [&audio, &job("menunggu é", Platform::Facebook)]);
    }

    #[test]
    fn damaged_or_partly_unknown_files_are_handled() {
        let dir =
            std::env::temp_dir().join(format!("danload-antrean-rusak-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("queue.json");
        assert!(load(&path).0.is_empty(), "file belum ada = antrean kosong");

        // Satu unduhan dengan format yang tidak dikenal dilewati; yang lain tetap terbaca.
        let mut good = serde_json::to_value(job("baik", Platform::YouTube)).unwrap();
        good["kolom_baru"] = Value::Bool(true);
        let file = serde_json::json!({
            "version": 1,
            "jobs": [good, { "url": "x", "platform": "youtube", "format": { "mode": "video", "format": "8k" } }]
        });
        fs::write(&path, file.to_string()).unwrap();
        let (queue, message) = load(&path);
        assert_eq!(titles(&queue), ["baik"]);
        assert!(message.unwrap().starts_with("1 unduhan"));

        fs::write(&path, "{ rusak").unwrap();
        let (queue, message) = load(&path);
        assert!(queue.is_empty());
        assert!(message.unwrap().contains("queue-rusak-"));
        assert!(!path.exists(), "file rusak dipindah ke cadangan");
    }

    #[test]
    fn job_becomes_a_download_request() {
        let mut item = job("Judul", Platform::YouTube);
        assert_eq!(item.display_title(), "Judul");
        item.title = "  ".into();
        assert_eq!(item.display_title(), "https://contoh/Judul");
        let request = item.request(Some(PathBuf::from("/thumbs")));
        assert_eq!(request.url, item.url);
        assert_eq!(request.folder, item.folder);
        assert_eq!(request.thumbnails, Some(PathBuf::from("/thumbs")));
    }
}
