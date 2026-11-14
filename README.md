# DANload

Aplikasi Windows untuk mengunduh video dan audio dari YouTube, Facebook, dan Instagram.
Ditulis dengan Rust + Slint; mesin unduhnya yt-dlp dan FFmpeg.

Status: **Fase 3**: unduh video MP4 dan audio MP3 dari YouTube. Format lain, Facebook/Instagram, dan Pustaka menyusul.

## Pemakaian pertama

Saat pertama kali dibuka, DANload menampilkan **wizard setup**:

1. Selamat datang.
2. **Komponen**: memasang yt-dlp, FFmpeg, dan Deno secara otomatis dari sumber resmi
   (atau memilih file `yt-dlp.exe` / `ffmpeg.exe` / `deno.exe` yang sudah kamu punya).
   Deno dibutuhkan yt-dlp untuk YouTube.
3. **Folder unduhan**.
4. Selesai.

Komponen dipasang di `%LOCALAPPDATA%\DANload\bin\`, pengaturan disimpan di `%APPDATA%\DANload\config.json`.
Wizard bisa dibuka lagi dari **Pengaturan → Pasang / perbarui...**.

## Mengunduh

1. Pilih platform di kanan atas. Bawaannya **YouTube** (Facebook dan Instagram menyusul).
2. Tempel link di panel kanan. Pesan hijau berarti link cocok dengan platform yang dipilih.
3. Pilih **Video** (MP4, dengan batas resolusi) atau **Audio** (MP3, dengan bitrate).
4. Klik **Unduh**. Kemajuannya tampil di halaman **Unduhan**; ada tombol **Batal**,
   **Tampilkan di folder**, dan **Coba lagi**.

Hasil unduhan bernama `Judul [ID].mp4` / `.mp3` dan disimpan di folder unduhan
(atau subfolder `Video\` / `Audio\` jika opsi itu dicentang di Pengaturan).
Aturan kerja dan rencana lengkap ada di [CLAUDE.md](CLAUDE.md); riwayat perubahan di [CHANGELOG.md](CHANGELOG.md).

## Kebutuhan

- Windows 10/11 64-bit
- Rust stable (toolchain `x86_64-pc-windows-msvc`), dipasang lewat [rustup](https://rustup.rs)
- Visual Studio / Build Tools dengan workload "Desktop development with C++"

## Menjalankan (mode pengembangan)

```
cargo run
```

Build pertama memakan waktu beberapa menit karena semua library dikompilasi dari awal.
Build berikutnya jauh lebih cepat.

## Menjalankan test

```
cargo test
```

Test yang menghubungi sumber resmi yt-dlp/FFmpeg (butuh internet, hanya mengambil file checksum kecil):

```
cargo test -- --ignored
```

Menguji mesin unduh dengan yt-dlp sungguhan (contoh PowerShell; sesuaikan path-nya):

```
$env:DANLOAD_TEST_YTDLP = "$env:LOCALAPPDATA\DANload\bin\yt-dlp.exe"
$env:DANLOAD_TEST_FFMPEG_DIR = "$env:LOCALAPPDATA\DANload\bin"
$env:DANLOAD_TEST_DENO = "$env:LOCALAPPDATA\DANload\bin\deno.exe"
$env:DANLOAD_TEST_URL = "https://www.youtube.com/watch?v=..."
cargo test real_yt_dlp -- --ignored --nocapture
```

## Build rilis

```
cargo build --release
```

Hasilnya ada di `target\release\danload.exe`.

## Mengukur RAM dan CPU (target: RAM < 50 MB, CPU < 5% saat idle)

1. Jalankan `target\release\danload.exe` (versi rilis, bukan `cargo run`).
2. Diamkan jendelanya sekitar 30 detik.
3. Buka Task Manager → tab **Details** → cari `danload.exe`.
4. Catat kolom **Memory (private working set)** dan **CPU**.

## Struktur singkat

| Lokasi | Isi |
|--------|-----|
| `src/main.rs` | Titik masuk aplikasi |
| `src/app.rs` | Penghubung tampilan (Slint) dengan logika |
| `src/config.rs` | Pengaturan (`%APPDATA%\DANload\config.json`) |
| `src/deps.rs` | Mencari yt-dlp, FFmpeg, ffprobe dan membaca versinya |
| `src/platform.rs` | Platform terpilih dan pengecekan kecocokan URL |
| `src/setup.rs` | Mengunduh, memverifikasi, dan memasang komponen |
| `src/formats.rs` | Pilihan format MP4/MP3, resolusi, bitrate, dan argumen yt-dlp |
| `src/downloader.rs` | Menjalankan yt-dlp, membaca kemajuan, pesan error, batal |
| `src/process.rs` | Menjalankan program tanpa console, prioritas rendah, menghentikan proses |
| `ui/app-window.slint` | Jendela utama |
| `ui/settings-page.slint` | Halaman Pengaturan |
| `ui/setup-wizard.slint` | Wizard setup awal |
| `ui/downloads.slint` | Kartu unduhan di halaman Unduhan |
| `ui/components.slint` | Komponen kecil (tombol, chip, kolom isian) |
| `ui/theme.slint` | Warna, ukuran, dan bentuk ikon |
| `build.rs` | Mengompilasi `.slint` dan menempelkan ikon ke `.exe` |
| `assets/` | Ikon aplikasi |
| `changes/` | Catatan tiap perubahan |
