# Fase 4: Format Video dan Audio Lain (MKV, WEBM, MOV, AVI, MPEG, M4A, OPUS, OGG, FLAC, WAV)

- **Tanggal:** 2026-11-15
- **Jenis:** Fitur baru
- **Status:** Sebagian — kode selesai dan diuji di Linux dengan yt-dlp + FFmpeg asli; menunggu uji di Windows

## Latar Belakang
- Fase 4 roadmap: semua format di Bagian 2.4 CLAUDE.md, dengan peringatan untuk format yang lambat.
- Fase 3 sudah diuji pemilik projek di Windows pada 2026-11-15 dan dilaporkan berjalan semua.
- Dua keputusan dari pemilik projek (2026-11-15):
  1. **Opsi A** — encode ulang dikerjakan oleh **langkah FFmpeg milik DANload**, bukan `--recode-video` yt-dlp. Alasannya: ada persen + sisa waktu, bisa dibatalkan, dan bisa membuat MPEG. `--recode-video` yt-dlp tidak mendukung `.mpg`, dan untuk AVI hanya memakai bitrate bawaan FFmpeg (200 kb/s).
  2. **Opsi A** — **MP4 mengutamakan H.264 di resolusi yang sama**. Sebelumnya MP4 sering berisi AV1, karena urutan kodek bawaan yt-dlp adalah AV1 > VP9 > H.264.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| src/convert.rs | Dibuat | Langkah konversi DANload: baca kodek dengan ffprobe; tiap stream disalin atau diubah ulang; FFmpeg `-progress pipe:1` untuk persen dan sisa waktu; pesan error berbahasa Indonesia; 8 unit test |
| src/formats.rs | Diubah | `VideoFormat` (6) dan `AudioFormat` (6), keterangan tiap format, `OutputFormat`, argumen yt-dlp per format, menu kualitas yang menyesuaikan format (`QualityMenu`/`QualityChoice`); 9 unit test + 1 uji pemilihan format dengan yt-dlp asli (`#[ignore]`) |
| src/downloader.rs | Diubah | Format konversi melewati folder `hasil\`; cek "file sudah ada" sebelum mengunduh; laporan `Converting`; `Tools` berisi path ffmpeg dan ffprobe; pemindahan file dicoba ulang jika dikunci antivirus. Unit test baru: konversi, gagal konversi, batal saat konversi, file sudah ada. Uji asli: semua format + unduh ulang (`#[ignore]`), dan batal saat konversi (`#[ignore]`) |
| src/config.rs | Diubah | Kolom `video_format` dan `audio_format`; nilai pilihan yang tidak dikenal tidak lagi mereset seluruh config; `output_format()`; 2 unit test baru |
| src/app.rs | Diubah | Dropdown format, menu kualitas mengikuti format, keterangan/peringatan format, kartu "Mengubah ke …", pesan "Sudah ada di folder"; 1 unit test |
| src/process.rs | Diubah | `spawn_line_reader` (dipakai bersama oleh yt-dlp dan FFmpeg) |
| src/deps.rs | Diubah | `run_capture` dipakai juga untuk ffprobe; stdout dibaca di thread lain supaya keluaran panjang tidak membuat program macet |
| src/main.rs | Diubah | Modul `convert` |
| ui/app-window.slint | Diubah | FORMAT menjadi dropdown; judul kolom kualitas mengikuti format (RESOLUSI MAKS./BITRATE/KUALITAS); keterangan di bawahnya (kuning untuk AVI/MPEG) |
| ui/downloads.slint | Diubah | Keadaan `converting` (progress bar mengikuti persen konversi) |
| README.md | Diubah | Status Fase 4, cara memilih format, cara uji |
| CLAUDE.md | Diubah | 2.3, 2.4 (cara tiap format dibuat), 2.5, 2.10, 4 (cara uji), 7 (struktur), roadmap (Fase 3 ✅, Fase 4 ⏳) |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan

