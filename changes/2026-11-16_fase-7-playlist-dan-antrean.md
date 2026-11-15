# Fase 7: Playlist YouTube dan Antrean (Beberapa Unduhan Bersamaan)

- **Tanggal:** 2026-11-16
- **Jenis:** Fitur baru, Perbaikan bug
- **Status:** Sebagian — kode selesai dan diuji di Linux (yt-dlp + FFmpeg asli dengan data tiruan, UI di layar virtual). Belum diuji di Windows, dan belum dicoba dengan playlist YouTube sungguhan.

## Latar Belakang
- Fase 7 roadmap: playlist dan antrean (CLAUDE.md Bagian 2.3 dan 8).
- Keputusan pemilik projek (2026-11-16):
  1. Isi playlist tampil sebagai daftar; video bisa dipilih semua sekaligus atau satu per satu.
  2. Video dari playlist langsung disimpan di folder unduhan biasa (tanpa subfolder playlist). *Asumsi: tanpa nomor urut di depan nama file.*
  3. Paling banyak 3 unduhan berjalan bersamaan.
  4. Antrean diingat walaupun DANload ditutup.
  5. Instagram tetap satu per satu (rekomendasi, disetujui).
  6. Konversi berat (mengubah ulang video) bergiliran satu per satu (rekomendasi, disetujui).
- *Asumsi lain dari rencana yang disetujui:*
  - playlist hanya YouTube; channel tetap ditolak;
  - urutan antrean tidak bisa diubah;
  - unduhan yang terputus diulang dari awal.

## Yang Diubah
| File | Aksi | Keterangan |
|------|------|------------|
| src/queue.rs | Dibuat | Antrean: urutan giliran, batas bersamaan, aturan Instagram satu per satu, jeda, simpan/muat `queue.json` (toleran, file rusak dicadangkan); 7 unit test |
| src/playlist.rs | Dibuat | Membaca isi playlist dengan `yt-dlp --flat-playlist`: kemajuan, batal, video yang tidak bisa diunduh (privat, dihapus, belum tayang, khusus anggota, butuh login, sedang live), pesan error; 5 unit test + 1 uji dengan yt-dlp asli |
| ui/queue.slint | Dibuat | Halaman Antrean, dialog "Pilih video dari playlist", dialog konfirmasi |
| src/app.rs | Diubah | Beberapa unduhan bersamaan, penjadwal antrean, Coba lagi lewat antrean, Batal semua, Jeda/Lanjutkan, alur playlist, simpan antrean, menutup aplikasi dengan rapi, pilihan unduhan bersamaan |
| src/downloader.rs | Diubah | Giliran konversi berat (satu per satu), laporan "menunggu giliran konversi", perbaikan batal di langkah akhir (lihat Perbaikan Bug), folder sementara aman untuk beberapa unduhan; pesan error playlist; 2 unit test baru |
| src/platform.rs | Diubah | Link playlist YouTube diterima (`playlist?list=…`, `watch?list=…`); deteksi link video yang menyebut playlist (`watch?v=…&list=…`, `youtu.be/…?list=…`); `watch_videos` dan channel tetap ditolak dengan pesan baru; 1 unit test baru |
| src/config.rs | Diubah | `max_parallel_downloads` (1–3, bawaan 3) dan lokasi `queue.json`; 1 unit test baru |
| src/formats.rs | Diubah | `OutputFormat` bisa disimpan di JSON (untuk `queue.json`) |
| src/library.rs | Diubah | Pencadangan file JSON rusak dan penyimpan di thread lain dipakai bersama oleh Pustaka dan antrean |
| src/main.rs | Diubah | Modul `queue` dan `playlist` |
| ui/app-window.slint | Diubah | Tombol Unduh tidak terkunci lagi, centang "Unduh seluruh playlist", NAMA FILE otomatis untuk playlist, halaman Antrean, badge di sidebar, status bar `N berjalan · M menunggu` + Batal semua, form bisa digulir |
| ui/downloads.slint | Diubah | Daftar memakai `ListView`; keadaan baru "menunggu giliran konversi"; Coba lagi selalu aktif |
| ui/settings-page.slint | Diubah | Bagian "Unduhan bersamaan" (1, 2, atau 3) |
| ui/components.slint | Diubah | `RowCheck` (dipindah dari library.slint, sekarang bisa dimatikan), badge angka di `NavButton`, `TextLink` |
| ui/library.slint | Diubah | Memakai `RowCheck` dari components.slint |
| README.md | Diubah | Status Fase 7, cara memakai playlist dan antrean, test, struktur |
| CLAUDE.md | Diubah | 2.3, 2.7, 2.10, 7, roadmap |
| CHANGELOG.md | Diubah | Tambah baris ringkasan |

