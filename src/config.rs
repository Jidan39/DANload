//! Pengaturan aplikasi yang disimpan di `%APPDATA%\DANload\config.json`.
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

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
/// `#[serde(default)]` membuat kolom yang tidak ada di file diisi nilai bawaan,
/// sehingga config lama tetap terbaca setelah aplikasi diperbarui.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Folder tujuan unduhan. `None` berarti pengguna belum memilih,
    /// dan aplikasi memakai [`default_download_folder`].
    pub download_folder: Option<PathBuf>,
    /// Mode unduh terakhir (Video/Audio).
    pub download_mode: DownloadMode,
    /// `true` setelah wizard setup awal selesai (dipakai mulai Fase 2B).
    pub setup_completed: bool,
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

    /// Folder unduhan yang berlaku: pilihan pengguna, atau folder bawaan.
    pub fn effective_download_folder(&self) -> PathBuf {
        self.download_folder
            .clone()
            .unwrap_or_else(default_download_folder)
    }
}

/// Lokasi `config.json`: `%APPDATA%\DANload\config.json`.
pub fn config_file_path() -> PathBuf {
    roaming_data_dir().join(APP_DIR_NAME).join("config.json")
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
        };
        config.save_to(&path).unwrap();
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
    fn mode_index_roundtrip() {
        assert_eq!(DownloadMode::from_index(0), DownloadMode::Video);
        assert_eq!(DownloadMode::from_index(1), DownloadMode::Audio);
        assert_eq!(DownloadMode::from_index(99), DownloadMode::Video);
        assert_eq!(DownloadMode::Audio.index(), 1);
    }
}
