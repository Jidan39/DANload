//! Membaca isi playlist YouTube (sejak Fase 7), supaya pengguna bisa memilih videonya.
//!
//! yt-dlp dijalankan dengan `--flat-playlist`: hanya daftar videonya yang dibaca (cepat, tanpa
//! membuka tiap video). Tiap video dicetak sebagai baris `DANLOAD|entry|{...}` (objek JSON), lalu
//! data playlist-nya sebagai `DANLOAD|playlist|{...}`. Video yang dipilih kemudian masuk antrean
//! sebagai unduhan biasa, satu link per video (`src/queue.rs`).
//!
//! Modul ini tidak bergantung pada GUI. [`read`] lambat: **jangan dipanggil dari UI thread**.

use std::ffi::OsString;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::downloader::{self, CancelHandle, Tools};
use crate::process;

/// Jeda minimal antar laporan jumlah video yang sudah terbaca.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Awalan baris yang dicetak khusus untuk DANload.
const LINE_PREFIX: &str = "DANLOAD|";

/// Data tiap video. `%(...)j` menulis objek JSON; kolom yang kosong tidak ikut ditulis.
const ENTRY_TEMPLATE: &str = "DANLOAD|entry|%(.{id,title,url,duration,availability,live_status})j";

/// Data playlist-nya, dicetak setelah semua video terbaca.
const PLAYLIST_TEMPLATE: &str = "playlist:DANLOAD|playlist|%(.{title})j";

/// Satu video di playlist.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub title: String,
    /// Link videonya, mis. `https://www.youtube.com/watch?v=ID`.
    pub url: String,
    /// Durasi dalam detik, jika diketahui.
    pub duration: Option<f64>,
    /// Alasan video ini tidak bisa diunduh (privat, dihapus, belum tayang). `None` = bisa.
    pub unavailable: Option<&'static str>,
}

/// Isi sebuah playlist.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Playlist {
    /// Judul playlist (kosong jika tidak diketahui).
    pub title: String,
    /// Video-videonya, urut seperti di playlist.
    pub entries: Vec<Entry>,
    /// Keterangan jika sebagian isi playlist tidak terbaca.
    pub note: Option<String>,
}

impl Playlist {
    /// Jumlah video yang bisa diunduh.
    pub fn available(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.unavailable.is_none())
            .count()
    }
}

/// Hasil membaca playlist.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Done(Playlist),
    /// Gagal; pesannya siap ditampilkan ke pengguna.
    Failed(String),
    /// Dibatalkan pengguna.
    Cancelled,
}

/// Argumen yt-dlp untuk membaca playlist `url`.
pub fn build_args(url: &str, tools: &Tools) -> Vec<OsString> {
    let fixed = [
        "--ignore-config",
        "--encoding",
        "utf-8",
        "--color",
        "never",
        // Hanya daftar videonya, tanpa membuka tiap video.
        "--flat-playlist",
        // Link `watch?v=...&list=...`: baca playlist-nya, bukan videonya saja.
        "--yes-playlist",
        "--print",
        ENTRY_TEMPLATE,
        "--print",
        PLAYLIST_TEMPLATE,
    ];
    let mut args: Vec<OsString> = fixed.iter().map(OsString::from).collect();
    let mut deno = OsString::from("deno:");
    deno.push(tools.deno.as_os_str());
    args.push("--js-runtimes".into());
    args.push(deno);
    args.push("--".into());
    args.push(url.trim().into());
    args
}

/// Membaca isi playlist `url` sampai selesai.
///
/// - `cancel`: dipakai pemanggil untuk membatalkan dari thread lain.
/// - `on_progress`: jumlah video yang sudah terbaca (paling sering sekitar 10 kali per detik).
pub fn read(
    url: &str,
    tools: &Tools,
    cancel: &CancelHandle,
    on_progress: &mut dyn FnMut(usize),
) -> Outcome {
    let outcome = read_inner(url, tools, cancel, on_progress);
    cancel.mark_finished();
    outcome
}

