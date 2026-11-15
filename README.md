# DANload

Aplikasi Windows untuk mengunduh video dan audio dari YouTube, Facebook, dan Instagram.
Ditulis dengan Rust + Slint; mesin unduhnya yt-dlp dan FFmpeg.

Status: **Fase 7**: unduh dari YouTube, Facebook, dan Instagram dalam 6 format video (MP4, MKV, WEBM, MOV, AVI,
MPEG) dan 6 format audio (MP3, M4A, OPUS, OGG, FLAC, WAV), dengan nama file yang bisa diatur, **Pustaka**
untuk mengelola file yang sudah diunduh, **playlist YouTube**, dan **antrean** dengan beberapa unduhan
bersamaan. Installer menyusul.

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

1. Pilih platform di kanan atas: **YouTube** (bawaan), **Facebook**, atau **Instagram**.
2. Tempel link di panel kanan. Pesan hijau berarti link cocok dengan platform yang dipilih.
   Link playlist YouTube bisa dipakai (lihat [Playlist dan antrean](#playlist-dan-antrean)). Channel YouTube,
   profil atau tag Instagram, dan story Instagram belum didukung; pesannya akan menjelaskan.
3. Pilih **Video** atau **Audio**, lalu **format** dan kualitasnya:

   | Mode | Format | Kualitas |
   |------|--------|----------|
   | Video | MP4 (bawaan, paling mudah diputar), MKV, WEBM, MOV | Resolusi maksimum (Terbaik … 360p) |
   | Video | AVI, MPEG | 1080p … 360p. Selalu diubah ulang, jadi prosesnya lebih lama |
   | Audio | MP3 (bawaan), OGG | Bitrate 128–320 kbps |
   | Audio | M4A, OPUS | Asli (tanpa diubah ulang) |
   | Audio | FLAC, WAV | Lossless / tanpa kompresi |

   Di bawah pilihan format ada keterangan singkat; format yang lambat ditandai kuning (keterangannya
   menyesuaikan platform: misalnya WEBM dari Instagram biasanya harus diubah ulang).
4. Opsional: isi **NAMA FILE**. Kosong berarti nama mengikuti judul video. Ekstensinya ditambahkan otomatis.
5. Klik **Unduh**. Kemajuannya tampil di halaman **Unduhan**, termasuk persen dan sisa waktu saat
   DANload mengubah format ("Mengubah ke AVI · 45% · sisa 1:20"). Ada tombol **Batal**,
   **Tampilkan di folder**, dan **Coba lagi**. Tombol Unduh tetap bisa dipakai selama ada unduhan berjalan:
   unduhan baru masuk antrean.

Hasil unduhan bernama `Judul.ext` (atau nama yang kamu isi) dan disimpan di folder unduhan
(atau subfolder `Video\` / `Audio\` jika opsi itu dicentang di Pengaturan). Jika nama itu sudah dipakai,
file baru diberi nomor: `Judul (2).mp4`, `Judul (3).mp4`, dst. File yang sudah ada tidak pernah ditimpa.

Postingan berisi beberapa video (mis. carousel Instagram) diunduh semuanya, masing-masing satu file; foto
dilewati. Konten yang butuh login (akun privat, story) belum bisa diunduh, dan Instagram kadang membatasi
akses tanpa login; DANload menampilkan pesan yang menjelaskannya.

## Playlist dan antrean

- **Playlist YouTube:** tempel link playlist (`youtube.com/playlist?list=...`) lalu klik **Unduh**. DANload
  membaca daftar videonya, lalu menampilkan dialog **Pilih video dari playlist**: semua video yang bisa
  diunduh sudah dicentang; lepas centang yang tidak diinginkan (atau **Kosongkan** lalu pilih satu per satu),
  lalu klik **Tambah N video ke antrean**. Video privat, dihapus, atau belum tayang tampil redup dan tidak bisa
  dipilih. Tiap video disimpan langsung di folder unduhan dengan judulnya sendiri, memakai format yang dipilih
  di panel kanan.
- Link video yang dibuka dari dalam playlist (`watch?v=...&list=...`) bawaannya hanya mengunduh videonya.
  Centang **Unduh seluruh playlist** di bawah kolom URL untuk memilih video dari playlist-nya.
- **Antrean:** paling banyak 3 unduhan berjalan bersamaan (bisa diubah ke 1 atau 2 di **Pengaturan →
  Unduhan bersamaan**); sisanya menunggu di halaman **Antrean** (angka di ikonnya = jumlah yang menunggu).
  Instagram selalu satu per satu, dan konversi berat (mis. ke AVI) juga bergiliran supaya komputer tetap ringan.
- Di halaman Antrean ada **Jeda / Lanjutkan**, **Hapus** per unduhan, dan **Kosongkan**. Status bar menampilkan
  `N berjalan · M menunggu` dan **Batal semua**.
- Antrean disimpan di `%APPDATA%\DANload\queue.json`. Jika DANload ditutup saat masih mengunduh, unduhan yang
  belum selesai dilanjutkan otomatis saat DANload dibuka lagi (diulang dari awal).

## Pustaka

Setiap unduhan yang berhasil otomatis tercatat di **Pustaka** (sidebar kiri), lengkap dengan thumbnail,
platform, format, kualitas sebenarnya, ukuran, durasi, dan tanggal unduh. Datanya disimpan di
`%APPDATA%\DANload\library.json` dan thumbnail kecilnya di `%LOCALAPPDATA%\DANload\thumbs\`, jadi tetap ada
walau folder unduhan diganti. Yang tercatat hanya unduhan sejak versi ini.

- **Cari** lewat kolom di bar atas (nama file dan judul asli). **Filter** dengan chip jenis dan platform (bisa
  digabung, mis. Video + YouTube), dan **urutkan** lewat menu di samping judul.
- Klik satu file untuk melihat detailnya di panel kanan:
  - **Buka** (aplikasi bawaan Windows) dan **Tampilkan di folder**.
  - **Ganti nama**: ekstensi tetap, nama yang sudah dipakai ditolak.
  - **Salin link sumber**.
  - **Unduh ulang**: link dan format terisi di form; formatnya boleh diganti dulu.
  - **Hapus...**
- **Hapus** selalu ditanya dulu: **Pindahkan ke Recycle Bin** (bisa dikembalikan) atau **Hapus dari daftar
  saja** (file tetap ada di folder). Centang beberapa file untuk menghapus sekaligus.
- File yang dipindah atau dihapus di luar DANload ditandai **File tidak ditemukan**.

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

Test yang menghubungi sumber resmi yt-dlp/FFmpeg/Deno (butuh internet, hanya mengambil file checksum kecil):

```
cargo test official_checksums -- --ignored
```

Menguji dengan yt-dlp dan FFmpeg sungguhan (contoh PowerShell; sesuaikan path-nya):

```
$env:DANLOAD_TEST_YTDLP = "$env:LOCALAPPDATA\DANload\bin\yt-dlp.exe"
$env:DANLOAD_TEST_FFMPEG_DIR = "$env:LOCALAPPDATA\DANload\bin"
$env:DANLOAD_TEST_DENO = "$env:LOCALAPPDATA\DANload\bin\deno.exe"
$env:DANLOAD_TEST_URL = "https://www.youtube.com/watch?v=..."
$env:DANLOAD_TEST_PLAYLIST_URL = "https://www.youtube.com/playlist?list=..."
cargo test real_ -- --ignored --nocapture --test-threads 1
```

Isinya empat test:

- `real_yt_dlp_selects_expected_formats`: memeriksa format yang dipilih yt-dlp untuk tiap pilihan DANload,
  memakai data video tiruan (tanpa internet; cukup `DANLOAD_TEST_YTDLP` dan `DANLOAD_TEST_FFMPEG_DIR`).
- `real_yt_dlp_end_to_end`: mengunduh URL uji dalam ke-12 format ke folder sementara sistem, lalu memeriksa
  kodek tiap hasil, data untuk Pustaka (kualitas, durasi, thumbnail), unduh ulang (harus menjadi `(2)`), dan
  nama kustom. Bawaannya 360p; ubah dengan `$env:DANLOAD_TEST_HEIGHT = "1080"`. Pakai video pendek. URL-nya
  boleh link Facebook atau Instagram.
- `real_cancel_during_conversion`: membatalkan konversi MPEG di tengah jalan dan memastikan tidak ada sisa file.
- `real_playlist_read_and_cancel`: membaca isi playlist `DANLOAD_TEST_PLAYLIST_URL` (daftar video, judul, durasi,
  video yang tidak bisa diunduh), lalu membatalkan pembacaan di tengah jalan. Cukup `DANLOAD_TEST_YTDLP` dan
  `DANLOAD_TEST_DENO`; pakai playlist yang pendek.

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
| `src/deps.rs` | Mencari yt-dlp, FFmpeg, ffprobe, Deno dan membaca versinya |
| `src/platform.rs` | Platform terpilih, pengecekan kecocokan URL dan jenis link |
| `src/setup.rs` | Mengunduh, memverifikasi, dan memasang komponen |
| `src/formats.rs` | Pilihan format video/audio, menu kualitas, keterangan format, dan argumen yt-dlp |
| `src/downloader.rs` | Menjalankan yt-dlp, membaca kemajuan, pesan error, batal, beberapa video per link, giliran konversi |
| `src/queue.rs` | Antrean: giliran (maks. 3 bersamaan, Instagram 1), jeda, `queue.json` |
| `src/playlist.rs` | Membaca isi playlist YouTube (`--flat-playlist`) untuk dipilih videonya |
| `src/filename.rs` | Pemeriksaan nama file kustom dan nama unik `Judul (2).mp4` |
| `src/convert.rs` | Langkah konversi DANload dengan FFmpeg (WEBM, MOV, AVI, MPEG) beserta persennya |
| `src/library.rs` | Pustaka: `library.json`, cari/filter/urut, ganti nama, file hilang, thumbnail |
| `src/system.rs` | Fungsi Windows: Recycle Bin, buka file dengan aplikasi bawaan, jam lokal (satu-satunya kode `unsafe`) |
| `src/process.rs` | Menjalankan program tanpa console, prioritas rendah, menghentikan proses |
| `ui/app-window.slint` | Jendela utama |
| `ui/settings-page.slint` | Halaman Pengaturan |
| `ui/setup-wizard.slint` | Wizard setup awal |
| `ui/downloads.slint` | Kartu unduhan di halaman Unduhan |
| `ui/queue.slint` | Halaman Antrean, dialog pilih video dari playlist, dialog konfirmasi |
| `ui/library.slint` | Halaman Pustaka, panel detail file, dan dialog hapus |
| `ui/components.slint` | Komponen kecil (tombol, chip, kolom isian) |
| `ui/theme.slint` | Warna, ukuran, dan bentuk ikon |
| `build.rs` | Mengompilasi `.slint` dan menempelkan ikon ke `.exe` |
| `assets/` | Ikon aplikasi |
| `changes/` | Catatan tiap perubahan |
