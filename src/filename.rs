//! Nama file hasil unduhan (CLAUDE.md Bagian 2.10).
//!
//! - [`check_custom_name`]: memeriksa nama yang diketik pengguna di kolom NAMA FILE.
//!   Kosong berarti nama mengikuti judul video.
//! - [`move_to_unique_name`]: memindahkan file hasil ke folder unduhan tanpa pernah menimpa
//!   file lain. Jika `nama.mp4` sudah ada, dipakai `nama (2).mp4`, lalu `nama (3).mp4`, dst.
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Panjang maksimum nama file tanpa ekstensi, dalam karakter. Judul yang lebih panjang dipotong
/// yt-dlp (judul Facebook sering berupa teks postingan yang panjang); nama kustom yang lebih
/// panjang ditolak. Batas ini juga menjaga path tetap di bawah batas 260 karakter Windows.
pub const MAX_NAME_CHARS: usize = 100;

/// Karakter yang tidak boleh ada di nama file Windows.
const FORBIDDEN: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Nama perangkat bawaan Windows; tidak boleh dipakai sebagai nama file, dengan ekstensi apa pun.
const RESERVED: [&str; 28] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "COM¹", "COM²", "COM³", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
    "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// Batas nomor nama kembar, mis. `nama (9999).mp4`.
const MAX_NUMBER: u32 = 9999;

/// Berapa kali memindahkan file dicoba ulang jika ditolak (mis. sedang dipindai antivirus).
const RENAME_RETRIES: u32 = 10;
const RENAME_RETRY_DELAY: Duration = Duration::from_millis(300);

/// Memeriksa nama kustom dari pengguna untuk file berekstensi `extension` (tanpa titik).
///
/// - `Ok(None)`: kolomnya kosong, jadi nama mengikuti judul video.
/// - `Ok(Some(nama))`: nama yang aman dipakai (tanpa ekstensi). Spasi di awal/akhir dan titik
///   di akhir dibuang, sama seperti yang dilakukan Windows. Jika pengguna ikut mengetik
///   ekstensinya (mis. `lagu.mp3` untuk MP3), ekstensi itu dibuang.
/// - `Err(pesan)`: nama tidak bisa dipakai; pesannya siap ditampilkan.
pub fn check_custom_name(input: &str, extension: &str) -> Result<Option<String>, String> {
    let mut name = input.trim();
    if name.is_empty() {
        return Ok(None);
    }
    if let Some(dot) = name.rfind('.')
        && name[dot + 1..].eq_ignore_ascii_case(extension)
    {
        name = &name[..dot];
    }
    let name = name.trim_end_matches(['.', ' ']).trim();
    if name.is_empty() {
        return Err("Nama file tidak boleh hanya berisi titik atau spasi.".to_string());
    }
    if name
        .chars()
        .any(|c| FORBIDDEN.contains(&c) || c.is_control())
    {
        return Err("Nama file tidak boleh berisi karakter \\ / : * ? \" < > |".to_string());
    }
    // Windows menganggap "CON.txt" atau "con .mp4" sama dengan "CON".
    let base = name.split('.').next().unwrap_or(name).trim_end();
    if RESERVED
        .iter()
        .any(|reserved| base.eq_ignore_ascii_case(reserved))
    {
        return Err(format!(
            "Nama \"{base}\" dipakai oleh Windows sendiri. Pilih nama lain."
        ));
    }
    let length = name.chars().count();
    if length > MAX_NAME_CHARS {
        return Err(format!(
            "Nama file terlalu panjang ({length} karakter, paling banyak {MAX_NAME_CHARS})."
        ));
    }
    Ok(Some(name.to_string()))
}

/// Nama file ke-`number`: `stem.ext` untuk nomor 1, `stem (2).ext` untuk nomor 2, dst.
fn numbered_name(stem: &OsStr, number: u32, extension: Option<&OsStr>) -> OsString {
    let mut name = stem.to_os_string();
    if number > 1 {
        name.push(format!(" ({number})"));
    }
    if let Some(extension) = extension {
        name.push(".");
        name.push(extension);
    }
    name
}

