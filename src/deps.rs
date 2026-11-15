//! Mencari komponen eksternal (yt-dlp, FFmpeg, ffprobe, Deno) dan membaca versinya.
//!
//! Urutan pencarian (CLAUDE.md Bagian 2.5):
//! 1. `%LOCALAPPDATA%\DANload\bin\`   - tempat wizard setup memasang komponen
//! 2. folder `bin\` di samping `DANload.exe`
//! 3. semua folder di variabel lingkungan `PATH`
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).

use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::{APP_DIR_NAME, env_path};
use crate::process::hide_console_window;

/// Batas waktu menunggu sebuah komponen menjawab perintah versi.
/// yt-dlp.exe perlu beberapa detik saat pertama kali dijalankan.
const VERSION_TIMEOUT: Duration = Duration::from_secs(30);

/// Komponen eksternal yang dibutuhkan DANload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Component {
    YtDlp,
    Ffmpeg,
    Ffprobe,
    /// Runtime JavaScript yang dibutuhkan yt-dlp untuk YouTube.
    Deno,
}

impl Component {
    /// Semua komponen, sesuai urutan tampil di halaman Pengaturan.
    pub const ALL: [Component; 4] = [
        Component::YtDlp,
        Component::Ffmpeg,
        Component::Ffprobe,
        Component::Deno,
    ];

    /// Nama untuk ditampilkan ke pengguna.
    pub fn display_name(self) -> &'static str {
        match self {
            Component::YtDlp => "yt-dlp",
            Component::Ffmpeg => "FFmpeg",
            Component::Ffprobe => "ffprobe",
            Component::Deno => "Deno",
        }
    }

    /// Penjelasan singkat kegunaannya.
    pub fn purpose(self) -> &'static str {
        match self {
            Component::YtDlp => "Mengambil video dan audio dari situs sumber",
            Component::Ffmpeg => "Menggabungkan dan mengubah format file",
            Component::Ffprobe => "Membaca informasi file media (satu paket dengan FFmpeg)",
            Component::Deno => "Menjalankan JavaScript yang dibutuhkan yt-dlp untuk YouTube",
        }
    }

    /// Nama file program di komputer ini, mis. `yt-dlp.exe` di Windows.
    pub fn file_name(self) -> String {
        let name = self.windows_file_name();
        if cfg!(windows) {
            name.to_string()
        } else {
            // Di Linux/macOS (dipakai saat pengujian) program tidak berakhiran `.exe`.
            name.trim_end_matches(".exe").to_string()
        }
    }

    /// Nama file program versi Windows. Inilah nama yang dicari di dalam arsip unduhan.
    pub fn windows_file_name(self) -> &'static str {
        match self {
            Component::YtDlp => "yt-dlp.exe",
            Component::Ffmpeg => "ffmpeg.exe",
            Component::Ffprobe => "ffprobe.exe",
            Component::Deno => "deno.exe",
        }
    }

    /// Argumen untuk menanyakan versi.
    fn version_arg(self) -> &'static str {
        match self {
            Component::YtDlp | Component::Deno => "--version",
            Component::Ffmpeg | Component::Ffprobe => "-version",
        }
    }

    /// Versi paling lama yang masih bisa dipakai, jika ada batasnya.
    ///
    /// - yt-dlp: dukungan runtime JavaScript untuk YouTube baru ada sejak 2025.11.12.
    /// - Deno: yt-dlp butuh Deno 2.3.0 atau lebih baru.
    pub fn minimum_version(self) -> Option<&'static str> {
        match self {
            Component::YtDlp => Some("2025.11.12"),
            Component::Deno => Some("2.3.0"),
            Component::Ffmpeg | Component::Ffprobe => None,
        }
    }
}

/// Hasil pemeriksaan satu komponen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// Ditemukan dan bisa dijalankan.
    Found { path: PathBuf, version: String },
    /// File ada, tetapi gagal dijalankan (rusak, diblokir antivirus, dsb.).
    Broken { path: PathBuf, reason: String },
    /// Tidak ditemukan di lokasi mana pun.
    Missing,
}

