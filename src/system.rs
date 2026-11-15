//! Fungsi sistem Windows yang dipanggil langsung lewat API Windows (crate `windows-sys`):
//!
//! - [`move_to_recycle_bin`]: memindahkan file ke Recycle Bin (bukan menghapus permanen).
//! - [`open_with_default_app`]: membuka file dengan aplikasi bawaan Windows (mis. pemutar video).
//! - [`local_time`]: mengubah waktu (detik sejak 1970, UTC) menjadi tanggal dan jam lokal.
//!
//! Semua kode `unsafe` milik DANload dikumpulkan di modul `win` di bawah, supaya mudah diperiksa.
//! Di Linux/macOS (hanya dipakai saat pengujian) ada pengganti sederhana tanpa `unsafe`.
//!
//! Fungsi Recycle Bin dan buka file bisa agak lama (Windows Shell): **panggil dari thread lain**,
//! bukan dari UI thread.

use std::path::Path;

/// Tanggal dan jam (tanpa detik).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: i32,
    /// 1 sampai 12.
    pub month: u32,
    /// 1 sampai 31.
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// Tanggal dan jam lokal untuk `unix_seconds` (detik sejak 1 Januari 1970, UTC), sesuai zona
/// waktu Windows (termasuk aturan musim panas di negara yang memakainya).
/// Di luar Windows dipakai UTC.
pub fn local_time(unix_seconds: i64) -> LocalTime {
    #[cfg(windows)]
    if let Some(local) = win::local_time(unix_seconds) {
        return local;
    }
    utc_time(unix_seconds)
}

/// Tanggal dan jam UTC untuk `unix_seconds`.
pub fn utc_time(unix_seconds: i64) -> LocalTime {
    let days = unix_seconds.div_euclid(86_400);
    let seconds = unix_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    LocalTime {
        year,
        month,
        day,
        hour: (seconds / 3600) as u32,
        minute: ((seconds % 3600) / 60) as u32,
    }
}

/// Tanggal (tahun, bulan, hari) dari jumlah hari sejak 1 Januari 1970.
///
/// Algoritme "civil from days" dari Howard Hinnant: kalender dihitung per "era" 400 tahun
/// (146.097 hari) yang dimulai 1 Maret, supaya hari kabisat (29 Februari) jatuh di akhir tahun.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468; // hari sejak 1 Maret tahun 0
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097); // 0..=146096
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153; // 0 = Maret, 11 = Februari
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

/// Memindahkan file `path` ke Recycle Bin. Pesan error siap ditampilkan ke pengguna.
///
/// Jika file tidak bisa masuk Recycle Bin (mis. ada di drive jaringan), Windows sendiri
/// menampilkan peringatan "akan dihapus permanen" dan pengguna bisa membatalkannya.
pub fn move_to_recycle_bin(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        win::move_to_recycle_bin(path)
    }
    #[cfg(not(windows))]
    {
        other::move_to_trash(path)
    }
}

/// Membuka `path` dengan aplikasi bawaannya (mis. pemutar video Windows).
pub fn open_with_default_app(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        win::open(path)
    }
    #[cfg(not(windows))]
    {
        other::open(path)
    }
}

