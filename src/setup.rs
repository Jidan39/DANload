//! Memasang komponen (yt-dlp, FFmpeg, Deno) ke `%LOCALAPPDATA%\DANload\bin\`.
//!
//! Alur satu paket (CLAUDE.md Bagian 2.9):
//! 1. ambil checksum (SHA-256) resmi dari sumbernya,
//! 2. unduh file langsung ke disk sambil menghitung checksum-nya,
//! 3. bandingkan checksum; kalau beda, file dibuang,
//! 4. untuk arsip `.zip`: keluarkan hanya program yang dibutuhkan,
//! 5. pindahkan hasilnya ke folder `bin` (baru di langkah terakhir,
//!    supaya tidak pernah ada file setengah jadi di sana).
//!
//! Modul ini tidak bergantung pada GUI, jadi bisa diuji sendiri (`cargo test`).
//! Semua fungsi di sini lambat (jaringan/disk): **jangan dipanggil dari UI thread**.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::deps::Component;

/// Jeda minimal antar laporan progress, supaya UI tidak digambar ulang terlalu sering
/// (CLAUDE.md Bagian 2.6: maksimal sekitar 10 kali per detik).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Paket yang bisa dipasang. Satu paket bisa berisi lebih dari satu program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Package {
    YtDlp,
    Ffmpeg,
    Deno,
}

impl Package {
    pub const ALL: [Package; 3] = [Package::YtDlp, Package::Ffmpeg, Package::Deno];

    pub fn display_name(self) -> &'static str {
        match self {
            Package::YtDlp => "yt-dlp",
            Package::Ffmpeg => "FFmpeg",
            Package::Deno => "Deno",
        }
    }

    /// Program yang disediakan paket ini.
    pub fn components(self) -> &'static [Component] {
        match self {
            Package::YtDlp => &[Component::YtDlp],
            Package::Ffmpeg => &[Component::Ffmpeg, Component::Ffprobe],
            Package::Deno => &[Component::Deno],
        }
    }

    /// Program utama paket: program ini wajib ada saat pengguna memilih file sendiri.
    pub fn main_component(self) -> Component {
        self.components()[0]
    }
}

/// Paket pemilik sebuah nama file program, mis. `ffprobe.exe` → FFmpeg.
/// Huruf besar/kecil tidak dibedakan. `None` jika bukan program yang dikenal DANload.
pub fn package_for_file(file_name: &str) -> Option<Package> {
    Package::ALL.into_iter().find(|package| {
        package
            .components()
            .iter()
            .any(|component| component.file_name().eq_ignore_ascii_case(file_name))
    })
}

/// Bentuk file yang diunduh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Langsung berupa program (`yt-dlp.exe`).
    Executable,
    /// Arsip zip berisi program di dalam salah satu foldernya.
    Zip,
}

/// Cara mendapatkan checksum resmi sebuah file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checksum {
    /// File kecil berisi hash satu file saja, dalam format apa pun
    /// (hash saja, `hash  nama`, atau keluaran `Get-FileHash` PowerShell seperti milik Deno).
    HashFile { url: String },
    /// Daftar "hash  nama-file" untuk banyak file (gaya `sha256sum`).
    SumsFile { url: String, file_name: String },
}

/// Satu tempat unduhan untuk sebuah paket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// Nama sumber untuk ditampilkan, mis. "gyan.dev".
    pub label: String,
    pub url: String,
    pub checksum: Checksum,
    pub kind: Kind,
}

/// Daftar sumber resmi sebuah paket, dicoba berurutan sampai ada yang berhasil.
///
/// Semua URL sengaja dikumpulkan di satu fungsi ini supaya mudah diganti.
pub fn sources(package: Package) -> Vec<Source> {
    // Khusus build debug: arahkan unduhan ke server lokal untuk pengujian,
    // mis. `set DANLOAD_DEV_MIRROR=http://127.0.0.1:8000`.
    #[cfg(debug_assertions)]
    if let Ok(mirror) = std::env::var("DANLOAD_DEV_MIRROR") {
        return dev_mirror_sources(package, mirror.trim_end_matches('/'));
    }

    match package {
        Package::YtDlp => {
            let base = "https://github.com/yt-dlp/yt-dlp/releases/latest/download";
            vec![Source {
                label: "GitHub".into(),
                url: format!("{base}/yt-dlp.exe"),
                checksum: Checksum::SumsFile {
                    url: format!("{base}/SHA2-256SUMS"),
                    file_name: "yt-dlp.exe".into(),
                },
                kind: Kind::Executable,
            }]
        }
        Package::Ffmpeg => {
            let gyan = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip";
            let btbn = "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest";
            let btbn_file = "ffmpeg-master-latest-win64-gpl.zip";
            vec![
                Source {
                    label: "gyan.dev".into(),
                    url: gyan.into(),
                    checksum: Checksum::HashFile {
                        url: format!("{gyan}.sha256"),
                    },
                    kind: Kind::Zip,
                },
                Source {
                    label: "GitHub (BtbN)".into(),
                    url: format!("{btbn}/{btbn_file}"),
                    checksum: Checksum::SumsFile {
                        url: format!("{btbn}/checksums.sha256"),
                        file_name: btbn_file.into(),
                    },
                    kind: Kind::Zip,
                },
            ]
        }
        Package::Deno => {
            let zip = "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip";
            vec![Source {
                label: "GitHub (Deno)".into(),
                url: zip.into(),
                checksum: Checksum::HashFile {
                    url: format!("{zip}.sha256sum"),
                },
                kind: Kind::Zip,
            }]
        }
    }
}