## Detail Perubahan

**Antrean dan unduhan bersamaan**
- Tombol **Unduh** tidak terkunci lagi saat ada unduhan berjalan. Unduhan baru masuk antrean dan langsung dimulai jika ada slot.
- **Batas bersamaan** diatur di Pengaturan → Unduhan bersamaan: 1, 2, atau 3 (bawaan 3, disimpan sebagai `max_parallel_downloads`). Menaikkan batas langsung memulai unduhan yang menunggu.
- **Instagram hanya satu per waktu.** Instagram berikutnya menunggu, tetapi unduhan YouTube/Facebook di belakangnya tetap boleh mulai supaya slot tidak menganggur.
- **Halaman Antrean:** daftar unduhan yang menunggu (nomor urut, judul atau link, platform · format · kualitas), tombol **Hapus** per baris, **Jeda / Lanjutkan**, dan **Kosongkan** (dengan konfirmasi).
  - Jeda: unduhan baru tidak dimulai; yang sedang berjalan tetap diselesaikan.
  - Daftar memakai `ListView`, jadi ratusan baris tetap ringan.
- **Halaman Unduhan:** berisi unduhan yang sedang berjalan dan yang sudah selesai pada sesi ini; sekarang juga memakai `ListView`.
- **Coba lagi:** kartu lama dihapus, unduhannya masuk antrean paling depan (langsung dimulai jika ada slot).
- **Status bar:** `N berjalan · M menunggu` (ditambah "(dijeda)") dan tautan **Batal semua**. Batal semua meminta konfirmasi, lalu menghentikan semua yang berjalan dan mengosongkan antrean. File yang sudah selesai tidak terpengaruh.
- **Badge** jumlah unduhan yang menunggu di ikon Antrean (lebih dari 99 ditulis `99+`).
- **Petunjuk di bawah tombol Unduh:** "Slot penuh: unduhan baru masuk Antrean dulu.", "Antrean dijeda: …", atau "Instagram satu per satu: …".
- **Pindah halaman otomatis:** jika unduhan yang ditambahkan langsung berjalan, halaman pindah ke Unduhan; jika semuanya menunggu, ke Antrean, dengan pesan di status bar.
- Format, kualitas, folder, dan nama file dicatat saat unduhan ditambahkan. Mengubah pengaturan sesudahnya tidak mengubah unduhan yang sudah ada di antrean.

**Antrean diingat (`%APPDATA%\DANload\queue.json`)**
- Disimpan setiap ada perubahan, di thread lain, lewat file sementara lalu rename (penyimpan yang sama dengan Pustaka).
- Isinya: unduhan yang sedang berjalan (paling depan), lalu yang menunggu, plus status jeda. Unduhan yang sudah selesai, gagal, atau dibatalkan pengguna tidak disimpan.
- **Menutup jendela saat ada unduhan berjalan:** muncul pertanyaan "N unduhan masih berjalan. Hentikan dan tutup DANload? …". Jika Ya:
  1. isi antrean dibekukan dulu, jadi unduhan yang dihentikan tetap tercatat;
  2. semua unduhan dan pembacaan playlist dihentikan bersamaan, paling lama 5 detik;
  3. folder sementaranya dibersihkan.
