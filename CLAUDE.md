# CLAUDE.md — DANload

File ini berisi aturan kerja dan konteks projek. **Baca seluruh file ini sebelum melakukan apa pun di projek ini.**
Jika ada aturan di sini yang bertentangan dengan instruksi langsung dari pemilik projek (Jidan) di percakapan, ikuti instruksi terbaru, lalu usulkan untuk memperbarui file ini.

---

## 1. Aturan Utama: Konfirmasi Dulu, Baru Kerjakan

1. **Jangan langsung mengubah kode.** Untuk setiap permintaan, jelaskan dulu secara singkat:
   - apa yang dipahami dari permintaan,
   - rencana langkah yang akan dilakukan,
   - file yang akan **dibuat / diubah / dihapus**,
   - dependensi (crate) baru yang akan ditambahkan, jika ada.
2. **Tunggu persetujuan** ("oke", "lanjut", "gas", dsb.) sebelum mengeksekusi.
3. Pengecualian: perubahan sangat kecil yang sudah diminta secara eksplisit (mis. "ganti teks tombol jadi X") boleh langsung dikerjakan, tetapi tetap dilaporkan dan tetap dibuatkan catatan perubahan (lihat Bagian 3).
4. **Wajib minta izin terpisah** sebelum:
   - menghapus, memindahkan, atau menimpa file;
   - menambah atau menghapus dependensi di `Cargo.toml`;
   - mengubah struktur folder;
   - menjalankan `git commit`, `git push`, atau membuat release;
   - mengunduh/menyertakan binary pihak ketiga (yt-dlp, FFmpeg) ke dalam repo.
5. Jika permintaan ambigu dan salah tebak akan membuang waktu, **tanya dulu** (cukup pertanyaan yang paling penting). Jika risikonya kecil, pakai asumsi yang masuk akal dan sebutkan asumsinya.
6. Jika ada keputusan teknis yang berpengaruh besar (pilihan library GUI, arsitektur, cara distribusi), berikan **2–3 opsi** beserta kelebihan, kekurangan, dan risikonya, lalu **satu rekomendasi**. Jangan sekadar mengikuti pilihan saya jika secara teknis kurang tepat — jelaskan alasannya.

---

## 2. Tentang Projek

### 2.1 Ringkasan
**Nama aplikasi: DANload**

Aplikasi **desktop Windows** untuk mengunduh **video dan audio** dari **YouTube, Facebook, dan Instagram**, dengan pilihan berbagai format output. Ditulis dalam **Rust**.

Projek ini adalah penulisan ulang (rewrite) dan pengembangan dari aplikasi lama:
`D:\Code Project\Youtube Downloader` (Python + Tkinter + yt-dlp).
Folder lama itu **hanya sebagai referensi** — **jangan diubah**.

### 2.2 Tujuan
- Aplikasi GUI yang ringan, cepat dibuka, dan mudah dipakai orang awam.
- Dibagikan sebagai **installer Inno Setup** (`DANload-Setup.exe`) yang mudah dipakai orang awam.
- Mendukung lebih banyak situs dan format dibanding aplikasi lama.
- Kode rapi, mudah dibaca, dan jadi bahan belajar Rust.

### 2.3 Fitur yang Diharapkan

**Wajib (MVP) — minimal setara aplikasi lama:**
- [x] Input URL + tombol **Tempel** dari clipboard
- [x] **Pilih platform** (YouTube / Facebook / Instagram) di bar atas **sebelum mengunduh**; bawaan **YouTube** setiap aplikasi dibuka. URL harus berasal dari platform yang dipilih; kalau tidak cocok, muncul peringatan dan tombol Unduh tidak aktif *(permintaan pemilik projek, 2026-11-14)*
- [x] Pilih mode: **Video** atau **Audio**
- [x] Pilih format output (lihat 2.4) — *sejak Fase 4 (6 format video, 6 format audio); diuji di Windows 2026-11-16*
- [x] **Nama file bisa diatur** sebelum mengunduh (kosong = judul video). Nama tanpa ID; jika sudah dipakai, menjadi `nama (2).mp4` *(permintaan pemilik projek, 2026-11-15)* — *sejak Fase 5; diuji di Windows 2026-11-16*
- [x] Unduh dari **Facebook** dan **Instagram**, termasuk postingan berisi beberapa video — *sejak Fase 5; diuji di Windows 2026-11-16*
- [x] Pilih resolusi maksimum (video) / bitrate (audio) — *sejak Fase 3; sejak Fase 4 pilihannya mengikuti format*
- [x] Pilih folder simpan, dan folder terakhir diingat
- [ ] Opsi unduh seluruh playlist — *ada sejak Fase 7 (playlist YouTube, pilih semua atau satu per satu), menunggu uji di Windows*
- [x] Progress bar (persen, kecepatan, ETA) di kartu unduhan — *sejak Fase 3; sejak Fase 4 juga saat DANload mengubah format*; panel log belum
- [x] Pengecekan yt-dlp dan FFmpeg saat aplikasi dibuka, dengan pesan error yang jelas
- [x] **Wizard setup awal** di dalam aplikasi: memasang yt-dlp, FFmpeg, dan Deno otomatis dan mengatur folder unduhan (detail di Bagian 2.9)
- [ ] **Installer Windows** yang mudah dipakai orang awam (detail di Bagian 2.9)
- [x] Pengaturan disimpan di `%APPDATA%\DANload\config.json`
- [ ] **Pustaka (Library):** menampilkan semua file yang sudah diunduh, bisa **dihapus** dan **di-rename** dari dalam aplikasi (detail di Bagian 2.8) — *ada sejak Fase 6, menunggu uji di Windows*
- [x] UI tidak macet (freeze) selama proses unduh — *unduhan berjalan di thread terpisah sejak Fase 3*
- [x] Ikon aplikasi

**Tambahan (setelah MVP stabil):**
- [ ] Antrean (queue) beberapa URL sekaligus — *ada sejak Fase 7 (maks. 3 bersamaan, Instagram 1, diingat setelah aplikasi ditutup), menunggu uji di Windows*
- [x] Tombol batal (cancel) unduhan yang sedang berjalan — *sejak Fase 3 (dimajukan dari Fase 7); sejak Fase 4 juga menghentikan konversi*
- [ ] Edit singkat lain di pustaka (mis. potong/trim audio-video via FFmpeg) — dipertimbangkan nanti
- [ ] Pratinjau info video sebelum unduh (judul, durasi, thumbnail, daftar kualitas)
- [ ] Unduh subtitle
- [ ] Tombol update yt-dlp & FFmpeg dari dalam aplikasi (memakai mekanisme yang sama dengan wizard) — *pasang ulang versi terbaru sudah bisa lewat wizard sejak Fase 2B; pengecekan otomatis apakah ada versi baru belum*
- [ ] Dukungan cookies browser (untuk konten yang butuh login, mis. sebagian Instagram)
- [ ] Opsi tema terang (default: gelap)

### 2.4 Format yang Didukung

