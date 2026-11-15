# Fase 6: Pustaka (Daftar File, Cari, Ganti Nama, Hapus ke Recycle Bin)

- **Tanggal:** 2026-11-16
- **Jenis:** Fitur baru, Dependensi
- **Status:** Sebagian — kode selesai dan diuji di Linux (yt-dlp + FFmpeg asli dengan data tiruan, UI di layar virtual). Bagian khusus Windows (Recycle Bin, buka file, jam lokal) sudah dicek tipenya dari Linux, tetapi baru bisa benar-benar diuji di Windows.

## Latar Belakang
- Fase 6 roadmap: Pustaka (CLAUDE.md Bagian 2.8). Isinya: daftar file yang sudah diunduh, cari/filter/urut, buka, ganti nama, hapus (Recycle Bin), tanda file hilang, salin link sumber, dan unduh ulang.
- Keputusan pemilik projek (2026-11-16):
  1. Fungsi Windows (Recycle Bin, buka file, jam lokal) memakai crate `windows-sys` (opsi A). Persetujuan ini sekaligus izin menambah dependensi.
  2. Thumbnail diambil dari platform asli lalu dikecilkan (opsi A).

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| src/library.rs | Dibuat | Data Pustaka: muat/simpan `library.json`, penyimpan di thread lain, cari/filter/urut, ganti nama file, cek file hilang, lokasi thumbnail, label kualitas, dan tanggal berbahasa Indonesia; 16 unit test |
| src/system.rs | Dibuat | Fungsi Windows lewat `windows-sys`: Recycle Bin (`SHFileOperationW`), buka file dengan aplikasi bawaan (`ShellExecuteW`), dan jam lokal (`SystemTimeToTzSpecificLocalTime`). Satu-satunya kode `unsafe` di projek. Ada pengganti untuk Linux (khusus pengujian); 3 unit test |
| ui/library.slint | Dibuat | Halaman Pustaka (chip filter, bar pilihan, daftar `ListView`, keadaan kosong), panel detail, kolom ganti nama, dan dialog konfirmasi hapus |
| src/app.rs | Diubah | Model daftar Pustaka (thumbnail dimuat saat barisnya tampil), semua aksi Pustaka, pencatatan unduhan yang selesai, pesan di status bar yang hilang sendiri; 2 unit test baru |
| src/downloader.rs | Diubah | `--write-thumbnail`; judul dan lokasi tiap file dicetak sebagai JSON; ffprobe untuk durasi dan kualitas sebenarnya; thumbnail dikecilkan ke folder thumbnail. Hasil unduhan sekarang berisi data per file (`SavedFile`); 2 unit test baru |
| src/convert.rs | Diubah | ffprobe juga membaca lebar/tinggi gambar dan bitrate; fungsi `make_thumbnail` (FFmpeg) |
| src/config.rs | Diubah | Lokasi `library.json`; pembaca kolom `or_default` dipakai juga oleh Pustaka |
| src/filename.rs | Diubah | `rename_with_retry` dipakai juga saat menyimpan `library.json` |
| src/platform.rs | Diubah | `Platform` bisa disimpan di JSON (`"youtube"`, `"facebook"`, `"instagram"`) |
| src/main.rs | Diubah | Modul `library` dan `system` |
| ui/app-window.slint | Diubah | Halaman Pustaka, urutan di samping judul, kolom cari aktif (dengan tombol x), panel kanan berganti ke detail file, dialog hapus, dan pesan status |
| ui/components.slint | Diubah | `InputField`: tombol kosongkan, fokus bisa diteruskan, pilih semua teks. Varian merah (`danger`) untuk `PrimaryButton` dan `GhostButton` |
| ui/theme.slint | Diubah | Ikon video, audio, dan peringatan |
| Cargo.toml, Cargo.lock | Diubah | `windows-sys` khusus build Windows (lihat Dependensi) |
| README.md | Diubah | Status Fase 6, cara memakai Pustaka, struktur |
| CLAUDE.md | Diubah | 2.3, 2.5, 2.8, 2.10, 4, 7, roadmap |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan

**Data Pustaka (`%APPDATA%\DANload\library.json`)**
- Isi tiap file: lokasi, judul asli, platform, mode, format, kualitas, ukuran, durasi, waktu diunduh, link sumber, dan nama thumbnail.
- Unduhan yang berhasil langsung dicatat, termasuk tiap video dari postingan berisi beberapa video.
- **Disimpan di thread lain:** file ditulis lewat file sementara lalu di-rename, jadi `library.json` tidak pernah setengah tertulis. Penyimpanan terakhir ditunggu saat aplikasi ditutup.
- **File rusak:** dicadangkan sebagai `library-rusak-<waktu>.json` (tidak dihapus), Pustaka dimulai kosong, dan pesannya tampil di status bar. Jika file tidak bisa dibaca sama sekali, perubahan tidak disimpan supaya file lama tidak tertimpa.
- **Toleran:**
  - nilai yang tidak dikenal (mis. platform baru) diganti nilai bawaan;
  - item yang tidak terbaca ikut disimpan lagi apa adanya;
  - nomor kembar diganti nomor baru.
- **Satu file, satu item:** file baru yang mendapat lokasi sama dengan item lama yang filenya sudah hilang menggantikan item lama itu. Contohnya saat unduh ulang, atau saat ganti nama.

**Saat unduhan selesai**
- yt-dlp diminta mengunduh thumbnail tiap video (`--write-thumbnail`, tanpa thumbnail postingan: `--no-write-playlist-metafiles`).
- DANload mengecilkannya dengan FFmpeg menjadi JPEG berukuran paling besar 320 px di `%LOCALAPPDATA%\DANload\thumbs\` (±4–17 KB).
- Jika thumbnail gagal diambil, unduhan tetap berhasil dan Pustaka menampilkan ikon video/audio.
- **Kualitas yang dicatat adalah hasil sebenarnya dari ffprobe:**
  - video: sisi terpendek gambar, mis. `1080p`;
  - MP3/OGG/M4A/OPUS: bitrate, mis. `192 kbps`;
  - FLAC: `Lossless`; WAV: `Tanpa kompresi`.
- Judul tiap file dicetak yt-dlp sebagai JSON (`%(.{filepath,title})j`). Judul di kartu unduhan juga JSON (`%(title)j`), supaya judul Facebook yang berisi baris baru tidak terpotong.

**Halaman Pustaka**
- **Isi baris:** thumbnail, nama file, lalu `platform · format · kualitas · ukuran · durasi`, dan tanggal (`15 Nov 19:40`; tanggal tahun lalu ditulis dengan tahunnya). Daftar memakai `ListView`, jadi hanya baris yang terlihat yang dibuat.
- **Cari:** lewat kolom di bar atas. Mencari nama file dan judul asli; semua kata harus ada; huruf besar/kecil tidak dibedakan. Mengetik langsung membuka Pustaka.
- **Filter:** chip jenis (Video/Audio) dan platform bisa digabung, mis. Video + YouTube. Klik chip aktif untuk mematikannya; "Semua" mematikan keduanya.
- **Urutan:** Terbaru, Terlama, Nama (A–Z, angka dibaca sebagai bilangan sehingga `(2)` sebelum `(10)`), dan Ukuran terbesar. Jumlah file tampil di samping judul, mis. `3 dari 12 file`.
- **Centang beberapa file:** bar "N file dipilih" (Pilih semua, Hapus..., Batal) muncul di tempat chip, jadi daftar tidak bergeser. Saat diuji, versi pertama bar ini menggeser daftar sehingga klik berikutnya mengenai baris yang salah.

**Panel detail (panel kanan, menggantikan form unduh)**
- **Isi:** thumbnail besar, nama file, dan judul asli (jika berbeda). Nama atau judul yang sangat panjang dipendekkan dengan "…" supaya tombol tetap terlihat. Di bawahnya info Platform, Format, Ukuran, Durasi, Diunduh, dan Lokasi.
- **Tombol:**
  - Buka (aplikasi bawaan Windows);
  - Tampilkan di folder;
  - Ganti nama;
  - Salin link sumber (clipboard lewat Slint);
  - Unduh ulang;
  - Hapus....
- **Ganti nama:**
  - Kolomnya langsung aktif dengan nama lama terpilih. Isinya selalu nama lengkap, bukan versi yang dipendekkan.
  - Diperiksa langsung dengan aturan kolom NAMA FILE.
  - Ekstensi tidak bisa berubah, dan nama yang sudah dipakai ditolak.
  - Nama baru "dipesan" dulu dengan file kosong, jadi file lain tidak pernah tertimpa.
  - Di Windows, mengganti huruf besar/kecil saja tetap bisa.
  - Jika file sedang dipakai program lain, muncul pesan yang jelas.
- **Unduh ulang** mengisi form dengan link, platform, mode, dan format yang sama. Formatnya boleh diganti sebelum menekan Unduh.

**Hapus**
- Dialog konfirmasi selalu muncul, dengan pilihan "Pindahkan ke Recycle Bin", "Hapus dari daftar saja", dan "Batal". Jika semua filenya sudah hilang, yang tersisa hanya "Hapus dari daftar".
- **Recycle Bin:**
  - Memakai `SHFileOperationW` dengan `FOF_ALLOWUNDO` (masuk Recycle Bin, bisa dikembalikan).
  - Ada juga `FOF_WANTNUKEWARNING`: jika file tidak bisa masuk Recycle Bin (mis. drive jaringan), Windows sendiri memperingatkan bahwa file akan terhapus permanen dan pengguna bisa membatalkannya.
  - Dijalankan per file di thread lain. Hasilnya dilaporkan di status bar, mis. "2 file dipindah ke Recycle Bin, 1 gagal (...)".
- Thumbnail item yang dihapus ikut dihapus dari cache.

**File hilang**
- Diperiksa di thread lain saat aplikasi dibuka dan setiap Pustaka dibuka (paling sering sekali per 3 detik).
- **Tampilan:**
  - baris: "File tidak ditemukan" berwarna kuning;
  - panel detail: peringatan;
  - tombol yang tersisa: Salin link sumber, Unduh ulang, dan Hapus dari daftar.
- Ukuran file yang berubah ikut diperbarui.

**Fungsi Windows (`src/system.rs`)**
- Semua kode `unsafe` dikumpulkan di modul `win`.
- COM diaktifkan dulu di thread pemanggil (anjuran Microsoft untuk fungsi Shell).
- Jam lokal memakai zona waktu Windows, termasuk aturan musim panas.
- Di Linux (pengujian) dipakai pengganti:
  - folder sampah `~/.local/share/Trash/files`;
  - `xdg-open`;
  - UTC.

**Status bar**
- Pesan hasil aksi tampil hijau, mis. "Link sumber disalin." dan "Nama diganti menjadi ….". Pesan error tampil merah.
- Pesan hilang sendiri setelah 6 detik, atau 12 detik untuk pesan error.

## Dependensi
- **Ditambah: `windows-sys` 0.61, khusus build Windows** (`[target.'cfg(windows)'.dependencies]`).
  - Fitur: `Win32_Foundation`, `Win32_System_Com`, `Win32_System_Time`, `Win32_UI_Shell`, `Win32_UI_WindowsAndMessaging`.
  - Disetujui pemilik projek, 2026-11-16 (opsi 1A).
- **Tidak ada crate baru yang diunduh:** versi 0.61.2 sudah dipakai rfd, arboard, zip, muda, softbuffer, dan rustls-platform-verifier.
  - `Cargo.lock` hanya bertambah satu baris (`windows-sys` di daftar dependensi `danload`).
  - Fitur yang baru diaktifkan hanya `Win32_System_Time`.

## Cara Menguji

**Sudah dilakukan (Linux, build rilis):**
- `cargo test`: 124 lulus, 4 diabaikan (uji dengan program asli). `cargo clippy --all-targets -- -D warnings` bersih, `cargo fmt --check` OK.
- **Kode khusus Windows dicek tipenya (`cargo clippy -D warnings`) dari Linux.** Caranya: modul `win` dikompilasi di crate terpisah yang memakai `windows-sys` versi dan fitur yang sama. Cek ini terbukti menangkap kesalahan tipe yang sengaja dibuat.
- **Uji dengan yt-dlp 2026.10.26 + FFmpeg asli** memakai data YouTube dan Instagram tiruan:
  - 12 format × (1 video YouTube + 2 video postingan Instagram) berhasil;
  - semua file punya kualitas dan durasi;
  - thumbnail dibuat, kecuali satu item yang link thumbnail-nya sengaja rusak (unduhan tetap berhasil);
  - uji batal dan uji pemilihan format juga lulus.
- **Uji UI di layar virtual:**
  - unduh 4 link (YouTube MP4 & MP3, Facebook, carousel Instagram);
  - daftar dan thumbnail; panel detail (judul Facebook panjang);
  - ganti nama (nama salah lalu benar; file di disk ikut berubah);
  - salin link (ditempel lagi lewat tombol Tempel);
  - cari, filter gabungan, urut nama, centang dua file;
  - hapus dari daftar (file tetap ada, thumbnail terhapus);
  - Recycle Bin (pengganti Linux);
  - file yang dihapus di luar aplikasi ditandai hilang;
  - unduh ulang (item lama yang hilang tergantikan);
  - Buka (aplikasi bawaan terbuka);
  - `library.json` rusak (dicadangkan, pesan tampil).

**Perlu dicoba di Windows:**
1. `cargo build --release`, jalankan `target\release\danload.exe`, lalu unduh 2–3 video/audio.
2. Buka Pustaka. Thumbnail, kualitas, ukuran, durasi, dan **jam lokal** (WIB) harus tampil.
3. Coba cari, chip filter, dan urutan.
4. Di panel detail, coba:
   - Buka (pemutar bawaan terbuka);
   - Tampilkan di folder;
   - Ganti nama: coba `a?b`, nama yang sudah ada, dan ganti huruf besar/kecil saja;
   - Salin link sumber (tempel di browser);
   - Unduh ulang.
5. Hapus:
   - "Hapus dari daftar saja": file tetap ada di folder.
   - "Pindahkan ke Recycle Bin": file muncul di Recycle Bin dan bisa di-Restore.
   - Coba juga hapus beberapa file sekaligus lewat centang.
6. Pindahkan satu file lewat Explorer, buka Pustaka lagi: file itu harus ditandai "File tidak ditemukan".
7. Ukur RAM/CPU di Task Manager (Pustaka berisi beberapa file, diam ±30 detik).

## Hasil Ukur Performa
Diukur di Linux, build rilis. Angkanya RSS dari `/proc`, bukan *private working set* Task Manager, jadi angka Windows bisa berbeda.
- **Pustaka kecil (3 file):** 25,4 MB saat dibuka, 26,2 MB di halaman Pustaka, CPU idle 0%.
- **500 file + 500 thumbnail:**
  - 27,1 MB saat dibuka;
  - 28,4 MB di halaman Pustaka;
  - 34,8 MB setelah menggulir seluruh 500 baris turun dan naik (cache gambar Slint dibatasi 5 MB);
  - CPU idle 0% (0 tick dalam 10 detik).
- Windows: belum diukur.

## Catatan / Yang Perlu Dicek Ulang
- **Belum dijalankan di Windows sama sekali:** Recycle Bin, Buka, dan jam lokal. Tipe dan nama fungsinya sudah dicek, tetapi perilakunya hanya bisa dipastikan di Windows. Ini termasuk peringatan "hapus permanen" untuk drive tanpa Recycle Bin.
- **Hanya unduhan mulai Fase 6** yang masuk Pustaka; file lama tidak diimpor.
- **Thumbnail asli dari YouTube/Facebook/Instagram belum diuji;** pengujian memakai data tiruan. Jika gagal diambil, yang tampil ikon.
- Sudut thumbnail tidak membulat (software renderer Slint tidak memotong gambar mengikuti sudut membulat). Hanya kosmetik.
- Urutan dan filter Pustaka tidak diingat setelah aplikasi ditutup.
- Unduh ulang dari postingan berisi beberapa video mengunduh semua video di postingan itu lagi.
- Chip filter di halaman Unduhan masih belum menyaring (sudah begitu sejak fase sebelumnya); yang berfungsi chip di Pustaka.