**Cara tiap format dibuat**
| Format | Cara | Waktu proses |
|--------|------|--------------|
| MP4 | yt-dlp. Urutan `-S res,fps,source,+vcodec:avc,+hdr,acodec:aac`: resolusi dulu, lalu di resolusi yang sama H.264 > H.265 > VP9 > AV1, gambar SDR, audio AAC | Cepat |
| MKV | yt-dlp, urutan bawaan (kualitas terbaik: AV1/VP9, termasuk HDR), tanpa encode ulang | Cepat |
| WEBM | yt-dlp `--merge-output-format webm/mkv`: VP9 (atau AV1) + Opus langsung digabung ke WEBM. Jika kodeknya tidak cocok, digabung ke MKV lalu DANload mengubahnya ke VP9 + Opus | Biasanya cepat |
| MOV | yt-dlp memilih H.264 + AAC (walau resolusinya lebih rendah), menggabung ke MP4, lalu DANload menyalinnya ke MOV. Tanpa H.264, DANload mengubah ke H.264 + AAC | Biasanya cepat |
| AVI | DANload mengubah ke Xvid (encoder MPEG-4 bawaan FFmpeg, `-q:v 3`, tag `XVID`) + MP3 192 kbps. Paling tinggi 1080p | Lambat |
| MPEG | DANload mengubah ke MPEG-2 (`-q:v 3`, B-frame 2, buffer VBV 9781k) + MP2 224 kbps, MPEG-2 Program Stream (`-f vob`). Paling tinggi 1080p | Lambat |
| MP3 / OGG | yt-dlp `-x`, bitrate dipilih (128–320 kbps) | Sedang |
| M4A / OPUS | yt-dlp `-x`; AAC/Opus asli disalin tanpa encode ulang ("Asli") | Cepat |
| FLAC | yt-dlp `-x`, dipaksa **16-bit** | Sedang |
| WAV | yt-dlp `-x`, PCM 16-bit | Sedang |