| Mode  | Format | Cara dibuat (sejak Fase 4) | Catatan |
|-------|--------|----------------------------|---------|
| Video | MP4 | yt-dlp. Resolusi dulu, lalu di resolusi yang sama **H.264** > H.265 > VP9 > AV1, SDR, audio AAC (`-S res,fps,source,+vcodec:avc,+hdr,acodec:aac`) | Default, paling kompatibel. Di atas 1080p biasanya VP9/AV1 (keputusan 2026-11-15) |
| Video | MKV | yt-dlp, urutan bawaan yt-dlp (kualitas terbaik), tanpa re-encode | Bisa AV1/VP9 dan HDR |
| Video | WEBM | yt-dlp langsung ke WEBM (VP9/AV1 + Opus, SDR diutamakan, `--merge-output-format webm/mkv`); jika kodek tidak cocok → DANload re-encode ke VP9 + Opus | Format asli YouTube |
| Video | MOV | yt-dlp memilih H.264 + AAC (walau resolusinya lebih rendah) → MP4 → DANload menyalin ke MOV; tanpa H.264 → DANload re-encode ke H.264 + AAC | Di YouTube biasanya maks. 1080p |
| Video | AVI | DANload re-encode ke Xvid (encoder MPEG-4 bawaan FFmpeg, tag `XVID`, `-q:v 3`) + MP3 192 kbps | Maks. 1080p, ukuran besar, lambat |
| Video | MPEG (`.mpg`) | DANload re-encode ke MPEG-2 (`-q:v 3`) + MP2 224 kbps, MPEG-2 Program Stream | Maks. 1080p, ukuran besar, lambat |
| Audio | MP3 | yt-dlp `-x`, bitrate dipilih (128–320 kbps) | Default |
| Audio | M4A (AAC) | yt-dlp `-x`; AAC asli disalin tanpa re-encode | Kualitas "Asli" |
| Audio | OPUS | yt-dlp `-x`; Opus asli disalin tanpa re-encode | Kualitas "Asli" |
| Audio | OGG (Vorbis) | yt-dlp `-x --audio-format vorbis`, bitrate dipilih | |
| Audio | FLAC | yt-dlp `-x`, dipaksa 16-bit | Lossless (ukuran besar, tapi kualitas tetap terbatas oleh sumber) |
| Audio | WAV | yt-dlp `-x`, PCM 16-bit | Tanpa kompresi |

Catatan: format yang butuh **re-encode** diberi keterangan di UI. AVI dan MPEG selalu re-encode dan ditandai kuning ("proses lebih lama"); MOV dan WEBM hanya re-encode jika kodek sumbernya tidak cocok.
Keterangannya mengikuti platform (sejak Fase 5): Facebook dan Instagram umumnya hanya menyediakan H.264 + AAC, jadi WEBM dari keduanya biasanya re-encode (ditandai kuning) dan OPUS diubah dari AAC, sedangkan MOV dan M4A cukup disalin.
Encode ulang dikerjakan oleh **langkah FFmpeg milik DANload** (bukan `--recode-video` yt-dlp), supaya ada persen + sisa waktu, bisa dibatalkan, dan MPEG bisa dibuat *(keputusan pemilik projek, 2026-11-15)*.

### 2.5 Teknologi