// Kode khusus Windows: API Windows lewat crate `windows-sys`.
#[cfg(windows)]
mod win {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr;

    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx, CoUninitialize,
    };
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    use windows_sys::Win32::UI::Shell::{
        FO_DELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT,
        FOF_WANTNUKEWARNING, SHFILEOPSTRUCTW, SHFileOperationW, ShellExecuteW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    use super::LocalTime;

    /// Selisih detik antara 1 Januari 1601 (awal hitungan waktu Windows) dan 1 Januari 1970.
    const SECONDS_1601_TO_1970: i64 = 11_644_473_600;

    /// Teks UTF-16 yang diakhiri karakter nol, seperti yang diminta fungsi Windows.
    fn wide(text: &OsStr) -> Vec<u16> {
        text.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// Mengaktifkan COM di thread ini selama nilainya hidup. Fungsi Shell Windows bisa memakai
    /// ekstensi Shell berbasis COM, jadi Microsoft menyarankan COM diaktifkan dulu.
    struct ComGuard {
        initialized: bool,
    }

    impl ComGuard {
        fn new() -> Self {
            let flags = (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32;
            // SAFETY: tanpa pointer selain `null`; boleh dipanggil berkali-kali di satu thread.
            let result = unsafe { CoInitializeEx(ptr::null(), flags) };
            // S_OK (0) dan S_FALSE (1, sudah aktif) sama-sama harus diimbangi CoUninitialize.
            ComGuard {
                initialized: result >= 0,
            }
        }
    }

    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.initialized {
                // SAFETY: mengimbangi CoInitializeEx yang berhasil di thread yang sama.
                unsafe { CoUninitialize() };
            }
        }
    }

    pub fn move_to_recycle_bin(path: &Path) -> Result<(), String> {
        let _com = ComGuard::new();
        // Daftar file untuk SHFileOperationW diakhiri DUA karakter nol.
        let mut from = wide(path.as_os_str());
        from.push(0);
        let flags = FOF_ALLOWUNDO // ke Recycle Bin, bukan hapus permanen
            | FOF_NOCONFIRMATION // konfirmasi sudah ditanyakan DANload
            | FOF_NOERRORUI // error dilaporkan lewat nilai kembali
            | FOF_SILENT // tanpa jendela progress
            | FOF_WANTNUKEWARNING; // tetap peringatkan jika file akan terhapus permanen
        let mut operation = SHFILEOPSTRUCTW {
            hwnd: ptr::null_mut(),
            wFunc: FO_DELETE,
            pFrom: from.as_ptr(),
            pTo: ptr::null(),
            fFlags: flags as u16,
            fAnyOperationsAborted: 0,
            hNameMappings: ptr::null_mut(),
            lpszProgressTitle: ptr::null(),
        };
        // SAFETY: `operation` dan `from` hidup sampai fungsi selesai, dan `from` diakhiri dua nol.
        let result = unsafe { SHFileOperationW(&mut operation) };
        if result != 0 {
            return Err(recycle_error(result));
        }
        if operation.fAnyOperationsAborted != 0 {
            return Err("Dibatalkan.".to_string());
        }
        Ok(())
    }

    /// Pesan untuk kode error SHFileOperationW (sebagian kode Win32, sebagian kode `DE_*`).
    fn recycle_error(code: i32) -> String {
        match code {
            2 | 3 => "File tidak ditemukan.".to_string(),
            5 | 0x78 => "Akses ditolak. File mungkin hanya-baca atau sedang dipakai program lain."
                .to_string(),
            32 => "File sedang dipakai program lain. Tutup dulu, lalu coba lagi.".to_string(),
            1223 => "Dibatalkan.".to_string(),
            0x7C => "Lokasi file tidak valid.".to_string(),
            other => format!("Windows menolak memindahkan file ke Recycle Bin (kode {other})."),
        }
    }

    pub fn open(path: &Path) -> Result<(), String> {
        let _com = ComGuard::new();
        let verb = wide(OsStr::new("open"));
        let file = wide(path.as_os_str());
        // SAFETY: kedua teks diakhiri nol dan hidup sampai fungsi selesai.
        let result = unsafe {
            ShellExecuteW(
                ptr::null_mut(),
                verb.as_ptr(),
                file.as_ptr(),
                ptr::null(),
                ptr::null(),
                SW_SHOWNORMAL,
            )
        };
        // Aturan lama Windows: nilai lebih dari 32 berarti berhasil.
        match result as isize {
            code if code > 32 => Ok(()),
            2 | 3 => Err("File tidak ditemukan.".to_string()),
            5 => Err("Akses ke file ditolak.".to_string()),
            31 => Err(
                "Belum ada aplikasi di Windows untuk membuka file jenis ini. Pasang pemutar video/audio (mis. VLC), lalu coba lagi."
                    .to_string(),
            ),
            code => Err(format!("Windows tidak bisa membuka file ini (kode {code}).")),
        }
    }

    pub fn local_time(unix_seconds: i64) -> Option<LocalTime> {
        // FILETIME: jumlah satuan 100 nanodetik sejak 1 Januari 1601 (UTC).
        let ticks = unix_seconds
            .checked_add(SECONDS_1601_TO_1970)?
            .checked_mul(10_000_000)?;
        let ticks = u64::try_from(ticks).ok()?;
        let file_time = FILETIME {
            dwLowDateTime: ticks as u32,
            dwHighDateTime: (ticks >> 32) as u32,
        };
        let mut utc = SYSTEMTIME::default();
        let mut local = SYSTEMTIME::default();
        // SAFETY: semua pointer menunjuk variabel lokal yang hidup sampai fungsi selesai;
        // zona waktu `null` = zona waktu Windows yang sedang dipakai.
        let converted = unsafe {
            FileTimeToSystemTime(&file_time, &mut utc) != 0
                && SystemTimeToTzSpecificLocalTime(ptr::null(), &utc, &mut local) != 0
        };
        converted.then(|| LocalTime {
            year: i32::from(local.wYear),
            month: u32::from(local.wMonth),
            day: u32::from(local.wDay),
            hour: u32::from(local.wHour),
            minute: u32::from(local.wMinute),
        })
    }
}