fn read_inner(
    url: &str,
    tools: &Tools,
    cancel: &CancelHandle,
    on_progress: &mut dyn FnMut(usize),
) -> Outcome {
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    let mut command = Command::new(&tools.yt_dlp);
    command
        .args(build_args(url, tools))
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
    drop(sender);

    let mut output = Output::default();
    let mut reported = 0;
    let mut last_report: Option<Instant> = None;
    loop {
        let due = last_report.is_none_or(|time| time.elapsed() >= PROGRESS_INTERVAL);
        if due && output.playlist.entries.len() != reported {
            reported = output.playlist.entries.len();
            last_report = Some(Instant::now());
            on_progress(reported);
        }
        match receiver.recv_timeout(PROGRESS_INTERVAL) {
            Ok(line) => output.handle_line(&line),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    for reader in readers {
        let _ = reader.join();
    }
    let status = child.wait();
    cancel.clear_running();
    if cancel.is_cancelled() {
        return Outcome::Cancelled;
    }
    if output.playlist.entries.len() != reported {
        on_progress(output.playlist.entries.len());
    }

    let failure = match status {
        Ok(status) if status.success() => None,
        Ok(status) => Some(output.failure_message(status.code())),
        Err(error) => Some(format!("Gagal menunggu yt-dlp selesai: {error}")),
    };
    output.finish(failure)
}

/// Keadaan yang dikumpulkan dari keluaran yt-dlp.
#[derive(Default)]
struct Output {
    playlist: Playlist,
    /// Baris video yang tidak bisa dibaca.
    skipped: usize,
    first_error: Option<String>,
    last_other_line: Option<String>,
}

impl Output {
    fn handle_line(&mut self, line: &str) {
        let Some(rest) = line.trim_end().strip_prefix(LINE_PREFIX) else {
            let line = line.trim();
            if line.starts_with("ERROR:") {
                self.first_error.get_or_insert_with(|| line.to_string());
            } else if !line.is_empty() && !line.starts_with("WARNING:") {
                self.last_other_line = Some(line.to_string());
            }
            return;
        };
        match rest.split_once('|') {
            Some(("entry", data)) => match parse_entry(data) {
                Some(entry) => self.playlist.entries.push(entry),
                None => self.skipped += 1,
            },
            Some(("playlist", data)) => {
                #[derive(Deserialize)]
                struct Printed {
                    title: Option<String>,
                }
                if let Ok(Printed { title: Some(title) }) = serde_json::from_str(data.trim()) {
                    self.playlist.title = title.trim().to_string();
                }
            }
            _ => {}
        }
    }

    fn failure_message(&self, exit_code: Option<i32>) -> String {
        match self.first_error.as_ref().or(self.last_other_line.as_ref()) {
            Some(line) => downloader::known_error(line).unwrap_or_else(|| {
                format!("Playlist gagal dibaca: {}", downloader::error_detail(line))
            }),
            None => match exit_code {
                Some(code) => format!("yt-dlp berhenti tanpa pesan (kode {code})."),
                None => "yt-dlp berhenti tanpa pesan.".to_string(),
            },
        }
    }

    /// Hasil akhir. Playlist yang terbaca sebagian tetap bisa dipilih, dengan keterangan.
    fn finish(mut self, failure: Option<String>) -> Outcome {
        if self.playlist.entries.is_empty() {
            return Outcome::Failed(failure.unwrap_or_else(|| {
                "Playlist ini kosong, atau semua videonya disembunyikan YouTube.".to_string()
            }));
        }
        let mut notes = Vec::new();
        if self.skipped > 0 {
            notes.push(format!("{} video tidak bisa dibaca.", self.skipped));
        }
        if let Some(failure) = failure {
            notes.push(format!("Isi playlist mungkin belum lengkap: {failure}"));
        }
        self.playlist.note = (!notes.is_empty()).then(|| notes.join(" "));
        Outcome::Done(self.playlist)
    }
}

/// Isi baris `entry` (objek JSON dari yt-dlp). `None` jika tidak ada link maupun ID videonya.
fn parse_entry(data: &str) -> Option<Entry> {
    #[derive(Deserialize)]
    struct Printed {
        id: Option<String>,
        title: Option<String>,
        url: Option<String>,
        duration: Option<f64>,
        availability: Option<String>,
        live_status: Option<String>,
    }
    let printed: Printed = serde_json::from_str(data.trim()).ok()?;
    let id = printed.id.filter(|id| !id.trim().is_empty());
    let url = match printed.url.map(|url| url.trim().to_string()) {
        Some(url) if url.starts_with("https://") || url.starts_with("http://") => url,
        _ => format!("https://www.youtube.com/watch?v={}", id.as_deref()?.trim()),
    };
    let title = printed
        .title
        .map(|title| title.trim().to_string())
        .filter(|title| !title.is_empty())
        .or(id)
        .unwrap_or_else(|| url.clone());
    let unavailable = unavailable_reason(
        &title,
        printed.availability.as_deref(),
        printed.live_status.as_deref(),
    );
    Some(Entry {
        title,
        url,
        duration: printed
            .duration
            .filter(|seconds| seconds.is_finite() && *seconds > 0.0),
        unavailable,
    })
}

/// Alasan sebuah video di playlist tidak bisa diunduh, dari data `--flat-playlist` yt-dlp.
/// YouTube menampilkan video privat dan yang sudah dihapus dengan judul pengganti.
fn unavailable_reason(
    title: &str,
    availability: Option<&str>,
    live_status: Option<&str>,
) -> Option<&'static str> {
    match (title, availability, live_status) {
        ("[Deleted video]", _, _) => Some("Video sudah dihapus"),
        ("[Private video]", _, _) | (_, Some("private"), _) => Some("Video privat"),
        ("[Unavailable video]", _, _) => Some("Video tidak tersedia"),
        (_, Some("premium_only" | "subscriber_only"), _) => Some("Khusus anggota atau Premium"),
        (_, Some("needs_auth"), _) => Some("Butuh login"),
        (_, _, Some("is_upcoming")) => Some("Belum tayang"),
        (_, _, Some("is_live")) => Some("Sedang siaran langsung"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn output_from(lines: &[&str]) -> Output {
        let mut output = Output::default();
        for line in lines {
            output.handle_line(line);
        }
        output
    }

    #[test]
    fn entries_and_title_are_read() {
        let output = output_from(&[
            r#"DANLOAD|entry|{"id": "a1", "title": "Lagu \u00e9 | satu", "url": "https://www.youtube.com/watch?v=a1", "duration": 215}"#,
            r#"DANLOAD|entry|{"id": "a2", "title": "[Private video]", "url": "https://www.youtube.com/watch?v=a2"}"#,
            r#"DANLOAD|entry|{"id": "a3", "title": "Pendek", "url": "https://www.youtube.com/shorts/a3", "duration": 31.5}"#,
            r#"DANLOAD|entry|{"id": "a4", "title": "Nanti", "live_status": "is_upcoming"}"#,
            "WARNING: [youtube:tab] YouTube said: INFO - 1 unavailable video is hidden",
            r#"DANLOAD|playlist|{"title": "Playlist Saya"}"#,
        ]);
        let Outcome::Done(playlist) = output.finish(None) else {
            panic!("harus berhasil");
        };
        assert_eq!(playlist.title, "Playlist Saya");
        assert_eq!(playlist.note, None);
        assert_eq!(playlist.entries.len(), 4);
        assert_eq!(playlist.available(), 2);
        let first = &playlist.entries[0];
        assert_eq!(first.title, "Lagu é | satu");
        assert_eq!(first.duration, Some(215.0));
        assert_eq!(first.unavailable, None);
        assert_eq!(playlist.entries[1].unavailable, Some("Video privat"));
        assert_eq!(playlist.entries[2].url, "https://www.youtube.com/shorts/a3");
        // Tanpa link: dibuat dari ID-nya.
        assert_eq!(
            playlist.entries[3].url,
            "https://www.youtube.com/watch?v=a4"
        );
        assert_eq!(playlist.entries[3].unavailable, Some("Belum tayang"));
    }

    #[test]
    fn unavailable_videos_are_recognized() {
        let reason = |title, availability, live| unavailable_reason(title, availability, live);
        assert_eq!(
            reason("[Deleted video]", None, None),
            Some("Video sudah dihapus")
        );
        assert_eq!(reason("Judul", Some("private"), None), Some("Video privat"));
        assert_eq!(
            reason("Judul", Some("subscriber_only"), None),
            Some("Khusus anggota atau Premium")
        );
        assert_eq!(
            reason("Judul", Some("needs_auth"), None),
            Some("Butuh login")
        );
        assert_eq!(
            reason("Judul", None, Some("is_live")),
            Some("Sedang siaran langsung")
        );
        for (availability, live) in [
            (Some("public"), None),
            (Some("unlisted"), Some("was_live")),
            (None, Some("not_live")),
            (None, Some("post_live")),
        ] {
            assert_eq!(reason("Judul", availability, live), None);
        }
    }

    #[test]
    fn broken_lines_are_counted_not_fatal() {
        let output = output_from(&[
            r#"DANLOAD|entry|{"id": "a1", "title": "Satu"}"#,
            "DANLOAD|entry|{rusak",
            r#"DANLOAD|entry|{"title": "tanpa id dan link"}"#,
            r#"DANLOAD|playlist|{"title": null}"#,
        ]);
        let Outcome::Done(playlist) = output.finish(None) else {
            panic!("harus berhasil");
        };
        assert_eq!(playlist.title, "");
        assert_eq!(playlist.entries.len(), 1);
        assert_eq!(playlist.note.as_deref(), Some("2 video tidak bisa dibaca."));
    }

    #[test]
    fn failures_are_explained() {
        let output = output_from(&[
            "ERROR: [youtube:tab] PLxyz: YouTube said: The playlist does not exist.",
        ]);
        let message = output.failure_message(Some(1));
        assert!(message.starts_with("Playlist tidak ditemukan"), "{message}");
        let Outcome::Failed(failed) = output.finish(Some(message.clone())) else {
            panic!("tanpa video harus gagal");
        };
        assert_eq!(failed, message);

        let unknown = output_from(&["ERROR: [youtube:tab] PLxyz: sesuatu yang aneh"]);
        assert_eq!(
            unknown.failure_message(Some(1)),
            "Playlist gagal dibaca: sesuatu yang aneh"
        );
        assert!(matches!(
            Output::default().finish(None),
            Outcome::Failed(message) if message.contains("kosong")
        ));

        // Sebagian sudah terbaca sebelum gagal: tetap bisa dipilih, dengan keterangan.
        let partial = output_from(&[r#"DANLOAD|entry|{"id": "a1", "title": "Satu"}"#]);
        let Outcome::Done(playlist) = partial.finish(Some("Tidak bisa terhubung.".into())) else {
            panic!("sebagian harus tetap berhasil");
        };
        assert!(playlist.note.unwrap().contains("belum lengkap"));
    }

    /// Uji dengan yt-dlp sungguhan: baca playlist, lalu batal di tengah pembacaan. Tidak jalan
    /// otomatis; jalankan dengan, misalnya:
    /// `DANLOAD_TEST_YTDLP=... DANLOAD_TEST_DENO=... DANLOAD_TEST_PLAYLIST_URL=...
    ///  cargo test real_playlist -- --ignored --nocapture`
    #[test]
    #[ignore = "butuh yt-dlp asli dan URL playlist uji"]
    fn real_playlist_read_and_cancel() {
        let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} belum diisi"));
        let tools = Tools {
            yt_dlp: env("DANLOAD_TEST_YTDLP").into(),
            ffmpeg: PathBuf::from("ffmpeg"),
            ffprobe: PathBuf::from("ffprobe"),
            deno: env("DANLOAD_TEST_DENO").into(),
        };
        let url = env("DANLOAD_TEST_PLAYLIST_URL");
        let mut counts = Vec::new();
        let handle = CancelHandle::default();
        let outcome = read(&url, &tools, &handle, &mut |count| counts.push(count));
        let Outcome::Done(playlist) = outcome else {
            panic!("playlist harus terbaca: {outcome:?}");
        };
        println!(
            "{:?}: {} video ({} bisa diunduh), laporan {counts:?}, catatan {:?}",
            playlist.title,
            playlist.entries.len(),
            playlist.available(),
            playlist.note
        );
        for entry in &playlist.entries {
            println!(
                "  {:?} {} {:?} {:?}",
                entry.title, entry.url, entry.duration, entry.unavailable
            );
        }
        assert!(handle.is_finished());
        assert!(!playlist.entries.is_empty());
        assert_eq!(counts.last(), Some(&playlist.entries.len()));
        assert!(counts.windows(2).all(|pair| pair[0] <= pair[1]));

        let handle = CancelHandle::default();
        let canceller = handle.clone();
        let started = Instant::now();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            canceller.cancel();
        });
        assert_eq!(read(&url, &tools, &handle, &mut |_| {}), Outcome::Cancelled);
        assert!(handle.is_finished());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn args_read_the_playlist_safely() {
        let tools = Tools {
            yt_dlp: PathBuf::from("yt-dlp"),
            ffmpeg: PathBuf::from("ffmpeg"),
            ffprobe: PathBuf::from("ffprobe"),
            deno: PathBuf::from(r"C:\Program Files\DANload\deno.exe"),
        };
        let url = "https://www.youtube.com/watch?v=a&list=PLAbC";
        let args: Vec<String> = build_args(&format!("  {url} "), &tools)
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args.contains(&"--flat-playlist".to_string()));
        assert!(args.contains(&"--yes-playlist".to_string()));
        assert!(!args.contains(&"--no-playlist".to_string()));
        assert!(args.contains(&r"deno:C:\Program Files\DANload\deno.exe".to_string()));
        assert_eq!(&args[args.len() - 2..], ["--", url]);
    }
}