| Bagian | Pilihan | Status |
|--------|---------|--------|
| Bahasa | Rust (edition terbaru yang stabil) | ✅ Ditetapkan |
| Target OS | Windows 10/11 (64-bit) | ✅ Ditetapkan |
| Mesin unduh | **yt-dlp** (dipanggil sebagai proses eksternal) | ✅ Ditetapkan |
| Konversi/merge | **FFmpeg** + **ffprobe** (proses eksternal). Merge dan ambil audio oleh yt-dlp; encode/salin ke WEBM, MOV, AVI, MPEG oleh langkah konversi DANload (`src/convert.rs`) dengan `-progress pipe:1` | ✅ Ditetapkan (langkah DANload sejak Fase 4) |
| GUI | **Slint 1.18** dengan **software renderer** (tanpa GPU); fitur: `std`, `compat-1-2`, `backend-winit`, `renderer-software`, `software-renderer-path`. Cadangan: `egui` jika Slint gagal memenuhi target | ✅ Ditetapkan |
| Ikon .exe | `winresource` (hanya saat build) | ✅ Ditetapkan |
| Async/proses | `std::thread` + channel (**tanpa tokio**) | ✅ Ditetapkan |
| Config | `serde` + `serde_json` | ✅ Ditetapkan |
| Dialog bawaan Windows (pilih folder, kotak pesan) | `rfd` | ✅ Ditetapkan |
| Data pustaka/riwayat | File **JSON** (`serde_json`) di `%APPDATA%\DANload\` | ✅ Ditetapkan |
| Lokasi yt-dlp/FFmpeg | `%LOCALAPPDATA%\DANload\bin\` → folder `bin\` di samping `DANload.exe` → `PATH` (urutan pencarian) | ✅ Ditetapkan |
| Unduh komponen (wizard) | `ureq` 3 (HTTPS, sertifikat dari Windows lewat fitur `platform-verifier`) + `sha2` (checksum) + `zip` (ekstrak, fitur `deflate-flate2-zlib-rs`) | ✅ Ditetapkan (Fase 2B) |
| Sumber yt-dlp | GitHub resmi `yt-dlp/yt-dlp`: `releases/latest/download/yt-dlp.exe`, checksum dari `SHA2-256SUMS`. Versi minimal **2025.11.12** (dukungan runtime JavaScript) | ✅ Ditetapkan |
| Runtime JavaScript (YouTube) | **Deno**, dipasang wizard dari GitHub resmi `denoland/deno`: `deno-x86_64-pc-windows-msvc.zip` (±43 MB, isinya `deno.exe` ±98 MB), checksum dari `.zip.sha256sum` (format keluaran `Get-FileHash` PowerShell). Versi minimal **2.3.0**. Diberikan ke yt-dlp lewat `--js-runtimes deno:PATH` | ✅ Ditetapkan 2026-11-14 |
| Sumber FFmpeg | Utama: **gyan.dev** `ffmpeg-release-essentials.zip`, checksum dari `ffmpeg-release-essentials.zip.sha256` (±109 MB untuk versi 9.0.2, dicek 2026-11-14). Cadangan: **BtbN** `ffmpeg-master-latest-win64-gpl.zip`, checksum dari `checksums.sha256` (ukurannya lebih besar). Keduanya ditautkan dari ffmpeg.org | ✅ Ditetapkan |
| Installer | **Inno Setup** | ✅ Ditetapkan |
| Hapus file | Pindah ke **Recycle Bin** lewat API Windows `SHFileOperationW` (`FOF_ALLOWUNDO`), bukan hapus permanen | ✅ Ditetapkan (Fase 6) |
| Fungsi Windows langsung (Recycle Bin, buka file dengan aplikasi bawaan, jam lokal) | Crate **`windows-sys` 0.61**, khusus build Windows (versi yang sama sudah dipakai rfd/arboard/zip). Semua kode `unsafe` dikumpulkan di `src/system.rs` | ✅ Ditetapkan 2026-11-16 (pemilik projek memilih opsi ini, bukan crate `trash`) |
| Thumbnail Pustaka | Thumbnail asli dari platform (`--write-thumbnail` yt-dlp), dikecilkan FFmpeg menjadi JPEG ≤ 320 px di `%LOCALAPPDATA%\DANload\thumbs\` | ✅ Ditetapkan 2026-11-16 |
| Error handling | `anyhow` / `thiserror` | ✅ Disarankan |

**Keputusan (disetujui pemilik projek, 2026-11-13):** mesin unduh memakai **yt-dlp + FFmpeg**; Rust hanya untuk **GUI dan pemrosesan** (pengatur proses, pembaca progress, pustaka, konfigurasi). Alasannya: YouTube, Facebook, dan Instagram sering mengubah sistemnya, dan yt-dlp dirawat aktif oleh komunitas, sehingga aplikasi cukup memperbarui yt-dlp tanpa perawatan kode ekstraktor sendiri. **Jangan** menulis ekstraktor situs sendiri di Rust.

### 2.6 Target Performa (Wajib)

Disetujui pemilik projek, 2026-11-14.

| Kondisi | Target |
|---------|--------|
| RAM proses `DANload.exe` saat **idle** (jendela ukuran normal, tidak sedang mengunduh) | **< 50 MB** |
| CPU proses `DANload.exe` saat **idle** | **< 5%** |

- Yang diukur hanya proses DANload. Proses **yt-dlp** dan **FFmpeg** tidak dihitung, karena berjalan terpisah dan hanya aktif saat mengunduh/konversi.
- Pengukuran memakai Task Manager (kolom *Memory* / *Private working set*), build **release** (`cargo build --release`).
- Kenapa Slint + software renderer: tidak memuat stack GPU/driver grafis yang bisa menambah puluhan MB, dan hanya menggambar ulang bagian UI yang berubah.
- Aturan untuk menjaga target:
  - Tidak ada animasi/timer yang menggambar ulang terus-menerus saat idle.
  - Progress unduhan dikirim ke UI dengan batas frekuensi (mis. maksimal ±10 kali per detik).
  - Thumbnail dimuat dalam ukuran kecil, hanya untuk item yang terlihat, dan di-cache di disk (bukan disimpan semua di RAM).
  - FFmpeg dijalankan dengan prioritas proses rendah supaya komputer tidak terasa berat saat konversi.
  - Jangan menambah crate berat tanpa alasan jelas dan persetujuan.
- Jika target tidak tercapai, **laporkan angka hasil ukur** dan usulkan perbaikan sebelum melanjutkan fase berikutnya.

**Riwayat hasil ukur di Windows**

| Tanggal | Build | RAM | CPU | Catatan |
|---------|-------|-----|-----|---------|
| 2026-11-14 | Fase 2B, release | 9,8 MB | 0% | Dilaporkan pemilik projek (Task Manager, tab Processes) |

### 2.7 Desain UI

Acuan visual: `docs/referensi-ui/referensi-ui-1.png` dan `docs/referensi-ui/referensi-ui-2.png`.
Yang ditiru adalah **gaya dan tata letaknya**, bukan nama, logo, atau konten aplikasi di gambar tersebut.

**Gaya umum**
- Tema **gelap** (latar hampir hitam, panel abu-abu gelap, teks putih/abu-abu muda).
- Sudut membulat (*rounded*) pada panel, tombol, kolom input, dan *chip*.
- Satu warna aksen untuk tombol utama; hijau/merah/biru hanya untuk status (selesai, gagal, sedang mengunduh).
- Font sans-serif bersih; hierarki jelas (judul tebal, info sekunder abu-abu kecil).
- Jendela tanpa bingkai bawaan Windows (*custom title bar*) dengan tombol minimize/maximize/close sendiri — opsional, jika library GUI mendukung.

**Tata letak**
```
┌──────────────────────────────────────────────────────────────────┐
│ ● DANload        [ 🔍 Cari file unduhan...  ]   [YT][FB][IG]  _ □ x │
├────┬──────────────────────────────────────────┬───────────────────┤
│ ⬇  │ [Semua][Video][Audio][YouTube][FB][IG]   │  PANEL KANAN      │
│ 📚 │ ┌──────────────────────────────────────┐ │  - Unduh baru:    │
│ ⏳ │ │ 🖼 Judul video        MP4 · 1080p   ⋮ │ │    URL, mode,     │
│ ⚙  │ │    YouTube · 45 MB · 13 Nov 19:40    │ │    format,        │
│    │ │ 🖼 Judul audio        MP3 · 192k    ⋮ │ │    kualitas, nama │
│    │ │    Instagram · 4 MB · 12 Nov 21:02   │ │    file, folder,  │
│    │ └──────────────────────────────────────┘ │    [ ⬇ Unduh ]    │
│    │                                          │  - atau detail    │
│    │                                          │    file terpilih  │
├────┴──────────────────────────────────────────┴───────────────────┤
│ Siap.                         ● 2 sedang mengunduh   Batal semua v0.1.0 │
└──────────────────────────────────────────────────────────────────┘
```
- **Sidebar kiri (ikon):** Unduh, Pustaka, Antrean (dengan badge jumlah yang menunggu, sejak Fase 7), Pengaturan.
- **Bar atas:** nama aplikasi, kolom pencarian, dan **pilihan platform** (YouTube / Facebook / Instagram). Pengguna memilih platform sebelum mengunduh (bawaan YouTube); label dan contoh URL di panel kanan ikut berganti, dan di bawah kolom URL muncul pesan apakah link cocok dengan platform terpilih. Platform **tidak** berganti otomatis mengikuti URL.
- **Area tengah:** daftar item dengan thumbnail, judul, platform, format, kualitas, ukuran, tanggal; *chip* filter di atas daftar; centang untuk pilih banyak.
- **Panel kanan:** form unduh baru, atau detail file yang sedang dipilih (thumbnail besar, info, tombol aksi).
- **Status bar bawah:** status, jumlah unduhan (`N berjalan · M menunggu`), tombol "Batal semua" (sejak Fase 7, dengan konfirmasi), versi aplikasi.

### 2.8 Fitur Pustaka (Library)

Menampilkan file yang sudah diunduh lewat DANload dan mengelolanya langsung dari aplikasi.

| Fitur | Perilaku yang diharapkan |
|-------|--------------------------|
| Daftar unduhan | Thumbnail, judul, platform, format, kualitas, ukuran, durasi, tanggal unduh, lokasi file |
| Cari & filter | Cari berdasarkan nama; filter Video/Audio dan per platform; urutkan berdasarkan tanggal/nama/ukuran |
| Buka | Buka file dengan pemutar bawaan Windows; tombol "Tampilkan di folder" (Explorer dengan file terpilih) |
| **Rename** | Ubah nama file langsung dari aplikasi. Validasi karakter terlarang Windows (pakai aturan yang sama dengan kolom NAMA FILE, `src/filename.rs`), tolak jika nama sudah dipakai, ekstensi tidak boleh ikut berubah, catatan pustaka ikut diperbarui |
| **Hapus** | Pilihan: (a) hapus dari daftar saja, atau (b) hapus file → dipindah ke **Recycle Bin**. Selalu ada dialog konfirmasi; mendukung hapus banyak sekaligus |
| File hilang | Jika file dipindah/dihapus di luar aplikasi, item ditandai "File tidak ditemukan" (tidak error), dengan opsi hapus dari daftar |
| Salin URL sumber | Salin link asli video ke clipboard |
| Unduh ulang | Unduh lagi dari URL sumber dengan format yang sama atau berbeda |

Data pustaka disimpan di `%APPDATA%\DANload\` (bukan di folder hasil unduhan), sehingga tetap ada walaupun folder unduhan diganti.

**Cara kerja (sejak Fase 6, `src/library.rs`, `src/system.rs`, `ui/library.slint`)**
- **Pencatatan:** unduhan yang berhasil langsung dicatat ke `library.json` (satu item per file, termasuk tiap video dari postingan berisi beberapa video). Hanya unduhan sejak Fase 6; file lama tidak diimpor.
- **Isi item:** lokasi, judul asli, platform, mode, format, kualitas **sebenarnya** (ffprobe: sisi terpendek gambar, bitrate, atau Lossless/Tanpa kompresi), ukuran, durasi, waktu unduh (UTC, ditampilkan jam lokal Windows), link sumber, nama thumbnail.
- **Menyimpan:** di thread tersendiri, lewat file sementara lalu rename. File rusak dicadangkan sebagai `library-rusak-<waktu>.json`, dan Pustaka dimulai kosong dengan pesan. Nilai yang tidak dikenal diganti bawaan, dan item yang tidak terbaca ikut disimpan lagi apa adanya.
- **Satu file, satu item:** item lama yang filenya hilang digantikan jika file baru mendapat lokasi yang sama.
- **Thumbnail:** ditampilkan hanya untuk baris yang terlihat (`ListView` + cache gambar Slint maks. 5 MB). Jika gagal diambil, yang tampil ikon video/audio. Dihapus bersama itemnya.
- **Cari/filter/urut:**
  - Cari nama file dan judul (semua kata harus ada, huruf besar/kecil tidak dibedakan). Mengetik di kolom cari langsung membuka Pustaka.
  - Chip jenis dan platform bisa digabung.
  - Urutan: Terbaru, Terlama, Nama (angka dibaca sebagai bilangan), Ukuran terbesar.
  - File yang tersembunyi oleh filter otomatis tidak dicentang dan tidak terbuka di panel detail.
- **Centang:** bar "N file dipilih" menggantikan chip di tempat yang sama, supaya daftar tidak bergeser.
- **Panel detail** (panel kanan, menggantikan form unduh; form tetap ada di belakangnya): thumbnail besar, nama, judul asli, info, dan tombol aksi. Nama/judul/folder yang sangat panjang dipendekkan dengan "…".
- **Rename:** memeriksa nama baru langsung. Nama baru "dipesan" dengan file kosong dulu, jadi file lain tidak pernah tertimpa. Mengganti huruf besar/kecil saja tetap bisa di Windows. File yang sedang dipakai program lain menghasilkan pesan yang jelas.
- **Hapus:** dialog "Pindahkan ke Recycle Bin" / "Hapus dari daftar saja" / "Batal". Recycle Bin memakai `SHFileOperationW` per file di thread lain, dengan `FOF_WANTNUKEWARNING`: Windows memperingatkan jika file akan terhapus permanen (mis. drive jaringan).
- **Buka:** `ShellExecuteW` di thread lain (COM diaktifkan dulu). **Salin URL:** clipboard lewat `TextInput` Slint. **Unduh ulang:** form diisi link, platform, mode, dan format yang sama.
- **File hilang:** diperiksa di thread lain saat aplikasi dibuka dan saat Pustaka dibuka (paling sering sekali per 3 detik). Ukuran yang berubah ikut diperbarui.
- **Pesan hasil aksi:** tampil di status bar dan hilang sendiri setelah 6 detik (error 12 detik).

### 2.9 Instalasi & Wizard Setup Awal

Tujuan: orang awam bisa memasang dan memakai DANload **tanpa membuka terminal** dan tanpa memasang yt-dlp/FFmpeg secara manual.

**Tahap 1 — Installer Windows**
- Memasang `DANload.exe` **per pengguna** di `%LOCALAPPDATA%\Programs\DANload\` (tidak butuh hak Administrator).
- Membuat shortcut Start Menu dan (opsional) Desktop; menyediakan uninstaller di "Apps & features".
- Opsi "Jalankan DANload" di akhir instalasi.
- Installer **tidak** menyertakan yt-dlp/FFmpeg; keduanya dipasang oleh wizard di dalam aplikasi (installer tetap kecil dan komponen selalu versi terbaru).
- Uninstall bertanya apakah data pengguna (`%APPDATA%\DANload\`, komponen di `%LOCALAPPDATA%\DANload\bin\`) ikut dihapus. File hasil unduhan **tidak pernah** dihapus oleh uninstaller.

**Tahap 2 — Wizard setup di dalam aplikasi** (muncul otomatis saat pertama kali dibuka, atau jika komponen hilang/rusak)

| Langkah | Isi |
|---------|-----|
| 1. Selamat datang | Penjelasan singkat apa yang akan dipasang dan kenapa dibutuhkan |
| 2. Komponen | Status yt-dlp, FFmpeg, ffprobe, Deno (Terpasang + versi / Bermasalah, termasuk versi terlalu lama / Belum ada). **Pasang otomatis** (hanya yang belum siap) atau **Pasang versi terbaru** (semua): unduh dari sumber resmi, progress + ukuran, verifikasi checksum, ekstrak, pasang ke `%LOCALAPPDATA%\DANload\bin\`; bisa **Batal**. Alternatif: **Pilih file sendiri...** (`yt-dlp.exe`, `ffmpeg.exe`/`ffprobe.exe`, atau `deno.exe`; program satu paket di folder yang sama ikut disalin). Jika gagal: pesan jelas, tombol **Coba lagi**, dan petunjuk unduh manual. Boleh **Lewati dulu** |
| 3. Folder unduhan | Pilih folder (awal: `%USERPROFILE%\Downloads\DANload`); opsi subfolder otomatis `Video\` dan `Audio\` |
| 4. Selesai | Ringkasan (komponen, folder, subfolder) → **Mulai pakai DANload** menandai setup selesai |

*Catatan Fase 2B: rancangan awal 5 langkah; langkah "cek" dan "pasang" digabung menjadi satu langkah "Komponen". Tombol "Buka halaman unduhan" diganti petunjuk teks yang muncul saat pemasangan gagal.*

Aturan wizard:
- Bisa dibuka lagi kapan saja dari **Pengaturan** → tombol **Pasang / perbarui...** (pasang ulang, update komponen, ganti folder unduhan).
- **Nanti saja** menutup wizard tanpa menandai setup selesai, jadi wizard muncul lagi saat aplikasi dibuka berikutnya. Jika setup sudah selesai tetapi ada komponen yang hilang/rusak, wizard terbuka otomatis di langkah Komponen.
- Aplikasi tetap bisa dibuka walau komponen belum ada, tetapi tombol Unduh dinonaktifkan dengan pesan jelas + tombol menuju wizard.
- Sumber unduhan hanya dari sumber resmi: yt-dlp dari rilis GitHub resmi; FFmpeg dari build Windows yang ditautkan di situs resmi ffmpeg.org. URL ditulis di satu tempat di kode supaya mudah diganti.
- Verifikasi checksum (SHA-256) **wajib**; semua sumber yang dipakai menyediakannya. Jika checksum tidak bisa diambil atau tidak cocok, file dibuang dan sumber cadangan dicoba (FFmpeg: gyan.dev → BtbN); kalau semua gagal, pengguna diberi tahu.
- File diunduh **langsung ke disk secara streaming** (bukan ditampung di RAM), ke folder sementara dulu, lalu dipindah setelah lengkap & terverifikasi — supaya tidak ada file setengah jadi.
- Tangani kondisi: tidak ada internet, unduhan terputus (bisa coba lagi), ruang disk tidak cukup, antivirus memblokir file.
- Ganti folder unduhan **tidak** memindahkan file lama; Pustaka tetap menyimpan lokasi asli setiap file.
- Lisensi: FFmpeg dan yt-dlp diunduh dari sumber resminya oleh pengguna sendiri (tidak dibundel di installer). Cantumkan nama & lisensi keduanya di halaman "Tentang".
- **Runtime JavaScript (diputuskan 2026-11-14):** sejak yt-dlp 2025.11.12, YouTube butuh runtime JavaScript; tanpa itu dukungan YouTube dianggap *deprecated*. Dipilih **Deno** (paling direkomendasikan yt-dlp dan satu-satunya yang aktif bawaan) sebagai komponen keempat yang dipasang wizard. Lisensi Deno juga dicantumkan di halaman "Tentang".

### 2.10 Perilaku Unduhan (sejak Fase 3, diperbarui Fase 4, 5, dan 7)

- **Syarat tombol Unduh aktif:** semua komponen siap, link cocok dengan platform terpilih **dan** berupa link satu video/postingan atau playlist YouTube, dan nama file (jika diisi) boleh dipakai. Sejak Fase 7 tombolnya **tidak terkunci** saat ada unduhan berjalan: unduhan baru masuk antrean.
- **Jenis link (sejak Fase 5, `src/platform.rs`):** channel YouTube, `watch_videos`, profil/tag/story Instagram, dan beranda ditolak dengan pesan. Link `watch?v=…&list=…` boleh (bawaannya hanya videonya, `--no-playlist`). Link Facebook tidak diperiksa lebih jauh karena bentuknya sangat beragam. Pengaman tambahan: paling banyak 20 item per link (`--playlist-items :20`).
- **Playlist YouTube (sejak Fase 7, `src/playlist.rs`):** link `playlist?list=…` dan `watch?list=…`, atau link `watch?v=…&list=…` / `youtu.be/…?list=…` dengan centang "Unduh seluruh playlist". Isinya dibaca dengan `--flat-playlist --yes-playlist` dan `--print` JSON (bisa dibatalkan, jumlah yang terbaca tampil), lalu pengguna memilih videonya di dialog: semua video yang bisa diunduh dicentang, video privat/dihapus/belum tayang/khusus anggota/butuh login/sedang live tidak bisa dipilih. Tiap video yang dipilih masuk antrean sebagai unduhan sendiri (link `watch?v=ID`), disimpan langsung di folder unduhan dengan judulnya (tanpa subfolder dan tanpa nomor urut; NAMA FILE tidak berlaku).
- **Antrean dan unduhan bersamaan (sejak Fase 7, `src/queue.rs`):** paling banyak N unduhan berjalan (Pengaturan, 1–3, bawaan 3, `max_parallel_downloads`); **Instagram satu per satu** (unduhan platform lain di belakangnya tetap boleh mulai). Halaman Antrean: Jeda/Lanjutkan (yang berjalan tetap diselesaikan), Hapus, Kosongkan (konfirmasi). "Coba lagi" memasukkan unduhan ke antrean paling depan. "Batal semua" (konfirmasi) menghentikan semua yang berjalan dan mengosongkan antrean. Format, kualitas, folder, dan nama dicatat saat unduhan ditambahkan.
- **Antrean diingat (sejak Fase 7):** `%APPDATA%\DANload\queue.json` (unduhan yang berjalan di depan, lalu yang menunggu, plus status jeda), disimpan di thread lain setiap ada perubahan. Saat jendela ditutup isinya dibekukan dulu, jadi unduhan yang dihentikan karena aplikasi ditutup ikut tercatat. Saat dibuka lagi, antrean berjalan otomatis setelah pemeriksaan komponen (kecuali dijeda); unduhan yang terputus diulang dari awal. File rusak dicadangkan sebagai `queue-rusak-<waktu>.json`.
- **Konversi berat bergiliran (sejak Fase 7):** langkah konversi DANload yang mengubah ulang video hanya satu per waktu; unduhan lain menunggu dengan keterangan "Menunggu giliran konversi" (masih bisa dibatalkan). Langkah yang hanya menyalin stream tidak menunggu.
- **Format dan kualitas:** lihat tabel di 2.4. Format terakhir per mode, resolusi, dan bitrate diingat di config. Batas resolusi memakai urutan `-S res:N` (sisi terpendek gambar, jadi video vertikal 1080x1920 tetap 1080p, dan tidak pernah error "format tidak tersedia"), bukan saringan `[height<=N]` seperti di Fase 3. Menu kualitas mengikuti format: AVI/MPEG hanya 1080p–360p; M4A/OPUS/FLAC/WAV tidak bisa dipilih ("Asli", "Lossless", "Tanpa kompresi").
- **Nama file (sejak Fase 5, `src/filename.rs`):** judul video tanpa ID, paling panjang 100 karakter (`%(title).100s`; karakter terlarang Windows diganti yt-dlp), atau nama dari kolom **NAMA FILE** (diperiksa langsung di UI: karakter `\ / : * ? " < > |`, nama perangkat Windows, panjang; ekstensi yang ikut diketik dibuang; kolom dikosongkan setelah unduhan dimulai). Jika nama sudah dipakai, file baru menjadi `nama (2).ext`, `nama (3).ext`, dst. File lain **tidak pernah ditimpa**: nama dipesan dulu dengan membuat file kosong, lalu hasilnya dipindah ke sana. *Fase 4 sempat memakai `Judul [ID].ext` dan melewati unduhan jika file sudah ada; diganti atas permintaan pemilik projek (2026-11-15).*
- **Beberapa video dalam satu link (sejak Fase 5):** mis. carousel Instagram. Semua video diunduh, masing-masing jadi file sendiri (bernomor jika namanya sama); foto dilewati. Jika sebagian item gagal atau berupa foto, unduhan tetap berhasil dengan keterangan ("1 item dilewati: …"). Teks kemajuan memakai nomor, mis. "Mengunduh video (2/3) · 45%".
- **Folder:** folder unduhan, ditambah `Video\` / `Audio\` jika subfolder aktif. Semua hasil yt-dlp (semua format, sejak Fase 5) ditaruh dulu di `.danload-tmp\<job>\hasil\<ID>\Judul.ext` di dalam folder itu (drive yang sama, jadi pemindahannya instan), lalu DANload memindahkannya dengan nama akhir. Folder sementara **selalu dihapus** setelah berhasil, gagal, atau dibatalkan.
- **Langkah konversi DANload (WEBM, MOV, AVI, MPEG):** jika ekstensi hasil yt-dlp sudah sama dengan format tujuan, file langsung dipindah. Jika belum, ffprobe membaca kodek dan tiap stream **disalin** (sudah cocok) atau **diubah ulang**, lalu FFmpeg dijalankan dengan `-progress pipe:1` (persen = waktu terproses ÷ durasi, sisa waktu dari `speed=`). Hasilnya ditulis di folder sementara, lalu dipindah ke folder unduhan (dicoba ulang beberapa kali jika file dikunci antivirus).
- **Cara membaca kemajuan:** yt-dlp dijalankan dengan `--progress-template`, `--print before_dl:` (judul dan nomor item), dan `--print after_move:` (lokasi file hasil di folder sementara), yang mencetak baris khusus berawalan `DANLOAD|`. Progress unduhan keluar di stdout, tahap FFmpeg di stderr, jadi keduanya dibaca. Laporan ke UI dibatasi ±10 kali per detik.
- **Opsi tetap:** `--ignore-config` (abaikan config yt-dlp milik pengguna), `--encoding utf-8`, `--no-playlist` (playlist dibaca terpisah sejak Fase 7, lalu tiap videonya diunduh sendiri), `--windows-filenames`, `--no-mtime`, `--ffmpeg-location`, `--js-runtimes deno:PATH`, dan `--` sebelum URL.
- **Proses:** yt-dlp dan FFmpeg milik langkah konversi berjalan tanpa jendela console dan berprioritas rendah (`BELOW_NORMAL`); FFmpeg yang dijalankan yt-dlp ikut berprioritas rendah karena diwariskan.
- **Batal:** menghentikan yt-dlp atau FFmpeg yang sedang berjalan beserta semua proses anaknya (`taskkill /T /F`), lalu membersihkan folder sementara. File yang sudah dipindah ke folder unduhan sebelum pembatalan tiba tetap dilaporkan selesai dan dicatat di Pustaka (sejak Fase 7). Menutup jendela saat unduhan berjalan memunculkan konfirmasi; jika disetujui, semua unduhan dan pembacaan playlist dihentikan bersamaan (paling lama 5 detik) sebelum aplikasi tertutup.
- **Error:** baris `ERROR:` dari yt-dlp diterjemahkan menjadi pesan berbahasa Indonesia (video privat, dibatasi umur, verifikasi bukan bot, khusus anggota, belum tayang, tidak tersedia di negara ini, dihapus, kualitas tidak tersedia, link tidak didukung, disk penuh, izin folder, gagal memproses YouTube, terlalu banyak permintaan, 404, 403, koneksi). Sejak Fase 5 juga pesan Instagram/Facebook (dibatasi tanpa login, akun privat, data postingan kosong, postingan hanya foto, Facebook tidak memberi data, video tidak tersedia menurut Facebook, butuh login); teksnya dicocokkan dengan kode sumber yt-dlp 2026.10.26. Pesan yang tidak dikenal ditampilkan apa adanya tanpa awalan teknis. Error FFmpeg di langkah konversi juga diterjemahkan (encoder tidak ada, disk penuh, izin folder).
- **Daftar unduhan:** hanya untuk sesi ini (terbaru di atas, `ListView` sejak Fase 7), dengan tombol Batal, Tampilkan di folder, dan Coba lagi. Unduhan yang belum dimulai ada di halaman Antrean. Riwayat permanen ada di Pustaka: setiap unduhan yang berhasil otomatis dicatat (sejak Fase 6, lihat 2.8).
- **Data untuk Pustaka (sejak Fase 6):** `--write-thumbnail` dan `--no-write-playlist-metafiles`. Judul dan lokasi tiap file dicetak sebagai JSON (`after_move:…%(.{filepath,title})j`; judul di kartu juga `%(title)j`, supaya judul berisi baris baru tidak terpotong). Setelah file dipindah, ffprobe membaca durasi dan kualitasnya, dan thumbnail dikecilkan FFmpeg. Gagal di langkah ini tidak membuat unduhan gagal.

---

## 3. Aturan Catatan Perubahan (Wajib)

**Setiap kali ada perubahan pada projek** (kode, konfigurasi, dependensi, struktur folder, atau dokumen), buat **satu file Markdown baru** di folder `changes/`.

### 3.1 Penamaan File
```
changes/YYYY-MM-DD_nama-perubahan.md
```
Contoh:
- `changes/2026-11-13_inisialisasi-claude-md.md`
- `changes/2026-11-15_setup-cargo-dan-gui-dasar.md`
- `changes/2026-11-18_fix-progress-bar-macet.md`

Jika ada lebih dari satu perubahan di hari yang sama, buat file terpisah untuk tiap perubahan. Jangan menimpa file catatan yang sudah ada.

### 3.2 Template Isi
```markdown
# [Judul Singkat Perubahan]