/// Pengganti untuk Linux/macOS, hanya untuk menjalankan dan menguji DANload di luar Windows.
#[cfg(not(windows))]
mod other {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// Memindahkan file ke folder sampah freedesktop (`~/.local/share/Trash/files`).
    pub fn move_to_trash(path: &Path) -> Result<(), String> {
        let data = crate::config::env_path("XDG_DATA_HOME")
            .or_else(|| crate::config::env_path("HOME").map(|home| home.join(".local/share")))
            .ok_or_else(|| "Folder sampah tidak ditemukan.".to_string())?;
        move_into(path, &data.join("Trash").join("files"))
    }

    /// Memindahkan `path` ke folder `trash` (nama kembar diberi nomor).
    pub fn move_into(path: &Path, trash: &Path) -> Result<(), String> {
        fs::create_dir_all(trash).map_err(|error| error.to_string())?;
        let name = path
            .file_name()
            .ok_or_else(|| "Nama file tidak valid.".to_string())?;
        let mut target = trash.join(name);
        let mut number = 2;
        while target.exists() {
            let mut numbered = name.to_os_string();
            numbered.push(format!(".{number}"));
            target = PathBuf::from(trash).join(numbered);
            number += 1;
        }
        fs::rename(path, &target).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => "File tidak ditemukan.".to_string(),
            _ => error.to_string(),
        })
    }

    pub fn open(path: &Path) -> Result<(), String> {
        if !path.is_file() {
            return Err("File tidak ditemukan.".to_string());
        }
        Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates_are_correct() {
        let at = |year, month, day, hour, minute| LocalTime {
            year,
            month,
            day,
            hour,
            minute,
        };
        assert_eq!(utc_time(0), at(1970, 1, 1, 0, 0));
        // 15 November 2026 12:40 UTC (19:40 WIB).
        assert_eq!(utc_time(1_794_746_400), at(2026, 11, 15, 12, 40));
        // Tahun kabisat: 29 Februari 2024, dan sehari sesudahnya.
        assert_eq!(utc_time(1_709_164_800), at(2024, 2, 29, 0, 0));
        assert_eq!(utc_time(1_709_251_200), at(2024, 3, 1, 0, 0));
        // 31 Desember 1999 23:59 dan sebelum 1970.
        assert_eq!(utc_time(946_684_740), at(1999, 12, 31, 23, 59));
        assert_eq!(utc_time(-1), at(1969, 12, 31, 23, 59));
    }

    #[test]
    fn every_day_follows_the_previous_one() {
        // Cek berurutan dari 1970 sampai 2100: tanggal selalu maju satu hari.
        let mut previous = utc_time(0);
        for day in 1..47_482_i64 {
            let current = utc_time(day * 86_400);
            let next_day = previous.day + 1 == current.day && previous.month == current.month;
            let next_month = current.day == 1
                && (current.month == previous.month + 1
                    || (current.month == 1
                        && previous.month == 12
                        && current.year == previous.year + 1));
            assert!(next_day || next_month, "{previous:?} -> {current:?}");
            previous = current;
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn trash_fallback_moves_the_file() {
        let dir = std::env::temp_dir().join(format!("danload-sys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("hapus saya.mp4");
        let trash = dir.join("Trash").join("files");
        std::fs::write(&file, "isi").unwrap();
        other::move_into(&file, &trash).unwrap();
        assert!(!file.exists());
        assert!(trash.join("hapus saya.mp4").exists());
        // Nama kembar di folder sampah tidak menimpa file lama.
        std::fs::write(&file, "isi baru").unwrap();
        other::move_into(&file, &trash).unwrap();
        assert!(trash.join("hapus saya.mp4.2").exists());
        assert_eq!(
            other::move_into(&file, &trash),
            Err("File tidak ditemukan.".to_string())
        );
    }
}
