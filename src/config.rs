//! Pengaturan aplikasi yang disimpan di `%APPDATA%\DANload\config.json`.
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};

use crate::formats::{
    AUDIO_BITRATES, AudioFormat, DEFAULT_AUDIO_BITRATE, OutputFormat, VIDEO_HEIGHTS, VideoFormat,
};

/// Nama folder data aplikasi di dalam `%APPDATA%` dan `%LOCALAPPDATA%`.
pub const APP_DIR_NAME: &str = "DANload";

/// Mode unduh yang terakhir dipilih pengguna.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadMode {
    #[default]
    Video,
    Audio,
}

impl DownloadMode {
    /// Indeks yang dipakai komponen `Segmented` di UI (0 = Video, 1 = Audio).
    pub fn index(self) -> i32 {
        match self {
            DownloadMode::Video => 0,
            DownloadMode::Audio => 1,
        }
    }

    /// Kebalikan dari [`DownloadMode::index`]. Angka selain 1 dianggap Video.
    pub fn from_index(index: i32) -> Self {
        if index == 1 {
            DownloadMode::Audio
        } else {
            DownloadMode::Video
        }
    }
}

/// Isi `config.json`.
///
/// `#[serde(default)]` membuat kolom yang tidak ada di file diisi nilai bawaan
/// (lihat `impl Default`), sehingga config lama tetap terbaca setelah aplikasi diperbarui.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Folder tujuan unduhan. `None` berarti pengguna belum memilih,
    /// dan aplikasi memakai [`default_download_folder`].
    pub download_folder: Option<PathBuf>,
    /// Mode unduh terakhir (Video/Audio).
    #[serde(deserialize_with = "or_default")]
    pub download_mode: DownloadMode,
    /// `true` setelah wizard setup awal selesai.
    pub setup_completed: bool,
    /// Simpan video dan audio di subfolder terpisah (`Video\` dan `Audio\`).
    pub separate_subfolders: bool,
    /// Format video terakhir (MP4, MKV, ...).
    #[serde(deserialize_with = "or_default")]
    pub video_format: VideoFormat,
    /// Format audio terakhir (MP3, M4A, ...).
    #[serde(deserialize_with = "or_default")]
    pub audio_format: AudioFormat,
    /// Resolusi maksimum video terakhir (mis. 1080). `None` = terbaik.
    pub video_max_height: Option<u32>,
    /// Bitrate audio terakhir (kbps), untuk format yang bitrate-nya bisa dipilih (MP3, OGG).
    pub audio_bitrate_kbps: u32,
    /// Jumlah unduhan yang boleh berjalan bersamaan (1–3, sejak Fase 7). Pakai
    /// [`Config::parallel_downloads`] supaya nilainya selalu dalam batas.
    #[serde(deserialize_with = "or_default_parallel")]
    pub max_parallel_downloads: u32,
}

/// Batas jumlah unduhan bersamaan yang bisa dipilih di Pengaturan.
pub const MAX_PARALLEL_DOWNLOADS: u32 = 3;

/// Membaca satu kolom pilihan. Nilai yang tidak dikenal (mis. format dari versi DANload yang
/// lebih baru) diganti nilai bawaan, supaya kolom lain di config tetap terbaca.
pub(crate) fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(T::deserialize(value).unwrap_or_default())
}

/// Seperti [`or_default`], tetapi nilai bawaannya [`MAX_PARALLEL_DOWNLOADS`] (bukan 0).
fn or_default_parallel<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(u32::deserialize(value).unwrap_or(MAX_PARALLEL_DOWNLOADS))
}

impl Default for Config {
    fn default() -> Self {
        Config {
            download_folder: None,
            download_mode: DownloadMode::default(),
            setup_completed: false,
            separate_subfolders: false,
            video_format: VideoFormat::default(),
            audio_format: AudioFormat::default(),
            video_max_height: None,
            audio_bitrate_kbps: DEFAULT_AUDIO_BITRATE,
            max_parallel_downloads: MAX_PARALLEL_DOWNLOADS,
        }
    }
}

impl Config {
    /// Membaca config dari `path`.
    ///
    /// File yang belum ada atau isinya rusak tidak dianggap error:
    /// aplikasi tetap jalan dengan pengaturan bawaan.
    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Menyimpan config ke `path`.
    ///
    /// Ditulis ke file sementara dulu lalu di-rename, supaya config lama
    /// tidak rusak kalau proses terhenti di tengah penulisan.
    pub fn save_to(&self, path: &Path) -> io::Result<()> {
        if let Some(folder) = path.parent() {
            fs::create_dir_all(folder)?;
        }
        let text = serde_json::to_string_pretty(self)?;
        let temp_path = path.with_extension("json.tmp");
        fs::write(&temp_path, text)?;
        fs::rename(&temp_path, path)
    }

    /// Jumlah unduhan yang boleh berjalan bersamaan, selalu 1 sampai
    /// [`MAX_PARALLEL_DOWNLOADS`] (nilai di luar itu, mis. config diubah manual, dibatasi).
    pub fn parallel_downloads(&self) -> usize {
        self.max_parallel_downloads.clamp(1, MAX_PARALLEL_DOWNLOADS) as usize
    }