- **Tanggal:** YYYY-MM-DD
- **Jenis:** Fitur baru / Perbaikan bug / Refactor / Dokumentasi / Dependensi / Lainnya
- **Status:** Selesai / Sebagian / Perlu dicek

## Latar Belakang
Kenapa perubahan ini dilakukan (permintaan, bug, atau kebutuhan).

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| src/main.rs | Diubah | ... |
| src/downloader.rs | Dibuat | ... |

## Detail Perubahan
Penjelasan teknis singkat: apa yang ditambah/diubah dan bagaimana cara kerjanya.

## Dependensi
Crate yang ditambah/dihapus/diperbarui (atau "Tidak ada").

## Cara Menguji
Langkah untuk memastikan perubahan bekerja.

## Hasil Ukur Performa
RAM idle: ... MB · CPU idle: ... % (atau "tidak relevan").

## Catatan / Yang Perlu Dicek Ulang
Risiko, keterbatasan, atau hal yang belum selesai.
```

### 3.3 Ringkasan Perubahan
Selain file di `changes/`, tambahkan **satu baris** ke `CHANGELOG.md` di root projek (urutan terbaru di atas) berisi tanggal, judul, dan link ke file detailnya.

---

## 4. Lingkungan Pengembangan

| Kebutuhan | Keterangan |
|-----------|------------|
| OS | Windows 10/11 64-bit (laptop pemilik projek) |
| Rust | Toolchain **stable**, target `x86_64-pc-windows-msvc` (dipasang lewat `rustup`) |
| Build tools | Visual Studio Build Tools (komponen "Desktop development with C++") — dibutuhkan toolchain MSVC |
| Installer | Inno Setup 6 (baru dibutuhkan di Fase 8) |
| Editor | VS Code / Antigravity + ekstensi `rust-analyzer` dan Slint |

- Build, menjalankan aplikasi, dan pengukuran RAM/CPU dilakukan **di Windows pemilik projek**. Jika Claude tidak bisa menjalankan perintah langsung di Windows, Claude memberikan perintah yang harus dijalankan beserta hasil yang diharapkan, lalu pemilik projek melaporkan hasilnya.
- Sebelum Fase 1, pastikan `rustc --version` dan `cargo --version` sudah bisa dijalankan di terminal.
- Projek memakai git dengan remote di GitHub milik pemilik projek. `git commit` dan `git push` dijalankan oleh pemilik projek (atau oleh Claude hanya dengan izin, lihat Bagian 1).
- `cargo test` menjalankan semua unit test tanpa internet. `cargo test official_checksums -- --ignored` menjalankan test yang menghubungi sumber resmi yt-dlp/FFmpeg/Deno (butuh internet; hanya mengambil file checksum yang kecil).
- Menguji wizard tanpa internet (khusus build debug): jalankan server lokal berisi `yt-dlp.exe`, `SHA2-256SUMS`, `ffmpeg-release-essentials.zip`, `ffmpeg-release-essentials.zip.sha256`, `deno-x86_64-pc-windows-msvc.zip`, dan `deno-x86_64-pc-windows-msvc.zip.sha256sum`, lalu set `DANLOAD_DEV_MIRROR=http://127.0.0.1:8000` sebelum `cargo run`. Build rilis mengabaikan variabel ini.
- Menguji dengan yt-dlp dan FFmpeg sungguhan (tidak jalan otomatis): isi `DANLOAD_TEST_YTDLP`, `DANLOAD_TEST_FFMPEG_DIR`, `DANLOAD_TEST_DENO`, dan `DANLOAD_TEST_URL` (opsional `DANLOAD_TEST_HEIGHT`, bawaan 360), lalu `cargo test real_ -- --ignored --nocapture --test-threads 1`. Isinya: pemilihan format dengan data video tiruan (tanpa internet), unduh ke-12 format + cek kodek hasilnya + unduh ulang (harus menjadi `(2)`) + nama kustom, dan batal di tengah konversi. `DANLOAD_TEST_URL` boleh link YouTube, Facebook, atau Instagram. Sejak Fase 6 uji unduh juga memeriksa data Pustaka (kualitas, durasi, thumbnail). Hasilnya ditaruh di folder sementara sistem.