**Langkah konversi DANload (`src/convert.rs`)**
- yt-dlp menaruh hasilnya di `.danload-tmp\<job>\hasil\`. Jika ekstensinya sudah sama dengan format tujuan (mis. WEBM), file langsung dipindah.
- Jika belum, ffprobe membaca kodek, dan tiap stream diputuskan: **disalin** jika sudah cocok, atau **diubah ulang**. Contoh: H.264 + AAC ke MOV cukup disalin, sehingga kartu menampilkan "Menyimpan sebagai MOV · 45%".
- FFmpeg dijalankan tanpa console, berprioritas rendah, dengan `-progress pipe:1`. Kartu menampilkan "Mengubah ke AVI · 45% · sisa 1:20". Tombol Batal menghentikan FFmpeg. Tidak ada file setengah jadi di folder unduhan.
- Encode ulang WEBM memakai `-deadline realtime -cpu-used 6`. Pada uji 1080p hasilnya 5,6× lebih cepat daripada `good`, dengan kualitas hampir sama (SSIM 0,982 vs 0,983); ukuran file ±20% lebih besar.

**Pilihan di panel kanan**
- FORMAT kini dropdown. Format terakhir per mode diingat di `config.json`. Config lama (tanpa kolom ini) memakai MP4 dan MP3.
- Kolom kualitas mengikuti format:
  - resolusi untuk video;
  - AVI/MPEG hanya 1080p–360p ("Terbaik" dan 2160p tampil sebagai 1080p);
  - bitrate untuk MP3/OGG;
  - nonaktif dengan teks "Asli" (M4A/OPUS), "Lossless" (FLAC), atau "Tanpa kompresi" (WAV).
- Setiap format punya keterangan singkat. AVI/MPEG berwarna kuning: "Selalu diubah ulang … proses lebih lama dan file lebih besar. Paling tinggi 1080p."

**File yang sudah ada**
- Sebelum mengunduh, yt-dlp mencetak nama file rencananya (`--print before_dl:…%(filename)s`). Jika file akhir dengan nama itu sudah ada di folder unduhan, yt-dlp dihentikan dan kartu menampilkan "Sudah ada di folder, tidak diunduh ulang · nama file".
- Ini berlaku juga untuk format konversi (AVI, MPEG, dll.), sehingga tidak ada unduhan dan encode yang sia-sia.

**Ditemukan dan diperbaiki saat pengujian**
- Aturan MP4 di draf awal (`bestvideo[ext=mp4]+bestaudio[ext=m4a]/…`) melewatkan video 4K yang hanya tersedia sebagai VP9; hasilnya turun ke 1080p. Aturan itu juga bisa memasukkan audio Opus ke MP4. Diganti dengan urutan `-S` di atas.
- `--merge-output-format mov/mkv` ternyata selalu menghasilkan MKV di yt-dlp 2026.10.26 (dicek di kode sumbernya, `get_compatible_ext`). Untuk MOV dipakai `mp4/mkv`, lalu DANload menyalinnya ke MOV.
- FLAC dari audio Opus/AAC menjadi 24-bit (564 KB vs 150 KB untuk 10 detik), padahal sumbernya lossy. Kini dipaksa 16-bit lewat `--postprocessor-args "ExtractAudio+ffmpeg_o:-sample_fmt s16"`.
- Encode VP9 dengan `-deadline good` terlalu lambat (31 detik untuk klip 10 detik di 2 core). Diganti `realtime`.

## Dependensi
Tidak ada crate baru (`Cargo.toml` dan `Cargo.lock` tidak berubah).

## Cara Menguji
1. `cargo test`: 89 test lolos di Linux, 4 diabaikan (butuh program asli). Di Windows jumlahnya lebih sedikit, karena beberapa test memakai skrip shell khusus Linux.
2. `cargo run`, lalu cek panel kanan:
   - FORMAT berisi MP4/MKV/WEBM/MOV/AVI/MPEG.
   - Pilih AVI: resolusi menjadi 1080p–360p, keterangan kuning muncul.
   - Mode Audio: MP3/M4A/OPUS/OGG/FLAC/WAV. FLAC menampilkan "Lossless" yang tidak bisa diubah.
3. Unduh satu video pendek dalam MP4 1080p, lalu cek kodeknya di Properties → Details (atau MediaInfo). Seharusnya H.264 + AAC.
4. Unduh video yang sama dalam WEBM, MOV, MKV. Lalu AVI dan MPEG: kartu menampilkan "Mengubah ke AVI · …%" dengan sisa waktu. Coba putar hasilnya.
5. Mode Audio: M4A, OPUS, OGG, FLAC, WAV.
6. Unduh lagi video + format yang sama: kartu langsung menampilkan "Sudah ada di folder, tidak diunduh ulang".
7. Mulai AVI dari video yang agak panjang, tekan **Batal** saat "Mengubah ke AVI". Pastikan `ffmpeg.exe` hilang dari Task Manager dan tidak ada file `.avi` atau folder `.danload-tmp` yang tertinggal.
8. Opsional, uji otomatis dengan program asli: lihat README bagian test.
   - `real_yt_dlp_selects_expected_formats` tidak butuh internet.
   - `real_yt_dlp_end_to_end` mengunduh ke-12 format dari URL uji.

Sudah diuji (Linux): `cargo fmt`, `clippy --all-targets` tanpa warning, 89 unit test (diulang 3 kali, stabil). Uji dengan **yt-dlp 2026.10.26 + FFmpeg 6.1 asli**:
- **Pemilihan format:** 20 kombinasi format × jenis video tiruan ala YouTube, semua sesuai harapan. Jenisnya: lengkap 4K, HDR, Shorts vertikal, tanpa H.264, H.264 saja.
- **Unduh menyeluruh:** server lokal dengan 3 jenis sumber (lengkap, H.264 saja, VP9 + Opus saja) × 12 format = **36 unduhan, semua berhasil**. Tiap hasil diperiksa: kodek dengan ffprobe, seluruh isi dibaca FFmpeg tanpa error, durasi benar, FLAC 16-bit. Judul non-ASCII (`é 日本 |`) aman.
- **Unduh ulang:** AVI dan MP3 terdeteksi "sudah ada" dalam 0,9 detik.
- **Batal saat konversi MPEG:** FFmpeg berhenti 6 ms setelah Batal, folder bersih.
- **Tampilan (tangkapan layar):** dropdown format, menu kualitas AVI, FLAC "Lossless", kartu "Mengubah ke WEBM/MPEG · …% · sisa …", "Sudah ada di folder".

Belum diuji: unduhan YouTube sungguhan (YouTube tidak bisa diakses dari workspace), FFmpeg build Windows (gyan.dev), dan kecepatan konversi di laptop pemilik projek.

## Hasil Ukur Performa
- Indikasi Linux (build release Fase 4):
  - RAM 23,2 MB saat diam setelah dibuka, 25,1 MB saat konversi MPEG, 26,4 MB saat diam setelah selesai;
  - CPU 0% saat diam, ±1% (proses DANload saja) saat konversi.
  - Sama dengan Fase 3 di Linux (±22–23 MB).
- Windows (Fase 3 dan 4): **belum diukur**. Fase 2B di Windows: 9,8 MB, 0%.

## Catatan / Yang Perlu Dicek Ulang
- **MP4 "Terbaik"** untuk video 4K berisi VP9 (atau AV1) di dalam MP4, sesuai keputusan "H.264 di resolusi yang sama": YouTube umumnya hanya menyediakan H.264 sampai 1080p. Pilih 1080p jika butuh H.264.
- **MOV** hampir selalu paling tinggi 1080p di YouTube (H.264 diutamakan supaya cukup disalin). Jika memilih 2160p, hasilnya tetap 1080p, tetapi label kartu menulis "MOV · 2160p".
- **"Sudah ada"** dicek dari nama file (`Judul [ID].ext`). Contoh: MP4 720p yang sudah ada tidak diganti saat memilih 1080p. Untuk mengunduh ulang, hapus atau ganti nama file lama (nanti bisa dari Pustaka, Fase 6).
- Ukuran AVI/MPEG besar: klip uji 1080p menghasilkan ±15–17 Mbps. Ini sesuai peringatan di UI.
- Encode ulang (AVI, MPEG, dan cadangan WEBM/MOV) memakai CPU penuh selama konversi, walau berprioritas rendah. Video panjang bisa butuh waktu lama.
- Format video belum menyimpan info judul (metadata) di file, sama seperti MP4 di Fase 3. Format audio sudah.
