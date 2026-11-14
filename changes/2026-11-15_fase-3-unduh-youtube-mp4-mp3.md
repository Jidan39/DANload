# Fase 3: Unduh YouTube (MP4/MP3), Deno, dan Tombol Batal

- **Tanggal:** 2026-11-15
- **Jenis:** Fitur baru
- **Status:** Sebagian — kode selesai dan diuji di Linux dengan yt-dlp asli; menunggu uji unduhan YouTube sungguhan di Windows

## Latar Belakang
- Fase 3 roadmap: unduh video MP4 dan audio MP3 dari YouTube dengan progress, setara aplikasi lama.
- Sejak yt-dlp 2025.11.12, YouTube butuh runtime JavaScript. Pemilik projek memilih **Deno** (2026-11-14) sebagai komponen keempat yang dipasang wizard.
- Tombol **Batal** dimajukan dari Fase 7 (disetujui pemilik projek), supaya yt-dlp/FFmpeg tidak tertinggal berjalan.
- Hasil ukur Fase 2B di Windows dicatat: RAM 9,8 MB, CPU 0%.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| src/formats.rs | Dibuat | Pilihan MP4 (resolusi Terbaik…360p) dan MP3 (128–320 kbps), label, argumen yt-dlp + 4 unit test |
| src/downloader.rs | Dibuat | Menjalankan yt-dlp, membaca progress dari stdout+stderr, pesan error berbahasa Indonesia, batal, folder sementara + 14 unit test (termasuk 1 uji yt-dlp asli, `#[ignore]`) |
| src/process.rs | Dibuat | Tanpa jendela console, prioritas rendah, hentikan seluruh pohon proses, buka Explorer dengan file terpilih |
| src/deps.rs | Diubah | Komponen Deno; cek versi minimum (yt-dlp ≥ 2025.11.12, Deno ≥ 2.3.0); helper console dipindah ke `process.rs` + 4 unit test |
| src/setup.rs | Diubah | Paket Deno (GitHub resmi); checksum format PowerShell; "Pilih file sendiri" menentukan paket dari nama file + 5 unit test |
| src/platform.rs | Diubah | `download_supported()` (Fase 3: baru YouTube) dan `feedback()` untuk UI + 1 unit test |
| src/config.rs | Diubah | Kolom `video_max_height` dan `audio_bitrate_kbps` (bawaan 192), `target_folder()` untuk subfolder Video/Audio + 2 unit test |
| src/app.rs | Diubah | Tombol Unduh, kartu unduhan, Batal, Coba lagi, Tampilkan di folder, simpan pilihan kualitas, konfirmasi saat menutup jendela, lokasi komponen untuk unduhan |
| src/main.rs | Diubah | Modul baru; hentikan unduhan yang tersisa setelah jendela tertutup |
| ui/downloads.slint | Dibuat | Kartu unduhan (ikon status, judul, format, progress, keterangan, tombol aksi) |
| ui/app-window.slint | Diubah | Daftar unduhan di halaman Unduhan, dropdown resolusi/bitrate, syarat tombol Unduh + keterangannya, status bar |
| ui/components.slint | Diubah | Komponen baru `Dropdown` dan `ProgressBar` (dipindah dari wizard) |
| ui/setup-wizard.slint | Diubah | Teks menyebut Deno, satu tombol "Pilih file sendiri...", baris komponen lebih ringkas, tinggi wizard sedikit bertambah |
| ui/theme.slint | Diubah | Ikon centang, silang, dan panah dropdown |
| README.md | Diubah | Cara mengunduh, uji yt-dlp asli, struktur |
| CLAUDE.md | Diubah | 2.3 (centang fitur yang sudah jalan), 2.5 (Deno, versi minimum), 2.6 (riwayat hasil ukur), 2.9 (Deno), 2.10 baru (perilaku unduhan), 4 (cara uji), struktur folder, roadmap |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan
- **Tombol Unduh** aktif hanya jika: keempat komponen siap, link cocok dengan platform terpilih, platformnya YouTube (Facebook/Instagram menampilkan keterangan "menyusul"), dan tidak ada unduhan lain berjalan. Di bawah tombol ada keterangan kenapa belum bisa mengunduh.
- **Resolusi/bitrate** dipilih lewat dropdown dan diingat di config.
- **Kartu unduhan** (terbaru di atas): judul, "YouTube · MP4 · 720p", progress dengan persen, kecepatan, dan sisa waktu; tahap FFmpeg ("Menggabungkan video dan audio...", "Mengubah ke MP3..."); hasil ("Selesai · nama file") atau pesan error.
- **Batal**: yt-dlp dan semua proses anaknya dihentikan, file `.part` di `.danload-tmp` dihapus. **Coba lagi** mengulang dengan pengaturan yang sama.
- **Menutup jendela saat mengunduh**: muncul konfirmasi; jika "Yes", unduhan dihentikan dulu (paling lama 5 detik).
- **Deno** muncul di Pengaturan dan wizard. Karena setup sudah pernah selesai, setelah update wizard terbuka otomatis di langkah Komponen dan "Pasang otomatis" hanya memasang Deno.
- Komponen yang versinya terlalu lama ditandai "Bermasalah" dengan petunjuk memperbarui.
- **Ditemukan saat uji dengan yt-dlp asli:** filter resolusi aplikasi lama (`[height<=720]`) menolak format yang tingginya tidak diketahui ("Requested format is not available"). Diganti `[height<=?720]`.
- Nama file hasil: `Judul [ID].ext` (bawaan yt-dlp) supaya video berjudul sama tidak tertukar. Video yang sudah pernah diunduh ke folder yang sama langsung dinyatakan selesai tanpa mengunduh ulang.