---

## 5. Alur Kerja Setiap Tugas

1. Baca `CLAUDE.md` ini dan file yang relevan.
2. Jelaskan pemahaman + rencana singkat (Bagian 1) → **tunggu persetujuan**.
3. Kerjakan perubahan.
4. Jalankan pemeriksaan (jika toolchain tersedia):
   - `cargo fmt`
   - `cargo clippy -- -D warnings`
   - `cargo build`
   - `cargo test` (jika ada test)
5. Buat file catatan di `changes/` + update `CHANGELOG.md` (Bagian 3).
6. Laporkan singkat: apa yang dikerjakan, file apa saja yang dibuat/diubah, hasil build/test, dan bagian yang perlu dicek ulang.

---

## 6. Aturan Penulisan Kode

- **Bahasa:** nama variabel, fungsi, dan struct dalam **bahasa Inggris**; **komentar** dan **teks UI** dalam **Bahasa Indonesia**.
- Ikuti gaya standar Rust (`cargo fmt`), tanpa warning `clippy`.
- **Hindari `unwrap()` / `expect()`** di kode non-test; tangani error dengan `Result` dan tampilkan pesan yang ramah ke pengguna.
- **UI thread tidak boleh diblok.** Proses unduh berjalan di thread/task terpisah dan mengirim progress lewat channel.
- Pisahkan logika: GUI ↔ pengatur unduhan ↔ konfigurasi. Logika unduh harus bisa diuji tanpa GUI.
- Saat membangun argumen untuk yt-dlp/FFmpeg, berikan argumen sebagai **daftar terpisah** (bukan satu string shell) supaya URL atau nama file aneh tidak merusak perintah.
- Nama file hasil unduhan harus aman untuk Windows (tanpa karakter `\ / : * ? " < > |`).
- Fungsi publik dan bagian yang rumit diberi komentar penjelasan, karena projek ini juga dipakai sebagai bahan belajar Rust.
- Jangan menambah crate jika fungsinya bisa dikerjakan dengan mudah memakai library standar.