#[cfg(debug_assertions)]
fn dev_mirror_sources(package: Package, mirror: &str) -> Vec<Source> {
    match package {
        Package::YtDlp => vec![Source {
            label: "server lokal".into(),
            url: format!("{mirror}/yt-dlp.exe"),
            checksum: Checksum::SumsFile {
                url: format!("{mirror}/SHA2-256SUMS"),
                file_name: "yt-dlp.exe".into(),
            },
            kind: Kind::Executable,
        }],
        Package::Ffmpeg => vec![Source {
            label: "server lokal".into(),
            url: format!("{mirror}/ffmpeg-release-essentials.zip"),
            checksum: Checksum::HashFile {
                url: format!("{mirror}/ffmpeg-release-essentials.zip.sha256"),
            },
            kind: Kind::Zip,
        }],
        Package::Deno => vec![Source {
            label: "server lokal".into(),
            url: format!("{mirror}/deno-x86_64-pc-windows-msvc.zip"),
            checksum: Checksum::HashFile {
                url: format!("{mirror}/deno-x86_64-pc-windows-msvc.zip.sha256sum"),
            },
            kind: Kind::Zip,
        }],
    }
}

/// Laporan kemajuan pemasangan, untuk ditampilkan di wizard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// Sedang menghubungi sumber.
    Connecting { package: Package, source: String },
    /// Sedang mengunduh. `total` bisa `None` kalau server tidak memberi tahu ukurannya.
    Downloading {
        package: Package,
        source: String,
        received: u64,
        total: Option<u64>,
    },
    /// Unduhan selesai, sedang memeriksa dan memasang.
    Installing { package: Package },
}

/// Kenapa pemasangan tidak selesai.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// Dibatalkan pengguna.
    Cancelled,
    /// Gagal; pesannya siap ditampilkan ke pengguna.
    Failed(String),
}

impl InstallError {
    fn failed(message: impl Into<String>) -> Self {
        InstallError::Failed(message.into())
    }
}