- **Saat dibuka lagi:** pesan "N unduhan dari sesi sebelumnya ada di Antrean dan akan dilanjutkan." Antrean berjalan otomatis setelah pemeriksaan komponen selesai (kecuali dijeda). Unduhan yang terputus diulang dari awal.
- **File rusak** dicadangkan sebagai `queue-rusak-<waktu>.json` (tidak dihapus), dan antrean dimulai kosong. Satu unduhan yang tidak terbaca (mis. format dari versi DANload yang lebih baru) dilewati, yang lain tetap dimuat.

**Playlist YouTube**
- **Link yang diterima:**
  - `youtube.com/playlist?list=…` (juga `m.` dan `music.youtube.com`);
  - `watch?list=…` tanpa video.
  Pesannya: "Link playlist. Tekan Unduh untuk memilih video."
- **Link video yang menyebut playlist** (`watch?v=…&list=…`, `youtu.be/…?list=…`):
  - bawaannya hanya videonya yang diunduh, seperti sebelumnya;
  - di bawah kolom URL muncul centang **Unduh seluruh playlist**. Centang ini kembali kosong setiap link diganti.
- **Membaca isi:** tekan Unduh. Dialog "Membaca playlist" tampil dengan jumlah video yang sudah terbaca dan tombol Batal (prosesnya dihentikan).
  - DANload menjalankan `yt-dlp --flat-playlist --yes-playlist` dengan `--print` khusus: hanya daftar videonya yang dibaca, tanpa membuka tiap video.
- **Dialog "Pilih video dari playlist":**
  - **Isi:** judul playlist, lalu daftar video bernomor dengan durasinya.
  - **Centang:** semua video yang bisa diunduh langsung dicentang. Klik baris atau kotak centang untuk mengubahnya; ada juga **Pilih semua**, **Kosongkan**, dan jumlah "N dari M video dipilih".
  - **Video yang tidak bisa diunduh** tampil redup dengan alasannya dan tidak bisa dicentang. Alasannya: Video privat, Video sudah dihapus, Video tidak tersedia, Khusus anggota atau Premium, Butuh login, Belum tayang, Sedang siaran langsung.
  - **Tambah N video ke antrean:** tiap video menjadi unduhan sendiri dengan format dari panel kanan, disimpan langsung di folder unduhan.
- Saat mode playlist, kolom NAMA FILE diganti tulisan "Otomatis dari judul tiap video" (isian lama tetap tersimpan).
- **Jika gagal dibaca:** dialog menampilkan alasannya, mis. "Playlist ini kosong, atau semua videonya disembunyikan YouTube." atau "Playlist tidak ditemukan atau tidak bisa dibuka…". Jika hanya sebagian yang terbaca, daftar tetap bisa dipilih dengan keterangan "Isi playlist mungkin belum lengkap".
- `watch_videos?video_ids=…` dan channel tetap ditolak dengan pesan yang menjelaskan.

**Konversi berat bergiliran**
- Langkah konversi yang mengubah ulang video (mis. ke AVI/MPEG, atau WEBM/MOV yang kodeknya tidak cocok) hanya berjalan satu per satu.
- Unduhan lain yang sampai di langkah ini menunggu dengan keterangan "Menunggu giliran konversi (unduhan lain sedang dikonversi)..." (ikon antrean, tanpa batang kemajuan). Tombol Batal tetap berfungsi saat menunggu.
- Langkah yang hanya menyalin stream tidak perlu menunggu.