**Khusus Windows & Slint**
- Build release memakai `#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]` supaya tidak muncul jendela console hitam.
- Proses yt-dlp/FFmpeg dijalankan dengan flag `CREATE_NO_WINDOW` supaya tidak ada console yang berkedip saat mengunduh.
- Tampilan ditulis di file `.slint` (folder `ui/`); logika di Rust. Jangan menaruh logika bisnis di file `.slint`.
- Update UI dari thread lain hanya lewat mekanisme resmi Slint (mis. `slint::invoke_from_event_loop` atau `Weak` handle), bukan menyentuh komponen langsung dari thread pekerja.
- Path selalu memakai `PathBuf`/`Path`, bukan string yang digabung manual; dukung nama file dengan spasi dan karakter non-ASCII.

---

## 7. Struktur Folder (Rencana Awal)

Struktur ini boleh berkembang seiring fase, tetapi setiap perubahan struktur harus dikonfirmasi dulu.

```
DANload/                 ← D:\Code Project\DANload
├── CLAUDE.md            ← file ini
├── CHANGELOG.md         ← ringkasan semua perubahan
├── README.md            ← cara build & pakai
├── Cargo.toml
├── Cargo.lock           ← versi pasti semua crate (ikut di-commit)
├── build.rs             ← kompilasi .slint + tempel ikon ke .exe
├── .gitignore
├── changes/             ← satu file .md per perubahan
├── docs/
│   └── referensi-ui/    ← gambar acuan desain
├── assets/
│   ├── icon.ico         ← ikon file .exe
│   └── icon.png         ← ikon jendela/taskbar (dibuat dari icon.ico)
├── ui/
│   ├── app-window.slint ← jendela utama
│   ├── settings-page.slint ← halaman Pengaturan (status komponen, folder unduhan)
│   ├── setup-wizard.slint  ← wizard setup awal (4 langkah)
│   ├── downloads.slint  ← kartu unduhan di halaman Unduhan
│   ├── queue.slint      ← halaman Antrean, dialog pilih video dari playlist, dialog konfirmasi
│   ├── library.slint    ← halaman Pustaka, panel detail file, dialog hapus
│   ├── components.slint ← komponen kecil (tombol, chip, kolom isian, dropdown, progress bar)
│   └── theme.slint      ← warna, ukuran, bentuk ikon
├── src/
│   ├── main.rs          ← titik masuk aplikasi
│   ├── app.rs           ← penghubung Slint ↔ logika (callback, thread pekerja)
│   ├── downloader.rs    ← menjalankan yt-dlp, membaca progress, pesan error, batal, beberapa video per link, giliran konversi
│   ├── queue.rs         ← antrean: giliran (maks. 3 bersamaan, Instagram 1), jeda, queue.json
│   ├── playlist.rs      ← membaca isi playlist YouTube (--flat-playlist) untuk dipilih videonya
│   ├── filename.rs      ← cek nama file kustom & nama unik `nama (2).ext`
│   ├── convert.rs       ← langkah konversi DANload dengan FFmpeg (WEBM, MOV, AVI, MPEG) + persennya
│   ├── formats.rs       ← pilihan format video/audio, menu kualitas & argumen yt-dlp
│   ├── process.rs       ← jalankan program tanpa console, prioritas rendah, kill tree, buka Explorer
│   ├── system.rs        ← fungsi Windows: Recycle Bin, buka file, jam lokal (satu-satunya kode `unsafe`)
│   ├── platform.rs      ← platform terpilih, cek kecocokan URL & jenis link (YouTube/Facebook/Instagram)
│   ├── library.rs       ← pustaka: library.json, cari/filter/urut, rename, cek file hilang, thumbnail
│   ├── config.rs        ← baca/tulis config.json, folder unduhan bawaan
│   ├── deps.rs          ← cari yt-dlp/FFmpeg/ffprobe/Deno, baca & cek versinya
│   └── setup.rs         ← unduh, verifikasi checksum, ekstrak, pasang komponen (dipakai wizard)
├── installer/
│   └── DANload.iss      ← skrip installer Inno Setup
└── bin/                 ← (opsional, untuk pengujian lokal) yt-dlp.exe & ffmpeg.exe, TIDAK di-commit
```