/// Folder tempat wizard memasang komponen: `%LOCALAPPDATA%\DANload\bin\`.
pub fn local_bin_dir() -> Option<PathBuf> {
    env_path("LOCALAPPDATA")
        .or_else(|| env_path("XDG_DATA_HOME"))
        .map(|base| base.join(APP_DIR_NAME).join("bin"))
}

/// Daftar folder yang diperiksa, sesuai urutan prioritas.
pub fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = local_bin_dir() {
        dirs.push(dir);
    }
    if let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        dirs.push(exe_dir.join("bin"));
    }
    if let Some(path_var) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path_var).filter(|dir| !dir.as_os_str().is_empty()));
    }
    dirs
}

/// Mencari file komponen di `dirs`; folder yang lebih awal menang.
pub fn find_in(component: Component, dirs: &[PathBuf]) -> Option<PathBuf> {
    let file_name = component.file_name();
    dirs.iter()
        .map(|dir| dir.join(&file_name))
        .find(|candidate| candidate.is_file())
}

/// Memeriksa satu komponen: cari filenya di `dirs` (biasanya hasil [`search_dirs`]),
/// lalu coba jalankan untuk membaca versinya.
///
/// Fungsi ini bisa memakan beberapa detik, jadi **jangan dipanggil dari UI thread**.
pub fn check_in(component: Component, dirs: &[PathBuf]) -> Status {
    let Some(path) = find_in(component, dirs) else {
        return Status::Missing;
    };
    match read_version(component, &path) {
        Ok(version) => match component.minimum_version() {
            Some(minimum) if !is_at_least(&version, minimum) => Status::Broken {
                path,
                reason: format!(
                    "Versi {version} terlalu lama; butuh versi {minimum} atau lebih baru. \
                     Perbarui lewat \"Pasang / perbarui...\"."
                ),
            },
            _ => Status::Found { path, version },
        },
        Err(reason) => Status::Broken { path, reason },
    }
}

/// Angka-angka di awal nomor versi, mis. `2.5.6` → `[2, 5, 6]`,
/// `2026.11.13.233954` → `[2026, 11, 13, 233954]`, `9.0.2-essentials` → `[9, 0, 2]`.
fn leading_numbers(version: &str) -> Vec<u64> {
    let mut numbers = Vec::new();
    for part in version.split('.') {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        let Ok(number) = digits.parse::<u64>() else {
            break;
        };
        numbers.push(number);
        if digits.len() != part.len() {
            break; // Bagian seperti `2-essentials`: berhenti setelah angkanya.
        }
    }
    numbers
}

/// `true` jika `version` sama dengan atau lebih baru dari `minimum`.
///
/// Nomor versi yang tidak bisa dibaca dianggap cukup, supaya format versi yang tidak
/// dikenal tidak membuat komponen yang sebenarnya berfungsi jadi ditolak.
pub fn is_at_least(version: &str, minimum: &str) -> bool {
    let have = leading_numbers(version);
    if have.is_empty() {
        return true;
    }
    for (index, need) in leading_numbers(minimum).into_iter().enumerate() {
        let got = have.get(index).copied().unwrap_or(0);
        if got != need {
            return got > need;
        }
    }
    true
}

/// Menjalankan komponen dengan argumen versi dan mengambil nomor versinya.
fn read_version(component: Component, path: &Path) -> Result<String, String> {
    let output = run_capture(path, [component.version_arg()], VERSION_TIMEOUT)?;
    parse_version(component, &output).ok_or_else(|| "Jawaban program tidak dikenali.".to_string())
}