**Perbaikan bug (ditemukan saat menguji Batal semua)**
- **Masalah lama (sejak Fase 6):** jika pembatalan datang sesudah file dipindah ke folder unduhan (saat ffprobe atau thumbnail), kartunya bertuliskan "Dibatalkan", padahal filenya sudah utuh di folder. File itu tidak tercatat di Pustaka, dan thumbnail-nya tertinggal tanpa pemilik.
- **Sekarang:** file yang sudah tersimpan selalu dilaporkan selesai dan tercatat di Pustaka. Untuk postingan berisi beberapa video, sisanya tidak diproses, dengan keterangan "Dibatalkan setelah 1 dari 2 file tersimpan."

**Lain-lain**
- **Form panel kanan bisa digulir.** Di layar virtual, pesan dua baris membuat jendela membesar sendiri beberapa piksel; sekarang isinya tetap di dalam panel.
- **Folder sementara:** pembuatan foldernya dicoba sekali lagi jika `.danload-tmp` kebetulan baru dihapus oleh unduhan lain yang selesai bersamaan.

## Dependensi
- Tidak ada yang ditambah atau diubah (`Cargo.toml` dan `Cargo.lock` tidak berubah).

## Cara Menguji

**Sudah dilakukan (Linux, build rilis):**
- **Pemeriksaan otomatis:**
  - `cargo test`: 140 lulus, 5 diabaikan (uji dengan program asli);
  - `cargo clippy --all-targets -- -D warnings` bersih, `cargo fmt --check` OK;
  - cek tipe kode khusus Windows (`src/system.rs`) lulus.
- **Uji dengan yt-dlp 2026.10.26 + FFmpeg asli** (`cargo test real_ -- --ignored`, 4 lulus):
  - 12 format end-to-end;
  - batal di tengah konversi;
  - pemilihan format;
  - membaca playlist tiruan (6 video: 3 bisa diunduh, 1 privat, 1 dihapus, 1 belum tayang) lalu batal di tengah pembacaan;
  - playlist 500 video terbaca dengan laporan kemajuan bertahap.
- **Uji UI di layar virtual** (server lokal yang sengaja dilambatkan ±350 KB/detik):
  - **Playlist:**
    - playlist 8 video, 2 dilepas centangnya: 3 berjalan, 3 menunggu, lalu sisanya mulai otomatis begitu slot kosong (selisih ±15,7 detik, sesuai kecepatan server);
    - playlist dengan video privat/dihapus/belum tayang (baris itu tidak bisa dicentang);
    - playlist kosong (pesan gagal);
    - link `watch?v=…&list=…` (tanpa centang: videonya saja; dengan centang: dialog playlist);
    - batal saat membaca playlist (prosesnya berhenti).
  - **Aturan antrean:**
    - Instagram kedua menunggu, sementara YouTube yang ditambahkan sesudahnya langsung berjalan;
    - 3 unduhan AVI: 1 mengonversi, 2 "Menunggu giliran konversi", hasil akhirnya mpeg4 + mp3;
    - Pengaturan 1 unduhan: 1 berjalan, 7 menunggu.
  - **Tombol:** Jeda/Lanjutkan, Hapus per baris, Kosongkan (konfirmasi: Kembali dan Kosongkan antrean), Batal semua (konfirmasi: Kembali lalu Batalkan semua), Coba lagi.
  - **Menutup dan membuka lagi:**
    - tutup jendela (jawaban Tidak): tetap terbuka;
    - DANload dihentikan paksa lalu dibuka lagi: antrean pulih dan unduhan yang terputus diulang;
    - tutup jendela (jawaban Ya): `queue.json` berisi unduhan yang sedang berjalan di urutan pertama, tidak ada yt-dlp tertinggal, dan folder sementara bersih;
    - menutup saat membaca playlist: proses baca ikut berhenti.
  - Pertanyaan saat menutup jendela diuji dengan `zenity` tiruan (dialog bawaan Linux tidak tersedia di layar virtual).
  - Antrean 500 video berjalan 3 bersamaan selama 90 detik (±155 unduhan), lalu Batal semua: `queue.json` kosong, tidak ada proses tertinggal.