---

## 8. Roadmap

| Fase | Isi | Status |
|------|-----|--------|
| 0 | Keputusan teknis: Slint, `std::thread`, JSON, lokasi yt-dlp/FFmpeg, Inno Setup | ✅ |
| 1 | Setup projek Cargo + Slint, jendela GUI kosong, ikon, **ukur RAM/CPU idle** (Bagian 2.6) | ✅ |
| 2A | Config, cek komponen (yt-dlp/FFmpeg/ffprobe), halaman Pengaturan, pilih folder unduhan | ✅ |
| 2B | **Wizard setup awal** (Bagian 2.9) + **pilihan platform** dengan cek kecocokan URL (bawaan YouTube) | ✅ Diuji di Windows 2026-11-14 (RAM 9,8 MB, CPU 0%) |
| 3 | Unduh video MP4 & audio MP3 dari YouTube + progress bar (setara app lama), Deno, tombol Batal (Bagian 2.10) | ✅ Diuji di Windows 2026-11-15 (dilaporkan pemilik projek: semua berjalan) |
| 4 | Tambah format lain (Bagian 2.4): 6 format video + 6 format audio, langkah konversi DANload dengan progress, MP4 utamakan H.264 | ✅ Diuji di Windows 2026-11-16 (dilaporkan pemilik projek: aman) |
| 5 | Dukungan Facebook & Instagram + penanganan error khusus tiap situs; postingan berisi beberapa video; nama file bisa diatur, nama tanpa ID + nomor `(2)` | ✅ Diuji di Windows 2026-11-16 (dilaporkan pemilik projek: aman) |
| 6 | **Pustaka:** daftar unduhan, cari/filter, rename, hapus (Recycle Bin) | ⏳ Kode selesai & diuji di Linux (yt-dlp + FFmpeg asli, data tiruan, UI di layar virtual); menunggu uji di Windows (Recycle Bin, buka file, jam lokal hanya bisa diuji di Windows) |
| 7 | Playlist, antrean (tombol batal sudah ada sejak Fase 3) | ⏳ Kode selesai & diuji di Linux (yt-dlp + FFmpeg asli, data tiruan, UI di layar virtual); menunggu uji di Windows dengan playlist YouTube sungguhan |
| 8 | Poles UI agar sesuai Bagian 2.7, build release, **installer** (Bagian 2.9), README, uji di Windows bersih | ⬜ |

