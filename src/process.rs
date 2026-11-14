//! Bantuan menjalankan program eksternal (yt-dlp, FFmpeg, Deno) di Windows.
//!
//! - [`hide_console_window`]: untuk perintah singkat (mis. membaca versi).
//! - [`run_in_background`]: untuk pekerjaan berat (unduh dan konversi).
//! - [`kill_tree`]: menghentikan sebuah proses beserta semua proses anaknya.

use std::process::{Command, Stdio};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
#[cfg(windows)]
const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Di Windows, mencegah jendela console hitam berkedip saat program dijalankan.
#[cfg(windows)]
pub fn hide_console_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
pub fn hide_console_window(_command: &mut Command) {}

/// Untuk pekerjaan berat: tanpa jendela console dan berprioritas rendah, supaya laptop
/// tetap lancar dipakai. Di Windows, proses anak (mis. FFmpeg yang dijalankan yt-dlp)
/// otomatis ikut berprioritas rendah.
#[cfg(windows)]
pub fn run_in_background(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
}

/// Di Linux/macOS (dipakai saat pengujian): jadikan proses ketua grup baru,
/// supaya [`kill_tree`] bisa menghentikan seluruh grupnya sekaligus.
#[cfg(unix)]
pub fn run_in_background(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

/// Menghentikan proses `pid` beserta semua proses anaknya.
///
/// yt-dlp.exe menjalankan dirinya sendiri sebagai proses anak, lalu FFmpeg sebagai cucu,
/// jadi menghentikan proses induknya saja tidak cukup.
#[cfg(windows)]
pub fn kill_tree(pid: u32) {
    let mut command = Command::new("taskkill");
    command
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hide_console_window(&mut command);
    let _ = command.status();
}

#[cfg(unix)]
pub fn kill_tree(pid: u32) {
    // Nilai negatif berarti "seluruh grup proses" (lihat `run_in_background`).
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Membuka File Explorer dengan file `path` terpilih. Jika file sudah tidak ada,
/// folder tempatnya yang dibuka (jika masih ada).
#[cfg(windows)]
pub fn show_in_file_manager(path: &std::path::Path) {
    use std::os::windows::process::CommandExt;
    let mut command = Command::new("explorer");
    if path.is_file() {
        // Path Windows tidak mungkin berisi tanda kutip, jadi aman dibungkus kutip.
        command.raw_arg(format!("/select,\"{}\"", path.display()));
    } else if let Some(folder) = existing_folder(path) {
        command.arg(folder);
    } else {
        return;
    }
    let _ = command.spawn();
}

#[cfg(not(windows))]
pub fn show_in_file_manager(path: &std::path::Path) {
    if let Some(folder) = existing_folder(path) {
        let _ = Command::new("xdg-open").arg(folder).spawn();
    }
}

/// Folder terdekat yang masih ada: `path` sendiri jika berupa folder, atau folder induknya.
fn existing_folder(path: &std::path::Path) -> Option<&std::path::Path> {
    path.ancestors().find(|candidate| candidate.is_dir())
}