/// Memindahkan `file` ke `folder` dengan nama `stem` dan ekstensi yang sama dengan `file`.
/// Jika nama itu sudah dipakai, dicoba `stem (2)`, `stem (3)`, dst. File lain tidak pernah ditimpa.
///
/// Mengembalikan lokasi akhir file. `file` harus berada di drive yang sama dengan `folder`
/// (folder sementara DANload ada di dalam folder unduhan), jadi pemindahannya instan.
pub fn move_to_unique_name(file: &Path, folder: &Path, stem: &OsStr) -> io::Result<PathBuf> {
    let extension = file.extension();
    for number in 1..=MAX_NUMBER {
        let candidate = folder.join(numbered_name(stem, number, extension));
        if candidate.exists() {
            continue;
        }
        // "Pesan" nama ini dengan membuat file kosong. Ini gagal jika ada program lain yang
        // baru saja membuat file bernama sama, sehingga file itu tidak akan tertimpa.
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(placeholder) => {
                drop(placeholder);
                // File kosong milik kita sendiri yang ditimpa di sini.
                return match rename_with_retry(file, &candidate) {
                    Ok(()) => Ok(candidate),
                    Err(error) => {
                        let _ = fs::remove_file(&candidate);
                        Err(error)
                    }
                };
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::other(format!(
        "sudah ada {MAX_NUMBER} file bernama sama"
    )))
}

/// `fs::rename`, dicoba ulang beberapa kali jika ditolak. Di Windows, antivirus kadang mengunci
/// file yang baru dibuat selama beberapa saat.
pub(crate) fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut attempts = 0;
    loop {
        match fs::rename(from, to) {
            Err(error)
                if error.kind() == io::ErrorKind::PermissionDenied && attempts < RENAME_RETRIES =>
            {
                attempts += 1;
                std::thread::sleep(RENAME_RETRY_DELAY);
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("danload-nama-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn empty_custom_name_means_video_title() {
        assert_eq!(check_custom_name("", "mp4"), Ok(None));
        assert_eq!(check_custom_name("   ", "mp4"), Ok(None));
    }

    #[test]
    fn custom_name_is_tidied_like_windows_does() {
        assert_eq!(
            check_custom_name("  Lagu Favorit  ", "mp3"),
            Ok(Some("Lagu Favorit".to_string()))
        );
        // Ekstensi yang sama dengan format terpilih ikut diketik: dibuang.
        assert_eq!(
            check_custom_name("Lagu Favorit.MP3", "mp3"),
            Ok(Some("Lagu Favorit".to_string()))
        );
        // Ekstensi lain dianggap bagian nama.
        assert_eq!(
            check_custom_name("rekaman.mp4", "mp3"),
            Ok(Some("rekaman.mp4".to_string()))
        );
        // Titik dan spasi di akhir dibuang Windows, jadi dibuang juga di sini.
        assert_eq!(
            check_custom_name("Bab 1...", "mp4"),
            Ok(Some("Bab 1".to_string()))
        );
        assert_eq!(
            check_custom_name("Mr. Brown v1.2 é 日本", "mp4"),
            Ok(Some("Mr. Brown v1.2 é 日本".to_string()))
        );
    }

    #[test]
    fn invalid_custom_names_are_explained() {
        for bad in [
            "a/b",
            "a\\b",
            "judul: baru",
            "apa?",
            "\"kutip\"",
            "a<b>",
            "a|b",
            "a*",
        ] {
            let error = check_custom_name(bad, "mp4").unwrap_err();
            assert!(error.contains("tidak boleh berisi"), "{bad}: {error}");
        }
        assert!(check_custom_name("baris\nbaru", "mp4").is_err());
        for reserved in ["CON", "con", "Nul.txt", "COM1", "lpt9 ", "aux.mp4"] {
            let error = check_custom_name(reserved, "mp4").unwrap_err();
            assert!(
                error.contains("dipakai oleh Windows"),
                "{reserved}: {error}"
            );
        }
        // Nama yang hanya mirip nama perangkat tetap boleh.
        assert!(check_custom_name("CONTOH", "mp4").is_ok());
        assert!(check_custom_name("COM10", "mp4").is_ok());
        assert!(check_custom_name("...", "mp4").is_err());
        assert!(check_custom_name(".mp4", "mp4").is_err());
        let long = "a".repeat(MAX_NAME_CHARS + 1);
        assert!(
            check_custom_name(&long, "mp4")
                .unwrap_err()
                .contains("terlalu panjang")
        );
        assert!(check_custom_name(&"é".repeat(MAX_NAME_CHARS), "mp4").is_ok());
    }

    #[test]
    fn numbered_names() {
        let stem = OsStr::new("Video by akun");
        let mp4 = Some(OsStr::new("mp4"));
        assert_eq!(numbered_name(stem, 1, mp4), "Video by akun.mp4");
        assert_eq!(numbered_name(stem, 2, mp4), "Video by akun (2).mp4");
        assert_eq!(numbered_name(stem, 12, None), "Video by akun (12)");
    }

    #[test]
    fn existing_names_get_a_number_and_are_never_overwritten() {
        let dir = temp_dir("unik");
        let folder = dir.join("unduhan");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("nama.mp4"), "lama").unwrap();
        // Folder bernama sama juga tidak boleh "ditimpa".
        fs::create_dir_all(folder.join("nama (2).mp4")).unwrap();

        let staged = |content: &str| {
            let file = dir.join(format!("{content}.mp4"));
            fs::write(&file, content).unwrap();
            file
        };
        let first = move_to_unique_name(&staged("baru1"), &folder, OsStr::new("nama")).unwrap();
        let second = move_to_unique_name(&staged("baru2"), &folder, OsStr::new("nama")).unwrap();
        let other = move_to_unique_name(&staged("lain"), &folder, OsStr::new("lain")).unwrap();

        assert_eq!(first, folder.join("nama (3).mp4"));
        assert_eq!(second, folder.join("nama (4).mp4"));
        assert_eq!(other, folder.join("lain.mp4"));
        assert_eq!(fs::read_to_string(folder.join("nama.mp4")).unwrap(), "lama");
        assert_eq!(fs::read_to_string(&first).unwrap(), "baru1");
        assert_eq!(fs::read_to_string(&second).unwrap(), "baru2");
        assert!(!dir.join("baru1.mp4").exists());
    }

    #[test]
    fn failed_move_leaves_no_empty_file_behind() {
        let dir = temp_dir("gagal");
        let missing = dir.join("tidak-ada.mp4");
        assert!(move_to_unique_name(&missing, &dir, OsStr::new("hasil")).is_err());
        assert!(!dir.join("hasil.mp4").exists());
    }
}