Perbarui kolom status setiap kali satu fase selesai (dan catat di `changes/`).

---

## 9. Definisi "Selesai"

Sebuah tugas dianggap selesai jika:
- [ ] `cargo build` berhasil tanpa error
- [ ] Tidak ada warning dari `cargo clippy`
- [ ] Untuk perubahan yang menyentuh UI/proses: RAM idle < 50 MB dan CPU idle < 5% (build release, dicek di Task Manager) — angka hasil ukur ditulis di file `changes/`
- [ ] Fitur sudah dicoba minimal dengan satu URL nyata (atau dijelaskan cara mengujinya)
- [ ] File catatan di `changes/` sudah dibuat dan `CHANGELOG.md` diperbarui
- [ ] Laporan singkat sudah disampaikan

---

## 10. Batasan & Etika

- Aplikasi ini untuk mengunduh konten yang **boleh diunduh** pengguna (konten sendiri, berlisensi bebas, atau untuk penggunaan pribadi sesuai aturan platform). Tampilkan pengingat singkat di halaman "Tentang".
- **Tidak** menambahkan fitur untuk membobol DRM, mengakses konten privat orang lain, atau menghindari pembatasan login secara tidak sah.
- Jangan menyimpan password, cookies, atau token pengguna di dalam kode maupun di repo.
- File besar (binary yt-dlp/FFmpeg, hasil build di `target/`) tidak di-commit; masukkan ke `.gitignore`.
- **Lisensi Slint:** Slint berlisensi ganda (GPLv3 atau lisensi *royalty-free* dengan syarat atribusi). Sebelum aplikasi dibagikan (Fase 8), cek syarat terbarunya di slint.dev dan penuhi atribusinya di halaman "Tentang". *(Belum diverifikasi ke sumber resmi.)*

---

## 11. Referensi

- Aplikasi lama (Python): `D:\Code Project\Youtube Downloader`
  - `youtube_downloader_gui.py` — versi GUI (Tkinter), acuan fitur dan alur UI
  - `youtube_downloader.py` — versi CLI
  - `icon.ico` — ikon yang bisa dipakai ulang (salin, jangan dipindah)
- Dokumentasi yt-dlp: opsi format (`-f`), urutan format (`-S`), `--merge-output-format`, `-x --audio-format`, `--remux-video`, `--recode-video`, `--postprocessor-args`, `--newline`, `--progress-template`
- Dokumentasi FFmpeg: `-progress`, encoder `mpeg4`, `mpeg2video`, `libvpx-vp9`, `libx264`, muxer `vob`