/// Membuat klien HTTP dengan batas waktu dan sertifikat dari Windows.
///
/// Memakai penyimpanan sertifikat sistem operasi (bukan daftar bawaan aplikasi),
/// supaya tetap berfungsi di komputer yang antivirusnya memeriksa lalu lintas HTTPS.
pub fn http_agent() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};

    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        // Batas total satu unduhan; cukup longgar untuk koneksi lambat.
        .timeout_recv_body(Some(Duration::from_secs(60 * 60)))
        .tls_config(
            TlsConfig::builder()
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .user_agent(concat!("DANload/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Mengunduh dan memasang satu paket ke `bin_dir`.
///
/// - `sources` dicoba berurutan; sumber berikutnya dipakai hanya jika yang sebelumnya gagal.
/// - `cancel` diperiksa terus selama mengunduh; kalau bernilai `true`, pemasangan berhenti.
/// - `job_id` dipakai untuk nama folder sementara, supaya dua pemasangan tidak bertabrakan.
pub fn install(
    agent: &ureq::Agent,
    package: Package,
    sources: &[Source],
    bin_dir: &Path,
    cancel: &AtomicBool,
    job_id: u64,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), InstallError> {
    let mut last_error = InstallError::failed("Tidak ada sumber unduhan.");
    for source in sources {
        match install_from_source(agent, package, source, bin_dir, cancel, job_id, on_progress) {
            Ok(()) => return Ok(()),
            Err(InstallError::Cancelled) => return Err(InstallError::Cancelled),
            Err(InstallError::Failed(message)) => {
                last_error = InstallError::Failed(format!(
                    "{} gagal dipasang dari {}. {message}",
                    package.display_name(),
                    source.label
                ));
            }
        }
    }
    Err(last_error)
}

fn install_from_source(
    agent: &ureq::Agent,
    package: Package,
    source: &Source,
    bin_dir: &Path,
    cancel: &AtomicBool,
    job_id: u64,
    on_progress: &mut dyn FnMut(Progress),
) -> Result<(), InstallError> {
    check_cancel(cancel)?;
    on_progress(Progress::Connecting {
        package,
        source: source.label.clone(),
    });

    fs::create_dir_all(bin_dir).map_err(|error| io_failure("membuat folder komponen", &error))?;
    // Folder sementara dihapus otomatis saat `temp` keluar dari cakupan,
    // baik pemasangan berhasil, gagal, maupun dibatalkan.
    let temp = TempDir::create(bin_dir.join(format!(".tmp-{job_id}")))?;

    let expected_hash = fetch_expected_hash(agent, &source.checksum)?;
    check_cancel(cancel)?;

    let download_path = temp.path.join("unduhan.part");
    let actual_hash = download_to_file(
        agent,
        &source.url,
        &download_path,
        cancel,
        &mut |received, total| {
            on_progress(Progress::Downloading {
                package,
                source: source.label.clone(),
                received,
                total,
            });
        },
    )?;

    on_progress(Progress::Installing { package });
    if !actual_hash.eq_ignore_ascii_case(&expected_hash) {
        return Err(InstallError::failed(
            "File yang diunduh tidak cocok dengan checksum resminya, jadi dibuang. Coba lagi.",
        ));
    }

    // Siapkan semua program di folder sementara dulu.
    let mut staged = Vec::new();
    match source.kind {
        Kind::Executable => {
            let component = package.main_component();
            let path = temp.path.join(component.file_name());
            fs::rename(&download_path, &path)
                .map_err(|error| io_failure("menyiapkan file", &error))?;
            staged.push((component, path));
        }
        Kind::Zip => {
            for component in package.components() {
                check_cancel(cancel)?;
                let path = temp.path.join(component.file_name());
                extract_program(&download_path, *component, &path)?;
                staged.push((*component, path));
            }
        }
    }

    // Baru setelah semuanya lengkap, pindahkan ke folder `bin`.
    for (component, path) in staged {
        make_executable(&path).map_err(|error| io_failure("menyiapkan file", &error))?;
        fs::rename(&path, bin_dir.join(component.file_name()))
            .map_err(|error| io_failure("memasang file ke folder komponen", &error))?;
    }
    Ok(())
}

/// Memasang komponen dari file yang dipilih pengguna sendiri (tanpa mengunduh).
///
/// File harus bernama persis seperti program yang dikenal DANload (mis. `yt-dlp.exe`),
/// supaya program lain tidak ikut tersalin dan dijalankan secara tidak sengaja.
/// Semua program satu paket di folder yang sama ikut disalin (mis. memilih `ffmpeg.exe`
/// juga menyalin `ffprobe.exe`). Program utama paket wajib ada di folder itu.
///
/// Mengembalikan paket yang dipasang.
pub fn install_from_file(chosen: &Path, bin_dir: &Path) -> Result<Package, InstallError> {
    let chosen_name = chosen
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(package) = package_for_file(&chosen_name) else {
        let names: Vec<String> = Package::ALL
            .iter()
            .map(|package| package.main_component().file_name())
            .collect();
        return Err(InstallError::failed(format!(
            "Pilih salah satu: {}. File yang dipilih: {chosen_name}.",
            names.join(", ")
        )));
    };

    let source_dir = chosen.parent().unwrap_or(Path::new("."));
    let main = package.main_component();
    if !source_dir.join(main.file_name()).is_file() {
        return Err(InstallError::failed(format!(
            "{} tidak ada di folder yang sama dengan {chosen_name}.",
            main.file_name()
        )));
    }

    fs::create_dir_all(bin_dir).map_err(|error| io_failure("membuat folder komponen", &error))?;
    for component in package.components() {
        let from = source_dir.join(component.file_name());
        let to = bin_dir.join(component.file_name());
        if !from.is_file() || same_file(&from, &to) {
            continue; // Tidak ada di folder itu, atau sudah berada di folder komponen.
        }
        fs::copy(&from, &to).map_err(|error| io_failure("menyalin file", &error))?;
        make_executable(&to).map_err(|error| io_failure("menyiapkan file", &error))?;
    }
    Ok(package)
}

/// Mengambil hash SHA-256 resmi dari sumbernya.
fn fetch_expected_hash(agent: &ureq::Agent, checksum: &Checksum) -> Result<String, InstallError> {
    let (url, file_name) = match checksum {
        Checksum::HashFile { url } => (url, None),
        Checksum::SumsFile { url, file_name } => (url, Some(file_name.as_str())),
    };
    let text = agent
        .get(url)
        .call()
        .map_err(|error| http_failure(&error))?
        .body_mut()
        .read_to_string()
        .map_err(|error| http_failure(&error))?;
    parse_checksum(&text, file_name)
        .ok_or_else(|| InstallError::failed("Checksum resmi tidak bisa dibaca dari sumbernya."))
}

/// Mencari hash SHA-256 (64 karakter heksadesimal) di dalam teks checksum.
///
/// - `file_name = None`: teks berisi hash satu file saja. Hash pertama yang ditemukan
///   di mana pun dalam teks yang diambil, jadi format `hash`, `hash  nama-file`, dan
///   keluaran `Get-FileHash` PowerShell (`Hash : ABC...`) sama-sama terbaca.
/// - `file_name = Some(..)`: teks berisi banyak baris `hash  nama-file`;
///   yang diambil adalah baris untuk nama file tersebut.
pub fn parse_checksum(text: &str, file_name: Option<&str>) -> Option<String> {
    let is_hash = |word: &str| word.len() == 64 && word.chars().all(|c| c.is_ascii_hexdigit());
    let Some(wanted) = file_name else {
        return text
            .split_whitespace()
            .find(|word| is_hash(word))
            .map(str::to_ascii_lowercase);
    };
    text.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let hash = words.next().filter(|word| is_hash(word))?;
        // `sha256sum` menandai mode biner dengan bintang di depan nama file.
        let listed = words.next().unwrap_or("").trim_start_matches('*');
        (listed == wanted).then(|| hash.to_ascii_lowercase())
    })
}

/// Mengunduh `url` ke `destination` secara streaming (tidak ditampung di RAM)
/// dan mengembalikan hash SHA-256 isinya dalam huruf kecil.
fn download_to_file(
    agent: &ureq::Agent,
    url: &str,
    destination: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(u64, Option<u64>),
) -> Result<String, InstallError> {
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| http_failure(&error))?;
    let total = response.body().content_length();
    let mut reader = response.body_mut().as_reader();

    let mut file =
        File::create(destination).map_err(|error| io_failure("membuat file unduhan", &error))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    let mut last_report = Instant::now();
    on_progress(0, total);

    loop {
        check_cancel(cancel)?;
        let count = match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => {
                return Err(InstallError::failed(
                    "Unduhan terputus. Periksa koneksi internet lalu coba lagi.",
                ));
            }
        };
        file.write_all(&buffer[..count])
            .map_err(|error| io_failure("menyimpan unduhan", &error))?;
        hasher.update(&buffer[..count]);
        received += count as u64;

        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = Instant::now();
            on_progress(received, total);
        }
    }
    file.flush()
        .map_err(|error| io_failure("menyimpan unduhan", &error))?;
    on_progress(received, total);

    if total.is_some_and(|total| total != received) {
        return Err(InstallError::failed(
            "Unduhan tidak lengkap. Periksa koneksi internet lalu coba lagi.",
        ));
    }
    Ok(to_hex(&hasher.finalize()))
}