## Dependensi
Tidak ada crate baru.

## Cara Menguji
1. `cargo test` → 66 test lolos di Linux; di Windows lebih sedikit karena beberapa test memakai skrip shell khusus Linux.
2. `cargo run` → wizard terbuka di langkah Komponen (Deno belum ada) → **Pasang otomatis** → Deno terpasang.
3. Tempel link YouTube → pesan hijau → pilih resolusi → **Unduh** → lihat progress → **Tampilkan di folder**.
4. Mode **Audio** → pilih bitrate → **Unduh** → hasil MP3.
5. Unduh video yang agak panjang → **Batal** di tengah → pastikan tidak ada folder `.danload-tmp` dan proses `yt-dlp.exe`/`ffmpeg.exe` di Task Manager → **Coba lagi**.
6. Mulai unduhan lalu tutup jendela → konfirmasi muncul.
7. Pilih Facebook dan tempel link Facebook → tombol Unduh tetap nonaktif dengan keterangan.
8. Opsional: uji mesin unduh langsung dengan yt-dlp asli, lihat README bagian test.

Sudah diuji (Linux): fmt, `clippy -D warnings`, 66 unit test (diulang beberapa kali, stabil). Mesin unduh dengan **yt-dlp 2026.10.26 asli** terhadap server lokal: MP3 (bitrate 128 kbps terverifikasi dengan ffprobe, judul tersimpan di metadata), MP4, judul non-ASCII (`vidéo ü 日本`) dengan lokal sistem `C`, error 404, batal (proses anak ikut berhenti). Seluruh alur di tampilan lewat tangkapan layar: wizard otomatis memasang Deno, unduh MP4 720p dan MP3, Batal → Coba lagi, Facebook ditolak dengan keterangan, subfolder Video/Audio. Opsi `-P home:D:\...` dan `--js-runtimes deno:C:\...` dicek di kode sumber yt-dlp: aman untuk path Windows.

Belum diuji: unduhan YouTube sungguhan (YouTube tidak bisa diakses dari workspace), semua hal khusus Windows (`taskkill`, prioritas `BELOW_NORMAL`, Explorer `/select`, dialog konfirmasi saat menutup), dan Deno sungguhan memproses YouTube.

## Hasil Ukur Performa
- Windows (Fase 2B, dilaporkan pemilik projek): RAM 9,8 MB, CPU 0%.
- Indikasi Linux (build release Fase 3): RAM ±22 MB saat diam, ±23 MB saat dan setelah mengunduh; CPU 0–0,3% saat diam. Ukuran program ±16,6 MB.
- Fase 3 di Windows: **belum diukur**.

## Catatan / Yang Perlu Dicek Ulang
- Dialog konfirmasi saat menutup hanya bisa diuji di Windows.
- Video MP4 "Terbaik" bisa berkodek AV1/VP9 di dalam MP4 (sama seperti aplikasi lama); pemutar lama di Windows mungkin butuh ekstensi AV1. Akan ditinjau saat format lain dibuat (Fase 4).
- Jika aplikasi crash di tengah unduhan, folder `.danload-tmp` bisa tertinggal di folder unduhan.
- Filter chip (Semua/Video/Audio/...) di halaman Unduhan belum berfungsi; menyusul bersama Pustaka (Fase 6).