    /// Folder unduhan yang berlaku: pilihan pengguna, atau folder bawaan.
    pub fn effective_download_folder(&self) -> PathBuf {
        self.download_folder
            .clone()
            .unwrap_or_else(default_download_folder)
    }

    /// Format dan kualitas terakhir untuk `mode`. Nilai yang tidak ada di menu (mis. config
    /// diubah manual) diganti nilai bawaan, supaya yang diunduh sama dengan yang tampil di menu.
    pub fn output_format(&self, mode: DownloadMode) -> OutputFormat {
        match mode {
            DownloadMode::Video => OutputFormat::Video {
                format: self.video_format,
                max_height: self
                    .video_max_height
                    .filter(|height| VIDEO_HEIGHTS.contains(&Some(*height))),
            },
            DownloadMode::Audio => OutputFormat::Audio {
                format: self.audio_format,
                bitrate_kbps: Some(self.audio_bitrate_kbps)
                    .filter(|kbps| AUDIO_BITRATES.contains(kbps))
                    .unwrap_or(DEFAULT_AUDIO_BITRATE),
            },
        }
    }

    /// Folder tujuan untuk satu unduhan: folder unduhan, ditambah subfolder `Video`
    /// atau `Audio` jika pilihan subfolder aktif.
    pub fn target_folder(&self, mode: DownloadMode) -> PathBuf {
        let folder = self.effective_download_folder();
        if !self.separate_subfolders {
            return folder;
        }
        folder.join(match mode {
            DownloadMode::Video => "Video",
            DownloadMode::Audio => "Audio",
        })
    }
}

/// Lokasi `config.json`: `%APPDATA%\DANload\config.json`.
pub fn config_file_path() -> PathBuf {
    roaming_data_dir().join(APP_DIR_NAME).join("config.json")
}

/// Lokasi antrean unduhan: `%APPDATA%\DANload\queue.json` (di sebelah `config.json`).
pub fn queue_file_path() -> PathBuf {
    roaming_data_dir().join(APP_DIR_NAME).join("queue.json")
}

/// Lokasi data Pustaka: `%APPDATA%\DANload\library.json` (di sebelah `config.json`).
pub fn library_file_path() -> PathBuf {
    roaming_data_dir().join(APP_DIR_NAME).join("library.json")
}

/// Folder unduhan bawaan: `%USERPROFILE%\Downloads\DANload`.
pub fn default_download_folder() -> PathBuf {
    home_dir().join("Downloads").join(APP_DIR_NAME)
}