/// Mengambil nomor versi dari keluaran program.
///
/// - yt-dlp mencetak versinya saja, mis. `2026.10.31`
/// - FFmpeg/ffprobe mencetak `ffmpeg version 7.1-essentials_build ... Copyright ...`
/// - Deno mencetak `deno 2.5.6 (stable, release, x86_64-pc-windows-msvc)`
pub fn parse_version(component: Component, output: &str) -> Option<String> {
    let first_line = output
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    match component {
        Component::YtDlp => {
            // Harus diawali angka, supaya pesan error tidak dianggap versi.
            first_line
                .starts_with(|c: char| c.is_ascii_digit())
                .then(|| first_line.to_string())
        }
        Component::Ffmpeg | Component::Ffprobe => {
            let mut words = first_line.split_whitespace();
            let (_program, keyword, version) = (words.next()?, words.next()?, words.next()?);
            (keyword == "version").then(|| version.to_string())
        }
        Component::Deno => {
            let mut words = first_line.split_whitespace();
            let (program, version) = (words.next()?, words.next()?);
            (program == "deno" && version.starts_with(|c: char| c.is_ascii_digit()))
                .then(|| version.to_string())
        }
    }
}

/// Menjalankan program singkat (mis. perintah versi atau ffprobe) dan mengembalikan teks
/// keluarannya (stdout). Program dihentikan jika tidak selesai dalam `timeout`.
///
/// Argumen diberikan sebagai daftar terpisah (bukan satu string shell),
/// sehingga path berisi spasi atau karakter aneh tetap aman.
pub(crate) fn run_capture<I, S>(
    program: &Path,
    args: I,
    timeout: Duration,
) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    hide_console_window(&mut command);

    let mut child = command
        .spawn()
        .map_err(|error| format!("Tidak bisa dijalankan: {error}"))?;

    // Stdout dibaca di thread lain selagi program berjalan. Kalau baru dibaca setelah program
    // selesai, program bisa macet sendiri saat keluarannya melebihi kapasitas pipe.
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.map(|mut stdout| stdout.read_to_end(&mut bytes));
        (bytes, result)
    });

    // Tunggu sampai selesai, tetapi jangan selamanya: kalau macet, hentikan. Saat dihentikan,
    // thread pembaca tidak ditunggu, karena proses anak yang tertinggal bisa membuat pipe-nya
    // tetap terbuka; thread itu selesai sendiri begitu pipe tertutup.
    let started = Instant::now();
    let exit_status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Program tidak menjawab.".to_string());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(format!("Gagal menunggu program: {error}")),
        }
    };

    let (bytes, result) = reader
        .join()
        .map_err(|_| "Gagal membaca jawaban program.".to_string())?;
    if let Some(Err(error)) = result {
        return Err(format!("Gagal membaca jawaban program: {error}"));
    }
    if !exit_status.success() {
        return Err(format!("Program berhenti dengan {exit_status}."));
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("danload-deps-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_yt_dlp_version() {
        assert_eq!(
            parse_version(Component::YtDlp, "2026.10.31\r\n"),
            Some("2026.10.31".to_string())
        );
        assert_eq!(parse_version(Component::YtDlp, "ERROR: something"), None);
        assert_eq!(parse_version(Component::YtDlp, ""), None);
    }

    #[test]
    fn parses_ffmpeg_version() {
        let output = "ffmpeg version 7.1-essentials_build-www.gyan.dev Copyright (c) 2000-2024 the FFmpeg developers\nbuilt with gcc 14.2.0\n";
        assert_eq!(
            parse_version(Component::Ffmpeg, output),
            Some("7.1-essentials_build-www.gyan.dev".to_string())
        );
        assert_eq!(
            parse_version(Component::Ffprobe, "ffprobe version N-1234-gabc Copyright"),
            Some("N-1234-gabc".to_string())
        );
        assert_eq!(
            parse_version(Component::Ffmpeg, "bukan keluaran ffmpeg"),
            None
        );
    }

    #[test]
    fn parses_deno_version() {
        let output = "deno 2.5.6 (stable, release, x86_64-pc-windows-msvc)\nv8 14.0.365.5-rusty\ntypescript 5.9.2\n";
        assert_eq!(
            parse_version(Component::Deno, output),
            Some("2.5.6".to_string())
        );
        assert_eq!(parse_version(Component::Deno, "error: unknown"), None);
        assert_eq!(parse_version(Component::Deno, "deno"), None);
    }

    #[test]
    fn compares_versions() {
        // Deno
        assert!(is_at_least("2.3.0", "2.3.0"));
        assert!(is_at_least("2.5.6", "2.3.0"));
        assert!(is_at_least("10.0.0", "2.3.0"));
        assert!(!is_at_least("2.2.9", "2.3.0"));
        assert!(!is_at_least("1.46.3", "2.3.0"));
        // yt-dlp (tanggal), termasuk versi nightly yang punya angka keempat
        assert!(is_at_least("2025.11.12", "2025.11.12"));
        assert!(is_at_least("2026.11.13.233954", "2025.11.12"));
        assert!(!is_at_least("2025.10.22", "2025.11.12"));
        assert!(!is_at_least("2024.12.31", "2025.11.12"));
        // Format yang tidak dikenal tidak ditolak
        assert!(is_at_least("N-121234-gabcdef", "2.3.0"));
        assert!(is_at_least("", "2.3.0"));
    }

    #[test]
    fn leading_numbers_stop_at_text() {
        assert_eq!(leading_numbers("9.0.2-essentials_build"), vec![9, 0, 2]);
        assert_eq!(leading_numbers("2.5"), vec![2, 5]);
        assert_eq!(leading_numbers("7.1rc1.5"), vec![7, 1]);
        assert!(leading_numbers("abc").is_empty());
    }

    #[test]
    fn missing_when_not_in_any_dir() {
        let dir = temp_dir("missing");
        assert_eq!(check_in(Component::YtDlp, &[dir]), Status::Missing);
    }

    #[test]
    fn earlier_dir_wins() {
        let first = temp_dir("first");
        let second = temp_dir("second");
        let name = Component::Ffmpeg.file_name();
        fs::write(second.join(&name), "x").unwrap();
        assert_eq!(
            find_in(Component::Ffmpeg, &[first.clone(), second.clone()]),
            Some(second.join(&name))
        );
        fs::write(first.join(&name), "x").unwrap();
        assert_eq!(
            find_in(Component::Ffmpeg, &[first.clone(), second]),
            Some(first.join(&name))
        );
    }

    #[test]
    fn folder_with_component_name_is_ignored() {
        let dir = temp_dir("folder");
        fs::create_dir_all(dir.join(Component::YtDlp.file_name())).unwrap();
        assert_eq!(find_in(Component::YtDlp, &[dir]), None);
    }

    #[test]
    fn unrunnable_file_is_broken() {
        let dir = temp_dir("broken");
        let path = dir.join(Component::YtDlp.file_name());
        fs::write(&path, "ini bukan program").unwrap();
        match check_in(Component::YtDlp, &[dir]) {
            Status::Broken { path: found, .. } => assert_eq!(found, path),
            other => panic!("seharusnya Broken, ternyata {other:?}"),
        }
    }

    /// Menjalankan program palsu sungguhan (hanya di Linux/macOS, memakai skrip shell).
    #[cfg(unix)]
    #[test]
    fn runs_fake_program_and_reads_version() {
        use std::os::unix::fs::PermissionsExt;

        // Nama folder sengaja memakai spasi dan huruf beraksen.
        let dir = temp_dir("folder dengan spasi é");
        let path = dir.join(Component::Ffmpeg.file_name());
        fs::write(
            &path,
            "#!/bin/sh\necho \"ffmpeg version 9.9-test Copyright\"\n",
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(
            check_in(Component::Ffmpeg, &[dir]),
            Status::Found {
                path,
                version: "9.9-test".to_string()
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn outdated_deno_is_reported() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("deno-lama");
        let path = dir.join(Component::Deno.file_name());
        fs::write(
            &path,
            "#!/bin/sh\necho \"deno 1.46.3 (stable, release, x86_64)\"\n",
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        match check_in(Component::Deno, &[dir]) {
            Status::Broken { reason, .. } => assert!(reason.contains("1.46.3"), "{reason}"),
            other => panic!("seharusnya Broken, ternyata {other:?}"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn failing_program_is_broken() {
        use std::os::unix::fs::PermissionsExt;

        let dir = temp_dir("failing");
        let path = dir.join(Component::YtDlp.file_name());
        fs::write(&path, "#!/bin/sh\nexit 3\n").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        assert!(matches!(
            check_in(Component::YtDlp, &[dir]),
            Status::Broken { .. }
        ));
    }
}