/// Mengeluarkan satu program dari arsip zip ke `destination`.
///
/// Arsip FFmpeg menaruh programnya di `nama-folder/bin/ffmpeg.exe`, dan nama foldernya
/// berubah tiap versi. Karena itu yang dicocokkan hanya nama filenya.
fn extract_program(
    zip_path: &Path,
    component: Component,
    destination: &Path,
) -> Result<(), InstallError> {
    let wanted = component.windows_file_name();
    let file = File::open(zip_path).map_err(|error| io_failure("membuka arsip", &error))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| InstallError::failed("Arsip yang diunduh rusak atau bukan file zip."))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| InstallError::failed("Arsip yang diunduh rusak."))?;
        // Entri yang namanya tidak terbaca pasti bukan program yang dicari; lewati saja.
        let is_wanted = entry.is_file()
            && entry.name().is_ok_and(|name| {
                name.rsplit(['/', '\\'])
                    .next()
                    .is_some_and(|file_name| file_name.eq_ignore_ascii_case(wanted))
            });
        if !is_wanted {
            continue;
        }
        let mut output =
            File::create(destination).map_err(|error| io_failure("mengekstrak arsip", &error))?;
        io::copy(&mut entry, &mut output)
            .map_err(|error| io_failure("mengekstrak arsip", &error))?;
        return Ok(());
    }
    Err(InstallError::failed(format!(
        "{wanted} tidak ditemukan di dalam arsip."
    )))
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), InstallError> {
    if cancel.load(Ordering::Relaxed) {
        Err(InstallError::Cancelled)
    } else {
        Ok(())
    }
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// `true` jika dua path menunjuk file yang sama.
fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Di Linux/macOS file baru belum boleh dijalankan; di Windows tidak perlu apa-apa.
#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Mengubah error jaringan menjadi pesan yang mudah dipahami.
fn http_failure(error: &ureq::Error) -> InstallError {
    let message = match error {
        ureq::Error::StatusCode(404) => {
            "File tidak ditemukan di server (kode 404). Sumbernya mungkin sudah pindah.".to_string()
        }
        ureq::Error::StatusCode(code) => format!("Server menolak permintaan (kode {code})."),
        ureq::Error::Timeout(_) => {
            "Server terlalu lama menjawab. Periksa koneksi internet lalu coba lagi.".to_string()
        }
        ureq::Error::HostNotFound | ureq::Error::ConnectionFailed | ureq::Error::Io(_) => {
            "Tidak bisa terhubung ke server. Periksa koneksi internet lalu coba lagi.".to_string()
        }
        other => format!("Gagal menghubungi server: {other}"),
    };
    InstallError::Failed(message)
}

/// Mengubah error disk menjadi pesan yang mudah dipahami.
fn io_failure(action: &str, error: &io::Error) -> InstallError {
    let reason = match error.kind() {
        io::ErrorKind::StorageFull => "ruang disk tidak cukup".to_string(),
        io::ErrorKind::PermissionDenied => {
            "akses ditolak (file mungkin sedang dipakai atau diblokir antivirus)".to_string()
        }
        _ => error.to_string(),
    };
    InstallError::Failed(format!("Gagal {action}: {reason}."))
}

/// Folder sementara yang terhapus sendiri.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn create(path: PathBuf) -> Result<Self, InstallError> {
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path)
            .map_err(|error| io_failure("membuat folder sementara", &error))?;
        Ok(TempDir { path })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader};
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("danload-setup-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        to_hex(&Sha256::digest(bytes))
    }

    /// Klien HTTP untuk test: tanpa proxy, supaya langsung ke server lokal.
    fn test_agent() -> ureq::Agent {
        ureq::Agent::config_builder()
            .proxy(None)
            .timeout_global(Some(Duration::from_secs(10)))
            .build()
            .into()
    }

    /// Server HTTP mini untuk test. Mengembalikan alamat dasarnya, mis. `http://127.0.0.1:4567`.
    /// Path yang tidak terdaftar dijawab 404.
    fn serve(files: HashMap<&'static str, Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let files = Arc::new(files);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let files = Arc::clone(&files);
                std::thread::spawn(move || answer(stream, &files));
            }
        });
        format!("http://{address}")
    }

    fn answer(mut stream: TcpStream, files: &HashMap<&'static str, Vec<u8>>) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        // Buang sisa header permintaan.
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|count| count > 2) {
            line.clear();
        }
        let path = request_line.split_whitespace().nth(1).unwrap_or("/");
        let (status, body) = match files.get(path) {
            Some(body) => ("200 OK", body.as_slice()),
            None => ("404 Not Found", &b"tidak ada"[..]),
        };
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(body);
    }

    /// Membuat arsip zip di memori dengan susunan folder seperti arsip FFmpeg asli.
    fn ffmpeg_zip() -> Vec<u8> {
        use zip::write::SimpleFileOptions;
        let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        writer
            .add_directory("ffmpeg-9.9-essentials_build/bin/", options)
            .unwrap();
        for (name, body) in [
            ("ffmpeg-9.9-essentials_build/README.txt", "baca saya"),
            (
                "ffmpeg-9.9-essentials_build/bin/ffmpeg.exe",
                "#!/bin/sh\necho \"ffmpeg version 9.9-test Copyright\"\n",
            ),
            (
                "ffmpeg-9.9-essentials_build/bin/ffplay.exe",
                "tidak dipakai",
            ),
            (
                "ffmpeg-9.9-essentials_build/bin/ffprobe.exe",
                "#!/bin/sh\necho \"ffprobe version 9.9-test Copyright\"\n",
            ),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(body.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn zip_source(base: &str) -> Source {
        Source {
            label: "test".into(),
            url: format!("{base}/ffmpeg.zip"),
            checksum: Checksum::HashFile {
                url: format!("{base}/ffmpeg.zip.sha256"),
            },
            kind: Kind::Zip,
        }
    }

    fn exe_source(base: &str) -> Source {
        Source {
            label: "test".into(),
            url: format!("{base}/yt-dlp.exe"),
            checksum: Checksum::SumsFile {
                url: format!("{base}/SHA2-256SUMS"),
                file_name: "yt-dlp.exe".into(),
            },
            kind: Kind::Executable,
        }
    }

    fn no_temp_left(bin: &Path) -> bool {
        fs::read_dir(bin)
            .unwrap()
            .flatten()
            .all(|entry| !entry.file_name().to_string_lossy().starts_with(".tmp"))
    }

    #[test]
    fn parses_single_hash_file() {
        let hash = "A".repeat(64);
        assert_eq!(
            parse_checksum(&format!("{hash}\n"), None),
            Some("a".repeat(64))
        );
        assert_eq!(
            parse_checksum(&format!("{hash}  ffmpeg.zip\r\n"), None),
            Some("a".repeat(64))
        );
        assert_eq!(parse_checksum("bukan hash", None), None);
        assert_eq!(parse_checksum("", None), None);
    }

    #[test]
    fn parses_sums_file() {
        let text = format!(
            "{}  yt-dlp\n{} *yt-dlp.exe\n{}  yt-dlp_x86.exe\n",
            "1".repeat(64),
            "2".repeat(64),
            "3".repeat(64)
        );
        assert_eq!(
            parse_checksum(&text, Some("yt-dlp.exe")),
            Some("2".repeat(64))
        );
        assert_eq!(parse_checksum(&text, Some("yt-dlp")), Some("1".repeat(64)));
        assert_eq!(parse_checksum(&text, Some("tidak-ada.exe")), None);
    }

    #[test]
    fn installs_executable_and_reports_progress() {
        let body = b"#!/bin/sh\necho 2026.10.31\n".to_vec();
        let sums = format!(
            "{}  yt-dlp\n{}  yt-dlp.exe\n",
            "0".repeat(64),
            sha256_hex(&body)
        );
        let base = serve(HashMap::from([
            ("/yt-dlp.exe", body.clone()),
            ("/SHA2-256SUMS", sums.into_bytes()),
        ]));
        let bin = temp_dir("exe").join("folder spasi é").join("bin");
        let mut events = Vec::new();

        install(
            &test_agent(),
            Package::YtDlp,
            &[exe_source(&base)],
            &bin,
            &AtomicBool::new(false),
            1,
            &mut |progress| events.push(progress),
        )
        .unwrap();

        assert_eq!(
            fs::read(bin.join(Component::YtDlp.file_name())).unwrap(),
            body
        );
        assert!(no_temp_left(&bin));
        assert!(matches!(events.first(), Some(Progress::Connecting { .. })));
        assert!(events.contains(&Progress::Downloading {
            package: Package::YtDlp,
            source: "test".into(),
            received: body.len() as u64,
            total: Some(body.len() as u64),
        }));
        assert_eq!(
            events.last(),
            Some(&Progress::Installing {
                package: Package::YtDlp
            })
        );
    }

    #[test]
    fn installs_only_needed_programs_from_zip() {
        let archive = ffmpeg_zip();
        let base = serve(HashMap::from([
            ("/ffmpeg.zip.sha256", sha256_hex(&archive).into_bytes()),
            ("/ffmpeg.zip", archive),
        ]));
        let bin = temp_dir("zip");

        install(
            &test_agent(),
            Package::Ffmpeg,
            &[zip_source(&base)],
            &bin,
            &AtomicBool::new(false),
            2,
            &mut |_| {},
        )
        .unwrap();

        assert!(bin.join(Component::Ffmpeg.file_name()).is_file());
        assert!(bin.join(Component::Ffprobe.file_name()).is_file());
        // ffplay, README, dan arsipnya sendiri tidak boleh ikut terpasang.
        assert_eq!(fs::read_dir(&bin).unwrap().count(), 2);
    }

    /// Hasil pemasangan harus dikenali oleh pemeriksa komponen (`deps`).
    #[cfg(unix)]
    #[test]
    fn installed_programs_pass_component_check() {
        use crate::deps::{Status, check_in};

        let archive = ffmpeg_zip();
        let base = serve(HashMap::from([
            ("/ffmpeg.zip.sha256", sha256_hex(&archive).into_bytes()),
            ("/ffmpeg.zip", archive),
        ]));
        let bin = temp_dir("check");
        install(
            &test_agent(),
            Package::Ffmpeg,
            &[zip_source(&base)],
            &bin,
            &AtomicBool::new(false),
            3,
            &mut |_| {},
        )
        .unwrap();

        for component in [Component::Ffmpeg, Component::Ffprobe] {
            match check_in(component, std::slice::from_ref(&bin)) {
                Status::Found { version, .. } => assert_eq!(version, "9.9-test"),
                other => panic!("{component:?} seharusnya Found, ternyata {other:?}"),
            }
        }
    }

    #[test]
    fn checksum_mismatch_installs_nothing() {
        let base = serve(HashMap::from([
            ("/yt-dlp.exe", b"isi yang sudah diubah orang".to_vec()),
            (
                "/SHA2-256SUMS",
                format!("{}  yt-dlp.exe\n", "f".repeat(64)).into_bytes(),
            ),
        ]));
        let bin = temp_dir("mismatch");

        let result = install(
            &test_agent(),
            Package::YtDlp,
            &[exe_source(&base)],
            &bin,
            &AtomicBool::new(false),
            4,
            &mut |_| {},
        );

        assert!(
            matches!(result, Err(InstallError::Failed(message)) if message.contains("checksum"))
        );
        assert_eq!(fs::read_dir(&bin).unwrap().count(), 0);
    }

    #[test]
    fn missing_file_on_server_fails_with_message() {
        let base = serve(HashMap::new());
        let bin = temp_dir("404");
        let result = install(
            &test_agent(),
            Package::YtDlp,
            &[exe_source(&base)],
            &bin,
            &AtomicBool::new(false),
            5,
            &mut |_| {},
        );
        assert!(matches!(result, Err(InstallError::Failed(message)) if message.contains("404")));
        assert!(no_temp_left(&bin));
    }

    #[test]
    fn falls_back_to_second_source() {
        let archive = ffmpeg_zip();
        let broken = serve(HashMap::new());
        let working = serve(HashMap::from([
            ("/ffmpeg.zip.sha256", sha256_hex(&archive).into_bytes()),
            ("/ffmpeg.zip", archive),
        ]));
        let bin = temp_dir("fallback");

        install(
            &test_agent(),
            Package::Ffmpeg,
            &[zip_source(&broken), zip_source(&working)],
            &bin,
            &AtomicBool::new(false),
            6,
            &mut |_| {},
        )
        .unwrap();

        assert!(bin.join(Component::Ffmpeg.file_name()).is_file());
    }

    #[test]
    fn cancel_stops_and_cleans_up() {
        let body = vec![7u8; 400_000];
        let base = serve(HashMap::from([
            (
                "/SHA2-256SUMS",
                format!("{}  yt-dlp.exe\n", sha256_hex(&body)).into_bytes(),
            ),
            ("/yt-dlp.exe", body),
        ]));
        let bin = temp_dir("cancel");
        let cancel = AtomicBool::new(false);

        // Batalkan begitu unduhan mulai melaporkan progress.
        let result = install(
            &test_agent(),
            Package::YtDlp,
            &[exe_source(&base), exe_source(&base)],
            &bin,
            &cancel,
            7,
            &mut |progress| {
                if matches!(progress, Progress::Downloading { .. }) {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
        );

        assert_eq!(result, Err(InstallError::Cancelled));
        assert_eq!(fs::read_dir(&bin).unwrap().count(), 0);
    }

    #[test]
    fn zip_without_program_fails() {
        use zip::write::SimpleFileOptions;
        let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        writer
            .start_file("folder/README.txt", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"kosong").unwrap();
        let archive = writer.finish().unwrap().into_inner();
        let base = serve(HashMap::from([
            ("/ffmpeg.zip.sha256", sha256_hex(&archive).into_bytes()),
            ("/ffmpeg.zip", archive),
        ]));
        let bin = temp_dir("nozip");

        let result = install(
            &test_agent(),
            Package::Ffmpeg,
            &[zip_source(&base)],
            &bin,
            &AtomicBool::new(false),
            8,
            &mut |_| {},
        );

        assert!(
            matches!(result, Err(InstallError::Failed(message)) if message.contains("ffmpeg.exe"))
        );
        assert_eq!(fs::read_dir(&bin).unwrap().count(), 0);
    }

    #[test]
    fn manual_file_copies_main_and_sibling() {
        let dir = temp_dir("manual");
        let from = dir.join("unduhan saya");
        fs::create_dir_all(&from).unwrap();
        fs::write(from.join(Component::Ffmpeg.file_name()), "ffmpeg").unwrap();
        fs::write(from.join(Component::Ffprobe.file_name()), "ffprobe").unwrap();
        let bin = dir.join("bin");

        // Memilih ffprobe juga memasang ffmpeg dari folder yang sama.
        let package = install_from_file(&from.join(Component::Ffprobe.file_name()), &bin).unwrap();

        assert_eq!(package, Package::Ffmpeg);
        assert_eq!(
            fs::read(bin.join(Component::Ffmpeg.file_name())).unwrap(),
            b"ffmpeg"
        );
        assert_eq!(
            fs::read(bin.join(Component::Ffprobe.file_name())).unwrap(),
            b"ffprobe"
        );
    }

    #[test]
    fn manual_file_needs_main_program() {
        let dir = temp_dir("manual-tanpa-utama");
        fs::write(dir.join(Component::Ffprobe.file_name()), "ffprobe").unwrap();
        let bin = dir.join("bin");

        let result = install_from_file(&dir.join(Component::Ffprobe.file_name()), &bin);

        assert!(matches!(result, Err(InstallError::Failed(message)) if message.contains("ffmpeg")));
        assert!(!bin.exists());
    }

    #[test]
    fn manual_file_with_wrong_name_is_rejected() {
        let dir = temp_dir("wrongname");
        let wrong = dir.join("program-lain.exe");
        fs::write(&wrong, "x").unwrap();
        let bin = dir.join("bin");

        let result = install_from_file(&wrong, &bin);

        assert!(matches!(result, Err(InstallError::Failed(_))));
        assert!(!bin.exists());
    }

    #[test]
    fn known_file_names_map_to_packages() {
        let name = |component: Component| component.file_name().to_uppercase();
        assert_eq!(
            package_for_file(&name(Component::YtDlp)),
            Some(Package::YtDlp)
        );
        assert_eq!(
            package_for_file(&name(Component::Ffmpeg)),
            Some(Package::Ffmpeg)
        );
        assert_eq!(
            package_for_file(&name(Component::Ffprobe)),
            Some(Package::Ffmpeg)
        );
        assert_eq!(
            package_for_file(&name(Component::Deno)),
            Some(Package::Deno)
        );
        assert_eq!(package_for_file("notepad.exe"), None);
    }

    #[test]
    fn parses_powershell_hash_file() {
        // Format file `.sha256sum` milik Deno (keluaran Get-FileHash PowerShell).
        let text = "\r\nAlgorithm : SHA256\r\nHash      : CBA84B26E0DF5AE7F74FC66B86A95B866872A86292EBD84FA1E959927EDD93BF\r\nPath      : C:\\a\\deno\\deno-x86_64-pc-windows-msvc.zip\r\n";
        assert_eq!(
            parse_checksum(text, None),
            Some("cba84b26e0df5ae7f74fc66b86a95b866872a86292ebd84fa1e959927edd93bf".to_string())
        );
    }

    #[test]
    fn installs_deno_from_zip() {
        use zip::write::SimpleFileOptions;
        let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        writer
            .start_file("deno.exe", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"deno palsu").unwrap();
        let archive = writer.finish().unwrap().into_inner();
        let powershell_style = format!(
            "Algorithm : SHA256\r\nHash      : {}\r\n",
            sha256_hex(&archive).to_uppercase()
        );
        let base = serve(HashMap::from([
            ("/deno.zip.sha256sum", powershell_style.into_bytes()),
            ("/deno.zip", archive),
        ]));
        let source = Source {
            label: "test".into(),
            url: format!("{base}/deno.zip"),
            checksum: Checksum::HashFile {
                url: format!("{base}/deno.zip.sha256sum"),
            },
            kind: Kind::Zip,
        };
        let bin = temp_dir("deno");

        install(
            &test_agent(),
            Package::Deno,
            &[source],
            &bin,
            &AtomicBool::new(false),
            9,
            &mut |_| {},
        )
        .unwrap();

        assert_eq!(
            fs::read(bin.join(Component::Deno.file_name())).unwrap(),
            b"deno palsu"
        );
    }

    /// Menghubungi sumber resmi sungguhan dan membaca checksum-nya (file kecil, tanpa
    /// mengunduh programnya). Butuh internet, jadi hanya jalan jika diminta:
    /// `cargo test -- --ignored`
    #[test]
    #[ignore = "butuh koneksi internet"]
    fn official_checksums_are_reachable() {
        let agent = http_agent();
        for package in Package::ALL {
            for source in sources(package) {
                let hash = fetch_expected_hash(&agent, &source.checksum)
                    .unwrap_or_else(|error| panic!("{}: {error:?}", source.label));
                assert_eq!(hash.len(), 64, "{}", source.label);
            }
        }
    }

    #[test]
    fn official_sources_use_https_and_have_checksums() {
        for package in Package::ALL {
            let list = sources(package);
            assert!(!list.is_empty());
            for source in list {
                let checksum_url = match &source.checksum {
                    Checksum::HashFile { url } | Checksum::SumsFile { url, .. } => url,
                };
                // Saat DANLOAD_DEV_MIRROR dipakai, sumbernya memang bukan HTTPS.
                if std::env::var("DANLOAD_DEV_MIRROR").is_err() {
                    assert!(source.url.starts_with("https://"));
                    assert!(checksum_url.starts_with("https://"));
                }
            }
        }
    }
}