/// Folder profil pengguna (`C:\Users\nama`).
fn home_dir() -> PathBuf {
    env_path("USERPROFILE")
        .or_else(|| env_path("HOME"))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Folder data aplikasi yang ikut profil pengguna (`%APPDATA%`).
fn roaming_data_dir() -> PathBuf {
    env_path("APPDATA")
        .or_else(|| env_path("XDG_CONFIG_HOME"))
        .unwrap_or_else(|| home_dir().join(".config"))
}

/// Membaca variabel lingkungan sebagai path; kosong dianggap tidak ada.
pub fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Folder sementara unik untuk satu test.
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("danload-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = temp_dir("missing");
        assert_eq!(
            Config::load_from(&dir.join("config.json")),
            Config::default()
        );
    }

    #[test]
    fn save_then_load_roundtrip() {
        let dir = temp_dir("roundtrip");
        // Subfolder yang belum ada harus dibuat otomatis.
        let path = dir.join("DANload").join("config.json");
        let config = Config {
            download_folder: Some(PathBuf::from("D:\\Video Saya\\Unduhan é")),
            download_mode: DownloadMode::Audio,
            setup_completed: true,
            separate_subfolders: true,
            video_format: VideoFormat::Mpeg,
            audio_format: AudioFormat::Flac,
            video_max_height: Some(720),
            audio_bitrate_kbps: 320,
            max_parallel_downloads: 2,
        };
        config.save_to(&path).unwrap();
        // Nilai enum disimpan sebagai teks yang mudah dibaca.
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#""video_format": "mpeg""#), "{text}");
        assert!(text.contains(r#""audio_format": "flac""#), "{text}");
        assert_eq!(Config::load_from(&path), config);
        assert!(!path.with_extension("json.tmp").exists());
    }

    #[test]
    fn corrupt_file_gives_defaults() {
        let dir = temp_dir("corrupt");
        let path = dir.join("config.json");
        fs::write(&path, "{ ini bukan json").unwrap();
        assert_eq!(Config::load_from(&path), Config::default());
    }

    #[test]
    fn unknown_and_missing_fields_are_tolerated() {
        let dir = temp_dir("fields");
        let path = dir.join("config.json");
        fs::write(&path, r#"{ "download_mode": "audio", "kolom_baru": 123 }"#).unwrap();
        let config = Config::load_from(&path);
        assert_eq!(config.download_mode, DownloadMode::Audio);
        assert_eq!(config.download_folder, None);
        assert!(!config.setup_completed);
    }

    #[test]
    fn effective_folder_prefers_user_choice() {
        let chosen = PathBuf::from("E:\\Musik");
        let config = Config {
            download_folder: Some(chosen.clone()),
            ..Config::default()
        };
        assert_eq!(config.effective_download_folder(), chosen);
        assert!(
            Config::default()
                .effective_download_folder()
                .ends_with("Downloads/DANload")
        );
    }

    #[test]
    fn old_config_gets_new_defaults() {
        // config.json dari Fase 2A/2B belum punya kolom kualitas.
        let dir = temp_dir("lama");
        let path = dir.join("config.json");
        fs::write(
            &path,
            r#"{ "download_mode": "audio", "setup_completed": true, "separate_subfolders": true }"#,
        )
        .unwrap();
        let config = Config::load_from(&path);
        assert!(config.setup_completed);
        assert_eq!(config.video_max_height, None);
        assert_eq!(config.audio_bitrate_kbps, DEFAULT_AUDIO_BITRATE);
        // Config dari Fase 3 belum punya kolom format: MP4 dan MP3 seperti sebelumnya.
        assert_eq!(config.video_format, VideoFormat::Mp4);
        assert_eq!(config.audio_format, AudioFormat::Mp3);
        // Config sebelum Fase 7: 3 unduhan bersamaan.
        assert_eq!(config.parallel_downloads(), 3);
    }

    #[test]
    fn parallel_downloads_stay_within_limits() {
        let mut config = Config::default();
        for (stored, used) in [(0, 1), (1, 1), (2, 2), (3, 3), (99, 3)] {
            config.max_parallel_downloads = stored;
            assert_eq!(config.parallel_downloads(), used, "{stored}");
        }
        // Nilai yang salah jenis tidak mengulang pengaturan lain.
        let dir = temp_dir("bersamaan");
        let path = dir.join("config.json");
        fs::write(
            &path,
            r#"{ "setup_completed": true, "max_parallel_downloads": "banyak" }"#,
        )
        .unwrap();
        let config = Config::load_from(&path);
        assert!(config.setup_completed);
        assert_eq!(config.parallel_downloads(), 3);
    }

    #[test]
    fn unknown_choice_does_not_reset_other_settings() {
        let dir = temp_dir("pilihan-asing");
        let path = dir.join("config.json");
        fs::write(
            &path,
            r#"{ "download_folder": "E:\\Unduhan", "setup_completed": true,
                 "download_mode": "playlist", "video_format": "hevc", "audio_format": 7 }"#,
        )
        .unwrap();
        let config = Config::load_from(&path);
        assert!(config.setup_completed);
        assert_eq!(config.download_folder, Some(PathBuf::from("E:\\Unduhan")));
        assert_eq!(config.download_mode, DownloadMode::Video);
        assert_eq!(config.video_format, VideoFormat::Mp4);
        assert_eq!(config.audio_format, AudioFormat::Mp3);
    }

    #[test]
    fn output_format_uses_saved_choices_safely() {
        let mut config = Config {
            video_format: VideoFormat::Webm,
            audio_format: AudioFormat::Ogg,
            video_max_height: Some(1440),
            audio_bitrate_kbps: 256,
            ..Config::default()
        };
        assert_eq!(
            config.output_format(DownloadMode::Video),
            OutputFormat::Video {
                format: VideoFormat::Webm,
                max_height: Some(1440)
            }
        );
        assert_eq!(
            config.output_format(DownloadMode::Audio),
            OutputFormat::Audio {
                format: AudioFormat::Ogg,
                bitrate_kbps: 256
            }
        );
        // Nilai yang tidak ada di menu tidak dipakai.
        config.video_max_height = Some(999);
        config.audio_bitrate_kbps = 100;
        assert_eq!(
            config.output_format(DownloadMode::Video),
            OutputFormat::Video {
                format: VideoFormat::Webm,
                max_height: None
            }
        );
        assert_eq!(
            config.output_format(DownloadMode::Audio),
            OutputFormat::Audio {
                format: AudioFormat::Ogg,
                bitrate_kbps: DEFAULT_AUDIO_BITRATE
            }
        );
    }

    #[test]
    fn target_folder_uses_subfolders_only_when_enabled() {
        let mut config = Config {
            download_folder: Some(PathBuf::from("D:\\Unduhan")),
            ..Config::default()
        };
        assert_eq!(
            config.target_folder(DownloadMode::Audio),
            PathBuf::from("D:\\Unduhan")
        );
        config.separate_subfolders = true;
        assert!(config.target_folder(DownloadMode::Audio).ends_with("Audio"));
        assert!(config.target_folder(DownloadMode::Video).ends_with("Video"));
    }

    #[test]
    fn mode_index_roundtrip() {
        assert_eq!(DownloadMode::from_index(0), DownloadMode::Video);
        assert_eq!(DownloadMode::from_index(1), DownloadMode::Audio);
        assert_eq!(DownloadMode::from_index(99), DownloadMode::Video);
        assert_eq!(DownloadMode::Audio.index(), 1);
    }
}