**Perlu dicoba di Windows:**
1. `cargo build --release`, jalankan `target\release\danload.exe`.
2. Tempel link playlist YouTube sungguhan (yang pendek, mis. 5–10 video), tekan Unduh, lepas centang beberapa video, lalu Tambah ke antrean. Pastikan 3 berjalan dan sisanya menunggu di Antrean.
3. Coba link `watch?v=…&list=…` (dari video yang dibuka di dalam playlist), dengan dan tanpa centang "Unduh seluruh playlist".
4. Tambah 2 link Instagram dan 1 link YouTube: Instagram kedua harus menunggu, YouTube langsung berjalan.
5. Pilih format AVI lalu unduh 3 video sekaligus: hanya 1 yang mengonversi, yang lain "Menunggu giliran konversi".
6. Coba di halaman Antrean: Jeda, Lanjutkan, Hapus, Kosongkan. Lalu coba Batal semua di status bar.
7. Tutup jendela saat ada unduhan berjalan (jawab **Yes**), buka lagi: unduhan harus dilanjutkan otomatis.
8. Pengaturan → Unduhan bersamaan → 1 unduhan, lalu tambah beberapa video.
9. Ukur RAM/CPU di Task Manager: saat diam, dan saat 3 unduhan berjalan.

## Hasil Ukur Performa
Diukur di Linux, build rilis. Angkanya RSS dari `/proc`, bukan *private working set* Task Manager, jadi angka Windows bisa berbeda. CPU di bawah ini hanya proses DANload (yt-dlp dan FFmpeg proses terpisah).
- **Diam setelah dibuka:** 23 MB, CPU 0%, 5 thread.
- **Dialog playlist 500 video:** 24 MB, juga setelah digulir; CPU 0,2%.
- **Antrean 500 video (dijeda):** 24 MB, juga setelah digulir; CPU 0,2%.
- **3 unduhan bersamaan menghabiskan antrean 500 video** (server lokal cepat, ±155 unduhan dalam 90 detik): 28–29 MB. CPU DANload 1,6% di halaman Antrean dan 2,6% di halaman Unduhan.
- **Setelah Batal semua:** 28 MB, kembali 5 thread, CPU 0,2%.
- Windows: belum diukur.

## Catatan / Yang Perlu Dicek Ulang
- **Belum diuji di Windows, dan belum dengan playlist YouTube sungguhan.** Yang perlu dipastikan:
  - isi playlist asli (termasuk `youtu.be/…?list=…`, YouTube Music, dan playlist Mix `list=RD…`) terbaca dengan benar;
  - judul pengganti untuk video privat/dihapus sama dengan yang dikenali DANload.
- **3 unduhan YouTube bersamaan** mungkin lebih cepat memicu verifikasi "bukan bot" dari YouTube. Jika sering terjadi, turunkan ke 1 atau 2 di Pengaturan.
- **Jika DANload dihentikan paksa** (Task Manager, crash, Windows dimatikan saat mengunduh):
  - unduhan yang terputus tetap diulang saat dibuka lagi;
  - tetapi folder `.danload-tmp\<nomor>` milik sesi lama tertinggal di folder unduhan. Ini sudah begitu sejak Fase 3. Membersihkannya otomatis saat DANload dibuka butuh keputusan pemilik projek, karena berarti menghapus file.
- Jika DANload ditutup tepat saat sebuah file sedang dipindah di langkah akhir, video itu bisa terunduh lagi saat DANload dibuka (sangat jarang).
- **Antrean:**
  - link yang sama bisa masuk antrean dua kali; hasil keduanya menjadi `Judul (2).ext`;
  - urutan antrean tidak bisa diubah.
- Jumlah di dialog konfirmasi "Kosongkan antrean?" dihitung saat tombol ditekan. Jika ada unduhan yang mulai sementara dialog terbuka, angkanya bisa lebih besar dari kenyataan (tidak berbahaya).
- Chip filter di halaman Unduhan masih belum menyaring (sejak fase sebelumnya).
