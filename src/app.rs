//! Penghubung antara tampilan (Slint) dan logika (config, deps, setup, downloader, library,
//! queue, playlist).
//!
//! Aturan penting: komponen UI hanya boleh disentuh dari UI thread.
//! Pekerjaan lambat (menjalankan yt-dlp/FFmpeg, mengunduh) dikerjakan di thread lain,
//! lalu hasilnya dikirim balik lewat `upgrade_in_event_loop`.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use slint::{
    CloseRequestResponse, ComponentHandle, Image, Model, ModelNotify, ModelRc, ModelTracker,
    SharedString, Timer, VecModel, Weak,
};

use crate::config::{self, Config, DownloadMode};
use crate::convert;
use crate::deps::{self, Component, Status};
use crate::downloader::{self, CancelHandle, Item, Outcome, SavedFile, Tools, Update};
use crate::filename;
use crate::formats::{AudioFormat, OutputFormat, QualityChoice, QualityMenu, VideoFormat};
use crate::library::{self, KindFilter, Library, Query, Saver, SortOrder};
use crate::platform::{self, Platform, UrlCheck};
use crate::playlist;
use crate::process;
use crate::queue::{self, Job, Queue};
use crate::setup::{self, InstallError, Package, Progress};
use crate::system;
use crate::{
    AppWindow, ComponentInfo, ComponentState, ConfirmDialog, DeleteDialog, DownloadItem,
    DownloadState, LibraryDetail, LibraryRow, PlaylistDialog, PlaylistEntry, QueueRow, UrlFeedback,
    WizardState,
};

/// Batas waktu menunggu semua unduhan berhenti saat aplikasi ditutup.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Jeda minimal antar pemeriksaan file hilang di Pustaka.
const FILE_CHECK_INTERVAL: Duration = Duration::from_secs(3);

/// Panjang maksimum (karakter) nama, judul, dan folder di panel detail Pustaka; yang lebih panjang
/// dipendekkan dengan "…" supaya tombol aksi tetap terlihat (kira-kira dua baris).
const DETAIL_NAME_CHARS: usize = 60;
const DETAIL_TITLE_CHARS: usize = 90;
const DETAIL_FOLDER_CHARS: usize = 56;

/// Lama pesan di status bar tampil sebelum hilang sendiri.
const STATUS_DURATION: Duration = Duration::from_secs(6);
const STATUS_ERROR_DURATION: Duration = Duration::from_secs(12);

/// Data yang dipakai bersama oleh semua penangan aksi di UI thread.
///
/// `RefCell`/`Cell` dipakai karena closure Slint hanya menerima referensi biasa,
/// sementara isinya perlu diubah. Aman, karena semuanya hanya diakses dari UI thread.
struct AppState {
    config: RefCell<Config>,
    config_path: PathBuf,
    /// Nomor pemasangan yang sedang berlaku. Laporan dari pemasangan lama
    /// (mis. yang sudah dibatalkan) diabaikan karena nomornya tidak cocok.
    install_job: Cell<u64>,
    /// Tanda batal untuk pemasangan yang sedang berjalan.
    install_cancel: RefCell<Arc<AtomicBool>>,
    /// `true` setelah hasil pemeriksaan komponen pertama ditangani.
    first_check_handled: Cell<bool>,
    /// Lokasi tiap komponen yang ditemukan, urutannya sama dengan `Component::ALL`.
    component_paths: RefCell<Vec<Option<PathBuf>>>,
    /// Kartu unduhan yang tampil di UI (terbaru di indeks 0).
    download_model: Rc<VecModel<DownloadItem>>,
    /// Data di balik tiap kartu unduhan.
    download_entries: RefCell<Vec<DownloadEntry>>,
    /// Unduhan yang sedang berjalan. Sejak Fase 7 bisa beberapa sekaligus (lihat [`schedule`]).
    active_downloads: RefCell<Vec<ActiveDownload>>,
    next_download_id: Cell<i32>,
    /// Pustaka (sejak Fase 6).
    library: LibraryState,
    /// Antrean dan playlist (sejak Fase 7).
    queue: QueueState,
}

/// Data antrean dan playlist milik UI thread.
struct QueueState {
    /// Unduhan yang menunggu giliran.
    queue: RefCell<Queue>,
    /// Penyimpan `queue.json` di thread lain. Dibuang (dan ditunggu) saat aplikasi ditutup.
    saver: RefCell<Option<Saver>>,
    /// Baris halaman Antrean.
    model: Rc<VecModel<QueueRow>>,
    /// `true` setelah DANload mulai ditutup: `queue.json` tidak diubah lagi, supaya unduhan yang
    /// dihentikan karena aplikasi ditutup tetap tercatat (lihat [`freeze_queue`]).
    frozen: Cell<bool>,
    /// Playlist yang sedang dibaca, atau isinya yang menunggu dipilih di dialog.
    playlist: RefCell<Option<PlaylistPick>>,
    /// Nomor pembacaan playlist terakhir.
    next_playlist: Cell<u64>,
    /// Baris dialog pilih video.
    playlist_model: Rc<VecModel<PlaylistEntry>>,
    /// Aksi yang menunggu jawaban dialog konfirmasi.
    pending_confirm: Cell<Option<ConfirmAction>>,
}

/// Playlist yang sedang dibaca, atau isinya yang menunggu dipilih pengguna.
struct PlaylistPick {
    /// Nomor pembacaan; laporan dari pembacaan yang sudah ditutup diabaikan.
    number: u64,
    cancel: CancelHandle,
    /// Format dan folder saat tombol Unduh ditekan.
    format: OutputFormat,
    folder: PathBuf,
    entries: Vec<playlist::Entry>,
    /// Centang tiap video, sejajar dengan `entries`.
    checked: Vec<bool>,
}

/// Data Pustaka milik UI thread.
struct LibraryState {
    library: RefCell<Library>,
    /// Penyimpan `library.json` di thread lain. Dibuang (dan ditunggu) saat aplikasi ditutup.
    saver: RefCell<Option<Saver>>,
    /// Folder thumbnail (`%LOCALAPPDATA%\DANload\thumbs`).
    thumbnails: Option<PathBuf>,
    /// Baris yang tampil di daftar.
    model: Rc<LibraryModel>,
    /// Pencarian, filter, dan urutan yang berlaku.
    query: RefCell<Query>,
    /// File yang dibuka di panel detail.
    selected: Cell<Option<u64>>,
    /// File yang dicentang.
    checked: RefCell<HashSet<u64>>,
    /// File yang sudah tidak ada di lokasinya (hasil pemeriksaan terakhir).
    missing: RefCell<HashSet<u64>>,
    /// File yang menunggu jawaban dialog konfirmasi hapus.
    pending_delete: RefCell<Vec<u64>>,
    /// Waktu pemeriksaan file hilang terakhir.
    last_check: Cell<Option<Instant>>,
    /// Nomor pesan status terakhir, supaya timer pesan lama tidak menghapus pesan baru.
    message_number: Cell<u64>,
}

/// Data satu kartu unduhan yang perlu diingat (untuk "Coba lagi", "Tampilkan di folder", dan
/// Pustaka).
struct DownloadEntry {
    id: i32,
    /// Unduhan yang diminta (link, platform, format, folder, nama file).
    job: Job,
    /// Lokasi file hasil, setelah berhasil (satu link bisa menghasilkan beberapa file).
    files: Vec<PathBuf>,
}

/// Unduhan yang sedang berjalan.
struct ActiveDownload {
    id: i32,
    /// Disimpan di `queue.json` selama berjalan, supaya diulang jika DANload ditutup di tengah
    /// unduhan.
    job: Job,
    cancel: CancelHandle,
    /// `true` setelah tombol Batal ditekan; laporan kemajuan berikutnya diabaikan.
    cancel_requested: bool,
    /// Video yang sedang diproses, untuk link berisi beberapa video.
    item: Option<Item>,
}

thread_local! {
    /// `AppState` milik UI thread.
    ///
    /// Thread pekerja tidak boleh memegang `Rc<AppState>`. Jadi saat hasilnya tiba di
    /// UI thread (lewat `upgrade_in_event_loop`), state diambil lagi dari sini.
    static STATE: RefCell<Option<Rc<AppState>>> = const { RefCell::new(None) };
}

/// Mengambil `AppState`. Hanya berisi nilai jika dipanggil dari UI thread setelah [`setup`].
fn current_state() -> Option<Rc<AppState>> {
    STATE.with(|slot| slot.borrow().clone())
}

/// Mengisi data awal ke jendela dan memasang semua penangan aksi pengguna.
pub fn setup(app: &AppWindow) {
    let config_path = config::config_file_path();
    let library_path = config::library_file_path();
    let (library, library_message) = Library::load(&library_path);
    let queue_path = config::queue_file_path();
    let (saved_queue, queue_message) = queue::load(&queue_path);
    let state = Rc::new(AppState {
        config: RefCell::new(Config::load_from(&config_path)),
        config_path,
        install_job: Cell::new(0),
        install_cancel: RefCell::new(Arc::new(AtomicBool::new(false))),
        first_check_handled: Cell::new(false),
        component_paths: RefCell::new(vec![None; Component::ALL.len()]),
        download_model: Rc::new(VecModel::default()),
        download_entries: RefCell::new(Vec::new()),
        active_downloads: RefCell::new(Vec::new()),
        next_download_id: Cell::new(1),
        library: LibraryState {
            library: RefCell::new(library),
            saver: RefCell::new(None),
            thumbnails: library::thumbnail_dir(),
            model: Rc::new(LibraryModel::default()),
            query: RefCell::new(Query::default()),
            selected: Cell::new(None),
            checked: RefCell::new(HashSet::new()),
            missing: RefCell::new(HashSet::new()),
            pending_delete: RefCell::new(Vec::new()),
            last_check: Cell::new(None),
            message_number: Cell::new(0),
        },
        queue: QueueState {
            queue: RefCell::new(saved_queue),
            saver: RefCell::new(None),
            model: Rc::new(VecModel::default()),
            frozen: Cell::new(false),
            playlist: RefCell::new(None),
            next_playlist: Cell::new(0),
            playlist_model: Rc::new(VecModel::default()),
            pending_confirm: Cell::new(None),
        },
    });
    STATE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&state)));

    // Pustaka dan antrean disimpan di thread lain; kalau gagal, pesannya tampil di status bar.
    let saver = |path: PathBuf, what: &'static str| {
        let weak = app.as_weak();
        Saver::start(path, move |error| {
            let message = format!("{what} gagal disimpan: {error}");
            let _ = weak.upgrade_in_event_loop(move |app| app.set_notice(message.into()));
        })
    };
    *state.library.saver.borrow_mut() = Some(saver(library_path, "Data Pustaka"));
    *state.queue.saver.borrow_mut() = Some(saver(queue_path, "Antrean"));

    {
        let config = state.config.borrow();
        // Nomor versi diambil dari Cargo.toml, jadi cukup diubah di satu tempat.
        app.set_app_version(env!("CARGO_PKG_VERSION").into());
        app.set_download_mode(config.download_mode.index());
        app.set_separate_subfolders(config.separate_subfolders);
        show_download_folder(app, &config.effective_download_folder());
        show_format_controls(app, &config);
        app.set_downloads(ModelRc::from(Rc::clone(&state.download_model)));

        // Belum pernah menyelesaikan setup: langsung tampilkan wizard dari awal.
        if !config.setup_completed {
            let wizard = app.global::<WizardState>();
            wizard.set_step(0);
            wizard.set_visible(true);
        }
    }

    // Daftar komponen; awalnya semua berstatus "sedang diperiksa".
    let rows: Vec<ComponentInfo> = Component::ALL.iter().map(|c| checking_row(*c)).collect();
    app.set_components(ModelRc::from(Rc::new(VecModel::from(rows))));

    connect_main_window(app, &state);
    connect_downloads(app, &state);
    connect_queue(app, &state);
    connect_library(app, &state);
    connect_wizard(app, &state);

    app.set_queue_rows(ModelRc::from(Rc::clone(&state.queue.model)));
    app.set_playlist_entries(ModelRc::from(Rc::clone(&state.queue.playlist_model)));
    show_parallel(app, &state);
    show_queue(app, &state);
    show_running(app, &state);

    app.set_library_rows(ModelRc::from(
        Rc::clone(&state.library.model) as Rc<dyn Model<Data = LibraryRow>>
    ));
    app.set_library_sort_options(string_model(
        SortOrder::ALL
            .iter()
            .map(|sort| sort.label().to_string())
            .collect(),
    ));
    refresh_library(app, &state);
    if let Some(message) = library_message {
        show_status(app, &state, message, true);
    }
    // Antrean dari sesi sebelumnya dilanjutkan setelah pemeriksaan komponen selesai.
    let restored = state.queue.queue.borrow().len();
    if let Some(message) = queue_message {
        show_status(app, &state, message, true);
    } else if restored > 0 {
        let message = if state.queue.queue.borrow().is_paused() {
            format!("{restored} unduhan dari sesi sebelumnya ada di Antrean (dijeda).")
        } else {
            format!("{restored} unduhan dari sesi sebelumnya ada di Antrean dan akan dilanjutkan.")
        };
        show_status(app, &state, message, false);
    }
    check_library_files(app, &state);

    start_component_check(app);
}

/// Dipanggil setelah jendela ditutup: pastikan tidak ada yt-dlp/FFmpeg yang tertinggal, dan
/// antrean terakhir tersimpan.
pub fn shutdown() {
    let Some(state) = current_state() else { return };
    stop_everything(&state);
    // Menunggu penyimpanan Pustaka dan antrean terakhir selesai (lihat `Saver`).
    drop(state.library.saver.borrow_mut().take());
    drop(state.queue.saver.borrow_mut().take());
}

/// Penangan aksi di jendela utama dan halaman Pengaturan.
fn connect_main_window(app: &AppWindow, state: &Rc<AppState>) {
    // Platform yang dipilih selalu dimulai dari YouTube (bawaan, lihat CLAUDE.md 2.3).
    app.set_platform(0);

    app.on_check_url(|platform_index, url| {
        let selected = Platform::from_index(platform_index);
        let (valid, message) = platform::feedback(selected, &url);
        UrlFeedback {
            valid,
            message: message.into(),
            playlist: platform::check(selected, &url) == UrlCheck::Playlist,
            in_playlist: valid && platform::video_in_playlist(&url),
        }
    });

    // Pesan untuk kolom NAMA FILE (kosong = nama boleh dipakai).
    app.on_check_file_name(|name, extension| {
        match filename::check_custom_name(&name, &extension) {
            Ok(_) => SharedString::new(),
            Err(message) => message.into(),
        }
    });

    // Keterangan format bergantung pada platform (mis. WEBM dari Instagram perlu diubah ulang).
    app.on_platform_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |_index| {
            if let Some(app) = weak.upgrade() {
                show_format_controls(&app, &state.config.borrow());
            }
        }
    });

    app.on_recheck_components({
        let weak = app.as_weak();
        move || {
            if let Some(app) = weak.upgrade() {
                start_component_check(&app);
            }
        }
    });

    app.on_download_mode_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |index| {
            state.config.borrow_mut().download_mode = DownloadMode::from_index(index);
            if let Some(app) = weak.upgrade() {
                show_format_controls(&app, &state.config.borrow());
            }
            save_config(&weak, &state);
        }
    });

    app.on_format_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |index| {
            {
                let mut config = state.config.borrow_mut();
                match config.download_mode {
                    DownloadMode::Video => config.video_format = VideoFormat::from_index(index),
                    DownloadMode::Audio => config.audio_format = AudioFormat::from_index(index),
                }
            }
            if let Some(app) = weak.upgrade() {
                show_format_controls(&app, &state.config.borrow());
            }
            save_config(&weak, &state);
        }
    });

    app.on_quality_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |index| {
            {
                let mut config = state.config.borrow_mut();
                let format = config.output_format(config.download_mode);
                match QualityMenu::for_format(format).choice(index) {
                    Some(QualityChoice::MaxHeight(height)) => config.video_max_height = height,
                    Some(QualityChoice::Bitrate(kbps)) => config.audio_bitrate_kbps = kbps,
                    None => return, // Kualitas format ini tidak bisa dipilih.
                }
            }
            if let Some(app) = weak.upgrade() {
                show_format_controls(&app, &state.config.borrow());
            }
            save_config(&weak, &state);
        }
    });

    app.on_subfolders_toggled({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |checked| {
            state.config.borrow_mut().separate_subfolders = checked;
            save_config(&weak, &state);
        }
    });

    app.on_choose_download_folder({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            let Some(app) = weak.upgrade() else { return };
            // Ambil folder awal dulu, supaya config tidak sedang "dipinjam"
            // selama dialog terbuka.
            let current = state.config.borrow().effective_download_folder();
            let Some(chosen) = pick_folder(&app, &current) else {
                return; // Pengguna menekan Batal.
            };
            state.config.borrow_mut().download_folder = Some(chosen.clone());
            show_download_folder(&app, &chosen);
            save_config(&weak, &state);
        }
    });
}

/// Penangan aksi di wizard setup.
fn connect_wizard(app: &AppWindow, state: &Rc<AppState>) {
    let wizard = app.global::<WizardState>();

    wizard.on_install({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |all| {
            let Some(app) = weak.upgrade() else { return };
            let packages = if all {
                Package::ALL.to_vec()
            } else {
                packages_not_ready(&app)
            };
            start_install(&app, &state, InstallRequest::Download(packages));
        }
    });

    wizard.on_cancel_install({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            let Some(app) = weak.upgrade() else { return };
            // Beri tahu thread pekerja untuk berhenti, lalu abaikan laporan berikutnya darinya.
            state.install_cancel.borrow().store(true, Ordering::Relaxed);
            state.install_job.set(state.install_job.get() + 1);
            let wizard = app.global::<WizardState>();
            wizard.set_installing(false);
            wizard.set_error("".into());
        }
    });

    wizard.on_pick_file({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            let Some(app) = weak.upgrade() else { return };
            let Some(file) = pick_program_file(&app) else {
                return; // Pengguna menekan Batal.
            };
            start_install(&app, &state, InstallRequest::FromFile(file));
        }
    });

    wizard.on_finish({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            let Some(app) = weak.upgrade() else { return };
            state.config.borrow_mut().setup_completed = true;
            save_config(&weak, &state);
            app.global::<WizardState>().set_visible(false);
        }
    });
}

// ---------------------------------------------------------------------------
// Pemeriksaan komponen
// ---------------------------------------------------------------------------

/// Memeriksa semua komponen di thread terpisah, satu thread per komponen.
fn start_component_check(app: &AppWindow) {
    if app.get_checking_components() {
        return; // Pemeriksaan sebelumnya belum selesai.
    }
    app.set_checking_components(true);
    app.set_components_ready(false);

    let model = app.get_components();
    for (index, component) in Component::ALL.iter().enumerate() {
        model.set_row_data(index, checking_row(*component));
    }

    // Daftar folder dihitung sekali, lalu dibagi ke semua thread.
    let dirs = Arc::new(deps::search_dirs());
    for (index, component) in Component::ALL.into_iter().enumerate() {
        let weak = app.as_weak();
        let dirs = Arc::clone(&dirs);
        std::thread::spawn(move || {
            let status = deps::check_in(component, &dirs);
            // Kembali ke UI thread untuk menampilkan hasilnya.
            // Kalau jendela sudah ditutup, hasilnya dibuang saja.
            let _ = weak.upgrade_in_event_loop(move |app| {
                apply_status(&app, index, component, &status);
            });
        });
    }
}

/// Menampilkan hasil pemeriksaan satu komponen, lalu memperbarui ringkasannya.
fn apply_status(app: &AppWindow, index: usize, component: Component, status: &Status) {
    let model = app.get_components();
    model.set_row_data(index, status_row(component, status));
    if let Some(state) = current_state()
        && let Some(slot) = state.component_paths.borrow_mut().get_mut(index)
    {
        *slot = match status {
            Status::Found { path, .. } => Some(path.clone()),
            Status::Broken { .. } | Status::Missing => None,
        };
    }

    let states: Vec<ComponentState> = model.iter().map(|row| row.state).collect();
    let still_checking = states.contains(&ComponentState::Checking);
    app.set_components_ready(states.iter().all(|state| *state == ComponentState::Found));
    app.set_checking_components(still_checking);

    if !still_checking && let Some(state) = current_state() {
        after_first_check(app, &state);
        // Komponen siap: antrean (termasuk dari sesi sebelumnya) bisa berjalan.
        schedule(app, &state);
    }
}

/// Setelah pemeriksaan pertama sejak aplikasi dibuka: kalau setup sudah pernah selesai
/// tetapi ada komponen yang hilang atau rusak, buka wizard langsung di langkah Komponen.
fn after_first_check(app: &AppWindow, state: &AppState) {
    if state.first_check_handled.replace(true) {
        return; // Hanya untuk pemeriksaan pertama.
    }
    let wizard = app.global::<WizardState>();
    if state.config.borrow().setup_completed && !app.get_components_ready() && !wizard.get_visible()
    {
        wizard.set_step(1);
        wizard.set_visible(true);
    }
}

/// Paket yang punya program belum siap (hilang atau bermasalah).
fn packages_not_ready(app: &AppWindow) -> Vec<Package> {
    let model = app.get_components();
    Package::ALL
        .into_iter()
        .filter(|package| {
            package.components().iter().any(|component| {
                Component::ALL
                    .iter()
                    .position(|c| c == component)
                    .and_then(|index| model.row_data(index))
                    .is_none_or(|row| row.state != ComponentState::Found)
            })
        })
        .collect()
}

/// Baris komponen saat masih diperiksa.
fn checking_row(component: Component) -> ComponentInfo {
    ComponentInfo {
        name: component.display_name().into(),
        purpose: component.purpose().into(),
        state: ComponentState::Checking,
        detail: "".into(),
        location: "".into(),
    }
}

/// Baris komponen sesuai hasil pemeriksaan.
fn status_row(component: Component, status: &Status) -> ComponentInfo {
    let (state, detail, location) = match status {
        Status::Found { path, version } => (
            ComponentState::Found,
            format!("Versi {version}"),
            path.display().to_string(),
        ),
        Status::Broken { path, reason } => (
            ComponentState::Broken,
            reason.clone(),
            path.display().to_string(),
        ),
        Status::Missing => (
            ComponentState::Missing,
            "Tidak ditemukan di komputer ini.".to_string(),
            String::new(),
        ),
    };
    ComponentInfo {
        name: component.display_name().into(),
        purpose: component.purpose().into(),
        state,
        detail: detail.into(),
        location: location.into(),
    }
}

// ---------------------------------------------------------------------------
// Unduhan
// ---------------------------------------------------------------------------

/// Penangan aksi unduhan: tombol Unduh, Batal, Coba lagi, Tampilkan di folder, Batal semua,
/// dan konfirmasi saat jendela ditutup di tengah unduhan.
fn connect_downloads(app: &AppWindow, state: &Rc<AppState>) {
    app.on_start_download(with_app(app, state, start_download));
    app.on_cancel_download({
        let state = Rc::clone(state);
        move |id| cancel_download(&state, id)
    });
    app.on_retry_download(with_app_arg(app, state, retry_download));
    app.on_cancel_all(with_app(app, state, |app, state| {
        ask_confirm(app, state, ConfirmAction::CancelAll);
    }));

    app.on_show_in_folder({
        let state = Rc::clone(state);
        move |id| {
            let entries = state.download_entries.borrow();
            if let Some(entry) = entries.iter().find(|entry| entry.id == id) {
                // Beberapa file: Explorer dibuka dengan file pertama terpilih.
                let target = entry.files.first().unwrap_or(&entry.job.folder);
                process::show_in_file_manager(target);
            }
        }
    });

    // Menutup jendela saat unduhan berjalan: tanya dulu, lalu hentikan unduhannya dengan rapi.
    // Unduhan yang dihentikan karena DANload ditutup tetap tercatat di antrean, lalu diulang dari
    // awal saat DANload dibuka lagi.
    app.window().on_close_requested({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            let running = state.active_downloads.borrow().len();
            if running == 0 {
                return CloseRequestResponse::HideWindow;
            }
            let Some(app) = weak.upgrade() else {
                return CloseRequestResponse::HideWindow;
            };
            let handle = app.window().window_handle();
            let answer = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title("DANload")
                .set_description(close_question(running))
                .set_buttons(rfd::MessageButtons::YesNo)
                .set_parent(&handle)
                .show();
            if answer == rfd::MessageDialogResult::Yes {
                stop_everything(&state);
                CloseRequestResponse::HideWindow
            } else {
                CloseRequestResponse::KeepWindowShown
            }
        }
    });
}

/// Pertanyaan saat jendela ditutup di tengah unduhan.
fn close_question(running: usize) -> String {
    format!(
        "{running} unduhan masih berjalan. Hentikan dan tutup DANload?\n\nUnduhan yang belum selesai tetap tersimpan di antrean, lalu diulang dari awal saat DANload dibuka lagi."
    )
}

/// Menghentikan semua pekerjaan saat DANload ditutup. Antrean dibekukan dulu (supaya unduhan yang
/// dihentikan tetap tercatat), lalu unduhan dan pembacaan playlist dihentikan dan ditunggu
/// bersama, paling lama [`SHUTDOWN_TIMEOUT`].
fn stop_everything(state: &AppState) {
    freeze_queue(state);
    let mut handles: Vec<CancelHandle> = state
        .active_downloads
        .borrow()
        .iter()
        .map(|active| active.cancel.clone())
        .collect();
    if let Some(pick) = state.queue.playlist.borrow().as_ref() {
        handles.push(pick.cancel.clone());
    }
    for handle in &handles {
        handle.cancel();
    }
    let started = Instant::now();
    while handles.iter().any(|handle| !handle.is_finished()) && started.elapsed() < SHUTDOWN_TIMEOUT
    {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Lokasi program yang dibutuhkan untuk mengunduh, dari hasil pemeriksaan komponen terakhir.
fn current_tools(state: &AppState) -> Option<Tools> {
    let paths = state.component_paths.borrow();
    let path_of = |wanted: Component| {
        Component::ALL
            .iter()
            .position(|component| *component == wanted)
            .and_then(|index| paths.get(index).cloned().flatten())
    };
    Some(Tools {
        yt_dlp: path_of(Component::YtDlp)?,
        ffmpeg: path_of(Component::Ffmpeg)?,
        ffprobe: path_of(Component::Ffprobe)?,
        deno: path_of(Component::Deno)?,
    })
}

/// Mengisi pilihan format dan kualitas di panel kanan sesuai mode, pilihan terakhir, dan
/// platform yang dipilih.
fn show_format_controls(app: &AppWindow, config: &Config) {
    let mode = config.download_mode;
    let (names, index): (Vec<String>, usize) = match mode {
        DownloadMode::Video => (
            VideoFormat::ALL
                .iter()
                .map(|f| f.name().to_string())
                .collect(),
            config.video_format.index(),
        ),
        DownloadMode::Audio => (
            AudioFormat::ALL
                .iter()
                .map(|f| f.name().to_string())
                .collect(),
            config.audio_format.index(),
        ),
    };
    app.set_format_options(string_model(names));
    app.set_format_index(to_index(index));

    let format = config.output_format(mode);
    let menu = QualityMenu::for_format(format);
    app.set_quality_title(menu.title().into());
    app.set_quality_options(string_model(menu.labels()));
    app.set_quality_index(to_index(menu.index_for(format)));
    app.set_quality_enabled(menu.is_selectable());
    let platform = Platform::from_index(app.get_platform());
    app.set_format_hint(format.hint(platform).into());
    app.set_format_hint_warning(format.hint_is_warning(platform));
    app.set_file_extension(format.extension().into());
}

/// Tombol Unduh ditekan. Link biasa masuk antrean (langsung dimulai jika ada slot); link playlist
/// dibaca dulu isinya supaya pengguna bisa memilih videonya.
fn start_download(app: &AppWindow, state: &AppState) {
    let selected = Platform::from_index(app.get_platform());
    let url = app.get_url().trim().to_string();
    // Diperiksa lagi di sini, walaupun tombolnya sudah dimatikan di UI untuk kasus ini.
    let check = platform::check(selected, &url);
    if !check.is_ok() {
        return;
    }
    let mode = DownloadMode::from_index(app.get_download_mode());
    let (format, folder) = {
        let config = state.config.borrow();
        (config.output_format(mode), config.target_folder(mode))
    };
    if check == UrlCheck::Playlist
        || (app.get_whole_playlist() && platform::video_in_playlist(&url))
    {
        start_playlist_read(app, state, url, format, folder);
        return;
    }
    // Diperiksa lagi di sini, walaupun tombolnya sudah dimatikan di UI jika namanya salah.
    let Ok(file_name) = filename::check_custom_name(&app.get_file_name(), format.extension())
    else {
        return;
    };
    // Nama kustom hanya untuk unduhan ini; unduhan berikutnya kembali memakai judul video.
    app.set_file_name(SharedString::new());
    let job = Job {
        url,
        platform: selected,
        format,
        folder,
        file_name,
        title: String::new(),
    };
    add_to_queue(app, state, vec![job], false);
}

/// Memasukkan unduhan yang diminta pengguna ke antrean (`front`: paling depan, untuk "Coba
/// lagi"), lalu memulai yang bisa dimulai. Halaman pindah ke Unduhan jika ada yang langsung
/// berjalan, atau ke Antrean jika semuanya menunggu giliran.
fn add_to_queue(app: &AppWindow, state: &AppState, jobs: Vec<Job>, front: bool) {
    let added = jobs.len();
    {
        let mut queue = state.queue.queue.borrow_mut();
        for job in jobs {
            if front {
                queue.push_front(job);
            } else {
                queue.push_back(job);
            }
        }
    }
    show_queue(app, state);
    save_queue(state);
    let started = schedule(app, state);
    app.set_current_page(if started > 0 { 0 } else { 2 });
    if added > 1 || started == 0 {
        let queue = state.queue.queue.borrow();
        let text = queued_text(added, queue.len(), queue.is_paused());
        drop(queue);
        show_status(app, state, text, false);
    }
}

/// Pesan setelah unduhan ditambahkan ke antrean. `waiting`: jumlah yang masih menunggu.
fn queued_text(added: usize, waiting: usize, paused: bool) -> String {
    let what = if added == 1 {
        "Unduhan masuk antrean".to_string()
    } else {
        format!("{added} video ditambahkan ke antrean")
    };
    if paused {
        format!("{what}. Antrean sedang dijeda; tekan Lanjutkan di halaman Antrean.")
    } else if waiting == 0 {
        format!("{added} video mulai diunduh.")
    } else {
        format!("{what} ({waiting} menunggu).")
    }
}

/// Memulai unduhan dari antrean selama masih ada slot: paling banyak sesuai Pengaturan, dan
/// Instagram hanya satu per waktu (`queue::can_start`). Tidak memulai apa pun selama komponen
/// belum siap, antrean dijeda, atau DANload sedang ditutup. Mengembalikan jumlah yang dimulai.
fn schedule(app: &AppWindow, state: &AppState) -> usize {
    if state.queue.frozen.get()
        || !app.get_components_ready()
        || state.queue.queue.borrow().is_empty()
    {
        return 0;
    }
    let Some(tools) = current_tools(state) else {
        return 0;
    };
    let max = state.config.borrow().parallel_downloads();
    let mut started = 0;
    loop {
        let running: Vec<Platform> = state
            .active_downloads
            .borrow()
            .iter()
            .map(|active| active.job.platform)
            .collect();
        let next = state.queue.queue.borrow_mut().take_next(&running, max);
        let Some(next) = next else { break };
        launch_download(app, state, next.job, &tools);
        started += 1;
    }
    if started > 0 {
        show_queue(app, state);
        save_queue(state);
    }
    started
}

/// Membuat kartu untuk `job` di halaman Unduhan, lalu menjalankannya di thread terpisah.
fn launch_download(app: &AppWindow, state: &AppState, job: Job, tools: &Tools) {
    let id = state.next_download_id.get();
    state.next_download_id.set(id + 1);
    // Unduhan terbaru tampil paling atas.
    state.download_model.insert(
        0,
        DownloadItem {
            id,
            title: job.display_title().into(),
            subtitle: job_subtitle(&job).into(),
            state: DownloadState::Preparing,
            progress: 0.0,
            detail: "Mengambil info video...".into(),
        },
    );
    state.download_entries.borrow_mut().push(DownloadEntry {
        id,
        job: job.clone(),
        files: Vec::new(),
    });
    let request = job.request(state.library.thumbnails.clone());
    let cancel = CancelHandle::default();
    state.active_downloads.borrow_mut().push(ActiveDownload {
        id,
        job,
        cancel: cancel.clone(),
        cancel_requested: false,
        item: None,
    });
    show_running(app, state);

    let weak = app.as_weak();
    let tools = tools.clone();
    let job_id = u64::try_from(id).unwrap_or_default();
    let format = request.format;
    std::thread::spawn(move || {
        let outcome = downloader::run(&request, &tools, job_id, &cancel, &mut |update| {
            let _ =
                weak.upgrade_in_event_loop(move |_app| apply_download_update(id, format, update));
        });
        let _ = weak.upgrade_in_event_loop(move |app| finish_download(&app, id, outcome));
    });
}

/// Menampilkan satu laporan kemajuan di kartu `id` (unduhan berformat `format`).
/// Dipanggil di UI thread.
fn apply_download_update(id: i32, format: OutputFormat, update: Update) {
    let Some(state) = current_state() else { return };
    let current = {
        let mut active = state.active_downloads.borrow_mut();
        let Some(active) = active
            .iter_mut()
            .find(|active| active.id == id && !active.cancel_requested)
        else {
            return;
        };
        match &update {
            Update::Item(item) => active.item = Some(*item),
            // Judul ikut disimpan di antrean, supaya tampil jika unduhan ini diulang nanti.
            Update::Title(title) if active.job.title.trim().is_empty() => {
                active.job.title = title.clone();
            }
            _ => {}
        }
        active.item
    };
    let numbered = |text: &str| downloader::with_item(text, current);
    update_card(&state, id, |item| match update {
        Update::Title(title) => {
            item.title = title.into();
            item.detail = numbered("Menyiapkan unduhan...").into();
        }
        Update::Item(_) => {
            item.state = DownloadState::Preparing;
            item.progress = 0.0;
            item.detail = numbered("Menyiapkan...").into();
        }
        Update::Downloading {
            part,
            downloaded,
            total,
            speed,
            eta,
        } => {
            let (text, fraction) =
                downloader::describe_download(part, downloaded, total, speed, eta);
            item.state = DownloadState::Downloading;
            item.progress = fraction;
            item.detail = numbered(&text).into();
        }
        Update::Processing(step) => {
            item.state = DownloadState::Processing;
            item.progress = 1.0;
            item.detail = numbered(&downloader::describe_step(step, format)).into();
        }
        Update::Converting(progress) => {
            let (text, fraction) = convert::describe(&progress);
            item.state = DownloadState::Converting;
            item.progress = fraction;
            item.detail = numbered(&text).into();
        }
        Update::WaitingForConversion => {
            item.state = DownloadState::Waiting;
            item.detail =
                numbered("Menunggu giliran konversi (unduhan lain sedang dikonversi)...").into();
        }
    });
}

/// Unduhan `id` selesai (berhasil, gagal, atau dibatalkan): catat hasilnya, lalu mulai unduhan
/// berikutnya dari antrean. Dipanggil di UI thread.
fn finish_download(app: &AppWindow, id: i32, outcome: Outcome) {
    let Some(state) = current_state() else { return };
    {
        let mut active = state.active_downloads.borrow_mut();
        if let Some(index) = active.iter().position(|active| active.id == id) {
            active.remove(index);
        }
    }
    if let Outcome::Done { files, .. } = &outcome {
        if let Some(entry) = state
            .download_entries
            .borrow_mut()
            .iter_mut()
            .find(|entry| entry.id == id)
        {
            entry.files = files.iter().map(|file| file.path.clone()).collect();
        }
        record_downloads(app, &state, id, files);
    }
    update_card(&state, id, |item| match outcome {
        Outcome::Done { files, note } => {
            item.state = DownloadState::Done;
            item.progress = 1.0;
            let paths: Vec<PathBuf> = files.iter().map(|file| file.path.clone()).collect();
            item.detail = done_text(&paths, note.as_deref()).into();
        }
        Outcome::Failed(message) => {
            item.state = DownloadState::Failed;
            item.detail = message.into();
        }
        Outcome::Cancelled => {
            item.state = DownloadState::Cancelled;
            item.detail = "Dibatalkan.".into();
        }
    });
    show_running(app, &state);
    // Unduhan yang sudah selesai (apa pun hasilnya) tidak diulang saat DANload dibuka lagi.
    save_queue(&state);
    schedule(app, &state);
}

/// Teks kartu unduhan yang berhasil, mis. `Selesai · Judul.mp4` atau
/// `Selesai · 3 file disimpan · Video by akun.mp4, …`. `note`: item yang dilewati, jika ada.
fn done_text(files: &[PathBuf], note: Option<&str>) -> String {
    let name = |file: &PathBuf| {
        file.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let mut text = match files {
        [] => "Selesai.".to_string(),
        [file] => format!("Selesai · {}", name(file)),
        [first, ..] => format!(
            "Selesai · {} file disimpan · {}, …",
            files.len(),
            name(first)
        ),
    };
    if let Some(note) = note {
        text.push_str(" · ");
        text.push_str(note);
    }
    text
}

/// Tombol Batal pada kartu `id`.
fn cancel_download(state: &AppState, id: i32) {
    let cancel = {
        let mut active = state.active_downloads.borrow_mut();
        match active.iter_mut().find(|active| active.id == id) {
            Some(active) if !active.cancel_requested => {
                active.cancel_requested = true;
                active.cancel.clone()
            }
            _ => return,
        }
    };
    cancel.cancel();
    update_card(state, id, |item| item.detail = "Membatalkan...".into());
    // Unduhan yang dibatalkan pengguna tidak diulang saat DANload dibuka lagi.
    save_queue(state);
}

/// Tombol "Coba lagi" pada unduhan yang gagal atau dibatalkan: kartunya dihapus dan unduhannya
/// masuk lagi ke antrean paling depan (langsung dimulai jika ada slot).
fn retry_download(app: &AppWindow, state: &AppState, id: i32) {
    if state
        .active_downloads
        .borrow()
        .iter()
        .any(|active| active.id == id)
    {
        return;
    }
    let job = {
        let mut entries = state.download_entries.borrow_mut();
        let Some(index) = entries.iter().position(|entry| entry.id == id) else {
            return;
        };
        entries.remove(index).job
    };
    remove_card(state, id);
    add_to_queue(app, state, vec![job], true);
}

/// "Batal semua" (setelah konfirmasi): antrean dikosongkan dan semua unduhan yang berjalan
/// dihentikan.
fn cancel_all(app: &AppWindow, state: &AppState) {
    state.queue.queue.borrow_mut().clear();
    let ids: Vec<i32> = state
        .active_downloads
        .borrow()
        .iter()
        .map(|active| active.id)
        .collect();
    for id in ids {
        cancel_download(state, id);
    }
    show_queue(app, state);
    save_queue(state);
    show_status(app, state, "Semua unduhan dibatalkan.", false);
}

/// Teks kecil di bawah judul kartu dan baris antrean, mis. `YouTube · MP4 · 1080p`.
fn job_subtitle(job: &Job) -> String {
    format!("{} · {}", job.platform.name(), job.format.label())
}

/// Jumlah unduhan yang berjalan di status bar dan petunjuk tombol Unduh.
fn show_running(app: &AppWindow, state: &AppState) {
    let active = state.active_downloads.borrow();
    app.set_running_count(to_index(active.len()));
    app.set_instagram_running(
        active
            .iter()
            .any(|active| active.job.platform == Platform::Instagram),
    );
}

/// Mengubah isi kartu `id` di daftar unduhan.
fn update_card(state: &AppState, id: i32, change: impl FnOnce(&mut DownloadItem)) {
    let model = &state.download_model;
    let Some(row) =
        (0..model.row_count()).find(|row| model.row_data(*row).is_some_and(|item| item.id == id))
    else {
        return;
    };
    if let Some(mut item) = model.row_data(row) {
        change(&mut item);
        model.set_row_data(row, item);
    }
}

/// Menghapus kartu `id` dari daftar unduhan.
fn remove_card(state: &AppState, id: i32) {
    let model = &state.download_model;
    if let Some(row) =
        (0..model.row_count()).find(|row| model.row_data(*row).is_some_and(|item| item.id == id))
    {
        model.remove(row);
    }
}

// ---------------------------------------------------------------------------
// Antrean dan playlist (sejak Fase 7)
// ---------------------------------------------------------------------------

/// Penangan aksi halaman Antrean, dialog pilih video dari playlist, dialog konfirmasi, dan
/// pilihan unduhan bersamaan di Pengaturan.
fn connect_queue(app: &AppWindow, state: &Rc<AppState>) {
    app.on_queue_toggle_pause(with_app(app, state, toggle_pause));
    app.on_queue_remove(with_app_arg(app, state, |app, state, id: i32| {
        if let Ok(id) = u64::try_from(id) {
            state.queue.queue.borrow_mut().remove(id);
            show_queue(app, state);
            save_queue(state);
        }
    }));
    app.on_queue_clear(with_app(app, state, |app, state| {
        ask_confirm(app, state, ConfirmAction::ClearQueue);
    }));
    app.on_confirm_accepted(with_app(app, state, |app, state| {
        app.set_confirm_dialog(ConfirmDialog::default());
        match state.queue.pending_confirm.take() {
            Some(ConfirmAction::CancelAll) => cancel_all(app, state),
            Some(ConfirmAction::ClearQueue) => {
                state.queue.queue.borrow_mut().clear();
                show_queue(app, state);
                save_queue(state);
                show_status(app, state, "Antrean dikosongkan.", false);
            }
            None => {}
        }
    }));
    app.on_confirm_dismissed(with_app(app, state, |app, state| {
        state.queue.pending_confirm.set(None);
        app.set_confirm_dialog(ConfirmDialog::default());
    }));

    app.on_playlist_toggled(with_app_arg(app, state, toggle_playlist_entry));
    app.on_playlist_check_all(with_app(app, state, |app, state| {
        check_all_playlist_entries(app, state, true);
    }));
    app.on_playlist_clear_checks(with_app(app, state, |app, state| {
        check_all_playlist_entries(app, state, false);
    }));
    app.on_playlist_confirm(with_app(app, state, confirm_playlist));
    app.on_playlist_cancel(with_app(app, state, close_playlist));

    app.on_parallel_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |index| {
            let count = u32::try_from(index + 1).unwrap_or(config::MAX_PARALLEL_DOWNLOADS);
            state.config.borrow_mut().max_parallel_downloads = count;
            save_config(&weak, &state);
            if let Some(app) = weak.upgrade() {
                show_parallel(&app, &state);
                // Batas dinaikkan: unduhan di antrean bisa langsung dimulai.
                schedule(&app, &state);
            }
        }
    });
}

/// Aksi yang menunggu jawaban dialog konfirmasi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfirmAction {
    CancelAll,
    ClearQueue,
}

/// Menampilkan dialog konfirmasi untuk `action`. Tidak tampil jika tidak ada yang terpengaruh.
fn ask_confirm(app: &AppWindow, state: &AppState, action: ConfirmAction) {
    let running = state
        .active_downloads
        .borrow()
        .iter()
        .filter(|active| !active.cancel_requested)
        .count();
    let waiting = state.queue.queue.borrow().len();
    let (title, message, confirm) = match action {
        ConfirmAction::CancelAll => {
            let mut parts = Vec::new();
            if running > 0 {
                parts.push(format!("{running} unduhan yang sedang berjalan dihentikan"));
            }
            if waiting > 0 {
                parts.push(format!("{waiting} unduhan di antrean dikeluarkan"));
            }
            if parts.is_empty() {
                return;
            }
            (
                "Batalkan semua unduhan?",
                format!(
                    "{}. File yang sudah selesai diunduh tidak terpengaruh.",
                    parts.join(", dan ")
                ),
                "Batalkan semua",
            )
        }
        ConfirmAction::ClearQueue => {
            if waiting == 0 {
                return;
            }
            (
                "Kosongkan antrean?",
                format!(
                    "{waiting} unduhan yang menunggu dikeluarkan dari antrean. Unduhan yang sedang berjalan tetap diselesaikan."
                ),
                "Kosongkan antrean",
            )
        }
    };
    state.queue.pending_confirm.set(Some(action));
    app.set_confirm_dialog(ConfirmDialog {
        visible: true,
        title: title.into(),
        message: message.into(),
        confirm: confirm.into(),
    });
}

/// Tombol Jeda / Lanjutkan di halaman Antrean.
fn toggle_pause(app: &AppWindow, state: &AppState) {
    let paused = {
        let mut queue = state.queue.queue.borrow_mut();
        let paused = !queue.is_paused();
        queue.set_paused(paused);
        paused
    };
    show_queue(app, state);
    save_queue(state);
    if !paused {
        schedule(app, state);
    }
}

/// Menampilkan isi antrean (halaman Antrean, angka di sidebar, status bar).
fn show_queue(app: &AppWindow, state: &AppState) {
    let queue = state.queue.queue.borrow();
    let rows: Vec<QueueRow> = queue
        .waiting()
        .map(|waiting| QueueRow {
            id: i32::try_from(waiting.id).unwrap_or(i32::MAX),
            title: waiting.job.display_title().into(),
            subtitle: job_subtitle(&waiting.job).into(),
        })
        .collect();
    state.queue.model.set_vec(rows);
    app.set_waiting_count(to_index(queue.len()));
    app.set_queue_paused(queue.is_paused());
}

/// Pilihan unduhan bersamaan di Pengaturan dan batasnya untuk petunjuk tombol Unduh.
fn show_parallel(app: &AppWindow, state: &AppState) {
    let count = state.config.borrow().parallel_downloads();
    app.set_max_parallel(to_index(count));
    app.set_parallel_index(to_index(count.saturating_sub(1)));
}

/// Menyimpan antrean ke `queue.json` di thread lain: unduhan yang sedang berjalan dulu (supaya
/// diulang paling dulu saat DANload dibuka lagi), lalu yang menunggu.
fn save_queue(state: &AppState) {
    if state.queue.frozen.get() {
        return;
    }
    let bytes = {
        let active = state.active_downloads.borrow();
        let running = active
            .iter()
            .filter(|active| !active.cancel_requested)
            .map(|active| &active.job);
        queue::to_json(running, &state.queue.queue.borrow())
    };
    if let (Some(bytes), Some(saver)) = (bytes, state.queue.saver.borrow().as_ref()) {
        saver.save(bytes);
    }
}

/// Menyimpan isi antrean terakhir, lalu membekukannya: setelah ini `queue.json` tidak diubah
/// lagi, jadi unduhan yang dihentikan karena DANload ditutup tetap tercatat.
fn freeze_queue(state: &AppState) {
    if !state.queue.frozen.get() {
        save_queue(state);
        state.queue.frozen.set(true);
    }
}

/// Mulai membaca isi playlist `url` di thread lain. Dialog pilih video langsung tampil dengan
/// keterangan kemajuannya.
fn start_playlist_read(
    app: &AppWindow,
    state: &AppState,
    url: String,
    format: OutputFormat,
    folder: PathBuf,
) {
    if state.queue.playlist.borrow().is_some() {
        return;
    }
    let Some(tools) = current_tools(state) else {
        return;
    };
    let number = state.queue.next_playlist.get() + 1;
    state.queue.next_playlist.set(number);
    let cancel = CancelHandle::default();
    *state.queue.playlist.borrow_mut() = Some(PlaylistPick {
        number,
        cancel: cancel.clone(),
        format,
        folder,
        entries: Vec::new(),
        checked: Vec::new(),
    });
    state.queue.playlist_model.set_vec(Vec::new());
    app.set_playlist_dialog(PlaylistDialog {
        visible: true,
        loading: true,
        message: "Membaca daftar video di playlist...".into(),
        format: format.label().into(),
        ..PlaylistDialog::default()
    });

    let weak = app.as_weak();
    std::thread::spawn(move || {
        let outcome = playlist::read(&url, &tools, &cancel, &mut |count| {
            let _ =
                weak.upgrade_in_event_loop(move |app| show_playlist_progress(&app, number, count));
        });
        let _ = weak.upgrade_in_event_loop(move |app| finish_playlist_read(&app, number, outcome));
    });
}

/// `true` jika `number` adalah pembacaan playlist yang sedang berlaku (bukan yang sudah ditutup).
fn is_current_pick(state: &AppState, number: u64) -> bool {
    state
        .queue
        .playlist
        .borrow()
        .as_ref()
        .is_some_and(|pick| pick.number == number)
}

fn show_playlist_progress(app: &AppWindow, number: u64, count: usize) {
    let Some(state) = current_state() else { return };
    let mut dialog = app.get_playlist_dialog();
    if is_current_pick(&state, number) && dialog.loading {
        dialog.message = format!("Membaca daftar video di playlist... {count} video").into();
        app.set_playlist_dialog(dialog);
    }
}

/// Isi playlist selesai dibaca (atau gagal): tampilkan daftarnya untuk dipilih. Semua video yang
/// bisa diunduh langsung dicentang.
fn finish_playlist_read(app: &AppWindow, number: u64, outcome: playlist::Outcome) {
    let Some(state) = current_state() else { return };
    if !is_current_pick(&state, number) {
        return;
    }
    let mut dialog = app.get_playlist_dialog();
    dialog.loading = false;
    match outcome {
        playlist::Outcome::Done(list) => {
            let available = list.available();
            dialog.title = if list.title.is_empty() {
                "Playlist YouTube".into()
            } else {
                list.title.as_str().into()
            };
            dialog.message = match (&list.note, available) {
                (Some(note), _) => note.as_str().into(),
                (None, 0) => "Tidak ada video di playlist ini yang bisa diunduh.".into(),
                (None, _) => SharedString::new(),
            };
            dialog.available = to_index(available);
            dialog.checked = to_index(available);
            let mut playlist = state.queue.playlist.borrow_mut();
            if let Some(pick) = playlist.as_mut() {
                pick.checked = list
                    .entries
                    .iter()
                    .map(|entry| entry.unavailable.is_none())
                    .collect();
                pick.entries = list.entries;
                state.queue.playlist_model.set_vec(playlist_rows(pick));
            }
        }
        playlist::Outcome::Failed(message) => {
            dialog.error = true;
            dialog.message = message.into();
        }
        playlist::Outcome::Cancelled => {
            close_playlist(app, &state);
            return;
        }
    }
    app.set_playlist_dialog(dialog);
}

fn playlist_rows(pick: &PlaylistPick) -> Vec<PlaylistEntry> {
    pick.entries
        .iter()
        .zip(&pick.checked)
        .map(|(entry, checked)| playlist_row(entry, *checked))
        .collect()
}

/// Satu baris di dialog pilih video: durasi, atau alasan video tidak bisa diunduh.
fn playlist_row(entry: &playlist::Entry, checked: bool) -> PlaylistEntry {
    let meta = match (entry.unavailable, entry.duration) {
        (Some(reason), _) => reason.to_string(),
        (None, Some(seconds)) => downloader::format_duration(seconds.round() as u64),
        (None, None) => String::new(),
    };
    PlaylistEntry {
        title: entry.title.as_str().into(),
        meta: meta.into(),
        available: entry.unavailable.is_none(),
        checked,
    }
}

/// Mencentang atau melepas centang satu video di dialog pilih video.
fn toggle_playlist_entry(app: &AppWindow, state: &AppState, index: i32) {
    let Ok(index) = usize::try_from(index) else {
        return;
    };
    let count = {
        let mut playlist = state.queue.playlist.borrow_mut();
        let Some(pick) = playlist.as_mut() else {
            return;
        };
        let (Some(entry), Some(checked)) = (pick.entries.get(index), pick.checked.get_mut(index))
        else {
            return;
        };
        if entry.unavailable.is_some() {
            return;
        }
        *checked = !*checked;
        state
            .queue
            .playlist_model
            .set_row_data(index, playlist_row(entry, *checked));
        pick.checked.iter().filter(|checked| **checked).count()
    };
    let mut dialog = app.get_playlist_dialog();
    dialog.checked = to_index(count);
    app.set_playlist_dialog(dialog);
}

/// "Pilih semua" (`value` = `true`) atau "Kosongkan" di dialog pilih video.
fn check_all_playlist_entries(app: &AppWindow, state: &AppState, value: bool) {
    let count = {
        let mut playlist = state.queue.playlist.borrow_mut();
        let Some(pick) = playlist.as_mut() else {
            return;
        };
        for (entry, checked) in pick.entries.iter().zip(pick.checked.iter_mut()) {
            *checked = value && entry.unavailable.is_none();
        }
        state.queue.playlist_model.set_vec(playlist_rows(pick));
        pick.checked.iter().filter(|checked| **checked).count()
    };
    let mut dialog = app.get_playlist_dialog();
    dialog.checked = to_index(count);
    app.set_playlist_dialog(dialog);
}

/// "Tambah N video ke antrean": video yang dicentang masuk antrean sebagai unduhan sendiri-sendiri,
/// dengan format dan folder saat tombol Unduh ditekan.
fn confirm_playlist(app: &AppWindow, state: &AppState) {
    let Some(pick) = state.queue.playlist.borrow_mut().take() else {
        return;
    };
    let jobs: Vec<Job> = pick
        .entries
        .into_iter()
        .zip(pick.checked)
        .filter(|(_, checked)| *checked)
        .map(|(entry, _)| Job {
            url: entry.url,
            platform: Platform::YouTube,
            format: pick.format,
            folder: pick.folder.clone(),
            file_name: None,
            title: entry.title,
        })
        .collect();
    close_playlist(app, state);
    if !jobs.is_empty() {
        add_to_queue(app, state, jobs, false);
    }
}

/// Menutup dialog pilih video. Pembacaan yang masih berjalan dihentikan.
fn close_playlist(app: &AppWindow, state: &AppState) {
    if let Some(pick) = state.queue.playlist.borrow_mut().take() {
        pick.cancel.cancel();
    }
    state.queue.playlist_model.set_vec(Vec::new());
    app.set_playlist_dialog(PlaylistDialog::default());
}

/// Penangan aksi tanpa argumen yang memanggil `action` dengan jendela dan state, selama jendelanya
/// masih ada.
fn with_app(
    app: &AppWindow,
    state: &Rc<AppState>,
    action: impl Fn(&AppWindow, &AppState) + 'static,
) -> impl Fn() + 'static {
    let weak = app.as_weak();
    let state = Rc::clone(state);
    move || {
        if let Some(app) = weak.upgrade() {
            action(&app, &state);
        }
    }
}

/// Seperti [`with_app`], untuk aksi dengan satu argumen (mis. nomor baris).
fn with_app_arg<T: 'static>(
    app: &AppWindow,
    state: &Rc<AppState>,
    action: impl Fn(&AppWindow, &AppState, T) + 'static,
) -> impl Fn(T) + 'static {
    let weak = app.as_weak();
    let state = Rc::clone(state);
    move |value| {
        if let Some(app) = weak.upgrade() {
            action(&app, &state, value);
        }
    }
}

// ---------------------------------------------------------------------------
// Pustaka
// ---------------------------------------------------------------------------

/// Baris daftar Pustaka untuk Slint.
///
/// Thumbnail baru dibuka saat barisnya diminta (ListView hanya meminta baris yang terlihat), lalu
/// disimpan sementara oleh cache gambar Slint (paling besar 5 MB). Jadi RAM tetap kecil walau
/// Pustaka berisi ribuan file (CLAUDE.md Bagian 2.6).
#[derive(Default)]
struct LibraryModel {
    rows: RefCell<Vec<RowData>>,
    notify: ModelNotify,
}

/// Isi satu baris, tanpa gambarnya.
#[derive(Clone)]
struct RowData {
    id: u64,
    name: SharedString,
    meta: SharedString,
    date: SharedString,
    thumbnail: Option<PathBuf>,
    audio: bool,
    missing: bool,
    checked: bool,
    selected: bool,
}

impl Model for LibraryModel {
    type Data = LibraryRow;

    fn row_count(&self) -> usize {
        self.rows.borrow().len()
    }

    fn row_data(&self, row: usize) -> Option<LibraryRow> {
        let rows = self.rows.borrow();
        let data = rows.get(row)?;
        let thumbnail = data
            .thumbnail
            .as_deref()
            .and_then(|path| Image::load_from_path(path).ok());
        Some(LibraryRow {
            name: data.name.clone(),
            meta: data.meta.clone(),
            date: data.date.clone(),
            has_thumbnail: thumbnail.is_some(),
            thumbnail: thumbnail.unwrap_or_default(),
            audio: data.audio,
            missing: data.missing,
            checked: data.checked,
            selected: data.selected,
        })
    }

    fn model_tracker(&self) -> &dyn ModelTracker {
        &self.notify
    }
}

impl LibraryModel {
    /// Mengganti semua baris (setelah cari, filter, urut, atau isi Pustaka berubah).
    fn set_rows(&self, rows: Vec<RowData>) {
        *self.rows.borrow_mut() = rows;
        self.notify.reset();
    }

    fn id_at(&self, row: i32) -> Option<u64> {
        let row = usize::try_from(row).ok()?;
        self.rows.borrow().get(row).map(|data| data.id)
    }

    /// Nomor file di semua baris, urut seperti yang tampil.
    fn ids(&self) -> Vec<u64> {
        self.rows.borrow().iter().map(|data| data.id).collect()
    }

    /// Mengubah baris-baris yang perlu; `change` mengembalikan `true` jika barisnya berubah.
    fn update(&self, mut change: impl FnMut(&mut RowData) -> bool) {
        let changed: Vec<usize> = self
            .rows
            .borrow_mut()
            .iter_mut()
            .enumerate()
            .filter_map(|(index, data)| change(data).then_some(index))
            .collect();
        // Diberi tahu setelah `rows` tidak dipinjam lagi, karena Slint bisa langsung membacanya.
        for index in changed {
            self.notify.row_changed(index);
        }
    }
}

/// Penangan aksi di halaman Pustaka, panel detail, dan dialog hapus.
fn connect_library(app: &AppWindow, state: &Rc<AppState>) {
    app.on_library_opened({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            let due = state
                .library
                .last_check
                .get()
                .is_none_or(|last| last.elapsed() >= FILE_CHECK_INTERVAL);
            if let Some(app) = weak.upgrade()
                && due
            {
                check_library_files(&app, &state);
            }
        }
    });

    app.on_library_search_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |text| {
            state.library.query.borrow_mut().text = text.to_string();
            if let Some(app) = weak.upgrade() {
                refresh_library(&app, &state);
            }
        }
    });

    app.on_library_filter_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |kind, platform| {
            {
                let mut query = state.library.query.borrow_mut();
                query.kind = match kind {
                    1 => KindFilter::Video,
                    2 => KindFilter::Audio,
                    _ => KindFilter::All,
                };
                query.platform = (1..=3)
                    .contains(&platform)
                    .then(|| Platform::from_index(platform - 1));
            }
            if let Some(app) = weak.upgrade() {
                app.set_library_kind(kind);
                app.set_library_platform(platform);
                refresh_library(&app, &state);
            }
        }
    });

    app.on_library_sort_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |index| {
            let sort = SortOrder::from_index(index);
            state.library.query.borrow_mut().sort = sort;
            if let Some(app) = weak.upgrade() {
                app.set_library_sort(to_index(
                    SortOrder::ALL.iter().position(|s| *s == sort).unwrap_or(0),
                ));
                refresh_library(&app, &state);
            }
        }
    });

    app.on_library_row_clicked({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |row| {
            if let (Some(app), Some(id)) = (weak.upgrade(), state.library.model.id_at(row)) {
                select_library_item(&app, &state, Some(id));
            }
        }
    });

    app.on_library_row_toggled({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |row| {
            let Some(id) = state.library.model.id_at(row) else {
                return;
            };
            let checked = {
                let mut set = state.library.checked.borrow_mut();
                if !set.remove(&id) {
                    set.insert(id);
                }
                set.contains(&id)
            };
            state.library.model.update(|data| {
                let change = data.id == id && data.checked != checked;
                if change {
                    data.checked = checked;
                }
                change
            });
            if let Some(app) = weak.upgrade() {
                app.set_library_checked(to_index(state.library.checked.borrow().len()));
            }
        }
    });

    app.on_library_check_all({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            let ids = state.library.model.ids();
            state
                .library
                .checked
                .borrow_mut()
                .extend(ids.iter().copied());
            state
                .library
                .model
                .update(|data| !std::mem::replace(&mut data.checked, true));
            if let Some(app) = weak.upgrade() {
                app.set_library_checked(to_index(state.library.checked.borrow().len()));
            }
        }
    });

    app.on_library_clear_checks({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            state.library.checked.borrow_mut().clear();
            state
                .library
                .model
                .update(|data| std::mem::replace(&mut data.checked, false));
            if let Some(app) = weak.upgrade() {
                app.set_library_checked(0);
            }
        }
    });

    app.on_library_delete_checked({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            let checked = state.library.checked.borrow().clone();
            let ids: Vec<u64> = state
                .library
                .model
                .ids()
                .into_iter()
                .filter(|id| checked.contains(id))
                .collect();
            if let Some(app) = weak.upgrade() {
                request_delete(&app, &state, ids);
            }
        }
    });

    app.on_library_delete_selected({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            if let (Some(app), Some(id)) = (weak.upgrade(), state.library.selected.get()) {
                request_delete(&app, &state, vec![id]);
            }
        }
    });

    app.on_library_confirm_delete({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |recycle| {
            if let Some(app) = weak.upgrade() {
                confirm_delete(&app, &state, recycle);
            }
        }
    });

    app.on_library_cancel_delete({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            state.library.pending_delete.borrow_mut().clear();
            if let Some(app) = weak.upgrade() {
                app.set_delete_dialog(DeleteDialog::default());
            }
        }
    });

    app.on_library_close_detail({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            if let Some(app) = weak.upgrade() {
                select_library_item(&app, &state, None);
            }
        }
    });

    app.on_library_open({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            if let Some(app) = weak.upgrade() {
                open_selected(&app, &state);
            }
        }
    });

    app.on_library_show_in_folder({
        let state = Rc::clone(state);
        move || {
            if let Some(path) = selected_path(&state) {
                process::show_in_file_manager(&path);
            }
        }
    });

    // Pesan untuk kolom ganti nama (kosong = nama boleh dipakai). Hanya membaca.
    app.on_library_check_rename({
        let state = Rc::clone(state);
        move |name| match selected_path(&state) {
            Some(path) => match library::rename_target(&path, &name) {
                Ok(_) => SharedString::new(),
                Err(message) => message.into(),
            },
            None => SharedString::new(),
        }
    });

    app.on_library_rename({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |name| {
            if let Some(app) = weak.upgrade() {
                rename_selected(&app, &state, &name);
            }
        }
    });

    app.on_library_copied({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            if let Some(app) = weak.upgrade() {
                show_status(&app, &state, "Link sumber disalin.", false);
            }
        }
    });

    app.on_library_redownload({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            if let Some(app) = weak.upgrade() {
                redownload_selected(&app, &state);
            }
        }
    });
}

/// Menyusun ulang daftar Pustaka sesuai pencarian, filter, dan urutan yang berlaku.
fn refresh_library(app: &AppWindow, state: &AppState) {
    let lib = &state.library;
    let (rows, shown, total) = {
        let library = lib.library.borrow();
        let items = library.items();
        let found = library::query(items, &lib.query.borrow());
        // File yang tersembunyi oleh filter tidak ikut dicentang atau terbuka di panel detail.
        let visible: HashSet<u64> = found.iter().map(|index| items[*index].id).collect();
        lib.checked.borrow_mut().retain(|id| visible.contains(id));
        if lib.selected.get().is_some_and(|id| !visible.contains(&id)) {
            lib.selected.set(None);
        }
        let now = system::local_time(library::now_unix());
        let missing = lib.missing.borrow();
        let checked = lib.checked.borrow();
        let rows: Vec<RowData> = found
            .iter()
            .map(|index| {
                let item = &items[*index];
                RowData {
                    id: item.id,
                    name: item.name().into(),
                    meta: meta_text(item).into(),
                    date: library::short_date(system::local_time(item.downloaded_at), now).into(),
                    thumbnail: thumbnail_file(lib, item),
                    audio: item.mode == DownloadMode::Audio,
                    missing: missing.contains(&item.id),
                    checked: checked.contains(&item.id),
                    selected: lib.selected.get() == Some(item.id),
                }
            })
            .collect();
        (rows, found.len(), items.len())
    };
    lib.model.set_rows(rows);
    app.set_library_total(to_index(total));
    app.set_library_count(count_text(shown, total).into());
    app.set_library_checked(to_index(lib.checked.borrow().len()));
    show_library_detail(app, state);
}

/// Teks jumlah di samping judul Pustaka, mis. `12 file` atau `3 dari 12 file`.
fn count_text(shown: usize, total: usize) -> String {
    match (shown, total) {
        (_, 0) => String::new(),
        (shown, total) if shown == total => format!("{total} file"),
        (shown, total) => format!("{shown} dari {total} file"),
    }
}

/// Baris kedua di daftar, mis. `YouTube · MP4 · 1080p · 45,2 MB · 3:25`.
fn meta_text(item: &library::Item) -> String {
    let mut parts = vec![item.platform.name().to_string(), item.format_name()];
    if !item.quality.is_empty() {
        parts.push(item.quality.clone());
    }
    if item.size > 0 {
        parts.push(downloader::format_size(item.size));
    }
    if let Some(duration) = item.duration.filter(|duration| *duration >= 0.5) {
        parts.push(downloader::format_duration(duration.round() as u64));
    }
    parts.join(" · ")
}

/// `text` dipotong menjadi paling banyak `max` karakter, diakhiri "…" jika dipotong.
fn shorten(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// Seperti [`shorten`], tetapi bagian tengah yang dibuang (untuk lokasi folder:
/// awal dan akhirnya yang paling berguna), mis. `C:\Users\…\DANload\Video`.
fn shorten_middle(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(1);
    let head = keep / 3;
    let tail = keep - head;
    let start: String = chars[..head].iter().collect();
    let end: String = chars[chars.len() - tail..].iter().collect();
    format!("{start}…{end}")
}

/// Lokasi thumbnail milik `item`, jika ada.
fn thumbnail_file(lib: &LibraryState, item: &library::Item) -> Option<PathBuf> {
    library::thumbnail_path(lib.thumbnails.as_deref()?, item.thumbnail.as_deref()?)
}

/// Mengisi panel detail untuk file yang dipilih (atau menutupnya jika tidak ada).
fn show_library_detail(app: &AppWindow, state: &AppState) {
    let lib = &state.library;
    let library = lib.library.borrow();
    let Some(item) = lib.selected.get().and_then(|id| library.get(id)) else {
        app.set_library_has_selection(false);
        app.set_library_renaming(false);
        return;
    };
    let name = item.name();
    let thumbnail = thumbnail_file(lib, item).and_then(|path| Image::load_from_path(&path).ok());
    let format = [item.format_name(), item.quality.clone()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    let unknown = || "–".to_string();
    app.set_library_detail(LibraryDetail {
        title: if item.title.trim() == name.trim() {
            SharedString::new()
        } else {
            shorten(&item.title, DETAIL_TITLE_CHARS).into()
        },
        extension: item.extension().into(),
        name: shorten(&name, DETAIL_NAME_CHARS).into(),
        full_name: name.clone().into(),
        platform: item.platform.name().into(),
        format: format.into(),
        size: if item.size > 0 {
            downloader::format_size(item.size)
        } else {
            unknown()
        }
        .into(),
        duration: item
            .duration
            .filter(|duration| *duration >= 0.5)
            .map_or_else(unknown, |duration| {
                downloader::format_duration(duration.round() as u64)
            })
            .into(),
        date: if item.downloaded_at > 0 {
            library::full_date(system::local_time(item.downloaded_at))
        } else {
            unknown()
        }
        .into(),
        folder: shorten_middle(
            &item
                .path
                .parent()
                .map(|folder| folder.display().to_string())
                .unwrap_or_default(),
            DETAIL_FOLDER_CHARS,
        )
        .into(),
        url: item.source_url.clone().into(),
        has_thumbnail: thumbnail.is_some(),
        thumbnail: thumbnail.unwrap_or_default(),
        audio: item.mode == DownloadMode::Audio,
        missing: lib.missing.borrow().contains(&item.id),
    });
    app.set_library_has_selection(true);
}

/// Membuka file `id` di panel detail (`None` = tutup panel detail).
fn select_library_item(app: &AppWindow, state: &AppState, id: Option<u64>) {
    state.library.selected.set(id);
    state.library.model.update(|data| {
        let selected = Some(data.id) == id;
        selected != std::mem::replace(&mut data.selected, selected)
    });
    app.set_library_renaming(false);
    show_library_detail(app, state);
}

/// Lokasi file yang dipilih di panel detail.
fn selected_path(state: &AppState) -> Option<PathBuf> {
    let id = state.library.selected.get()?;
    state
        .library
        .library
        .borrow()
        .get(id)
        .map(|item| item.path.clone())
}

/// Menyimpan Pustaka (di thread penyimpan).
fn save_library(state: &AppState) {
    let bytes = state.library.library.borrow().to_json();
    if let (Some(bytes), Some(saver)) = (bytes, state.library.saver.borrow().as_ref()) {
        saver.save(bytes);
    }
}

/// Mencatat file hasil unduhan `entry_id` ke Pustaka.
fn record_downloads(app: &AppWindow, state: &AppState, entry_id: i32, files: &[SavedFile]) {
    let Some(job) = state
        .download_entries
        .borrow()
        .iter()
        .find(|entry| entry.id == entry_id)
        .map(|entry| entry.job.clone())
    else {
        return;
    };
    let (mode, format) = library::format_key(job.format);
    let now = library::now_unix();
    let mut stale = Vec::new();
    {
        let mut library = state.library.library.borrow_mut();
        for file in files {
            let id = library.add(library::Item {
                id: 0,
                path: file.path.clone(),
                title: file.title.clone(),
                platform: job.platform,
                mode,
                format: format.clone(),
                quality: file.quality.clone(),
                size: std::fs::metadata(&file.path).map_or(0, |metadata| metadata.len()),
                duration: file.duration,
                downloaded_at: now,
                source_url: job.url.trim().to_string(),
                thumbnail: file
                    .thumbnail
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned()),
            });
            stale.extend(library.remove_others_at(&file.path, id));
        }
    }
    forget_items(state, &stale);
    save_library(state);
    refresh_library(app, state);
}

/// Membersihkan sisa item yang sudah dihapus dari Pustaka: thumbnail, centang, tanda hilang,
/// dan pilihan di panel detail.
fn forget_items(state: &AppState, removed: &[library::Item]) {
    let lib = &state.library;
    if let Some(dir) = &lib.thumbnails {
        for item in removed {
            if let Some(name) = &item.thumbnail {
                library::remove_thumbnail(dir, name);
            }
        }
    }
    let ids: HashSet<u64> = removed.iter().map(|item| item.id).collect();
    lib.checked.borrow_mut().retain(|id| !ids.contains(id));
    lib.missing.borrow_mut().retain(|id| !ids.contains(id));
    if lib.selected.get().is_some_and(|id| ids.contains(&id)) {
        lib.selected.set(None);
    }
}

/// Memeriksa di thread lain file mana yang sudah tidak ada (dipindah/dihapus di luar DANload).
fn check_library_files(app: &AppWindow, state: &AppState) {
    state.library.last_check.set(Some(Instant::now()));
    let items: Vec<(u64, PathBuf)> = state
        .library
        .library
        .borrow()
        .items()
        .iter()
        .map(|item| (item.id, item.path.clone()))
        .collect();
    if items.is_empty() {
        return;
    }
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let sizes = library::check_files(&items);
        let results: Vec<(u64, PathBuf, Option<u64>)> = items
            .into_iter()
            .zip(sizes)
            .map(|((id, path), (_, size))| (id, path, size))
            .collect();
        let _ = weak.upgrade_in_event_loop(move |app| apply_file_check(&app, results));
    });
}

/// Hasil [`check_library_files`]: (nomor, lokasi yang diperiksa, ukuran atau `None` jika hilang).
fn apply_file_check(app: &AppWindow, results: Vec<(u64, PathBuf, Option<u64>)>) {
    let Some(state) = current_state() else { return };
    let lib = &state.library;
    let mut sizes_changed = false;
    let mut missing = HashSet::new();
    {
        let mut library = lib.library.borrow_mut();
        for (id, path, size) in results {
            // Lokasi sudah berubah selama diperiksa (mis. baru saja diganti nama): abaikan.
            if library.get(id).is_none_or(|item| item.path != path) {
                if lib.missing.borrow().contains(&id) {
                    missing.insert(id);
                }
                continue;
            }
            match size {
                Some(size) => {
                    library.update(id, |item| {
                        if item.size != size {
                            item.size = size;
                            sizes_changed = true;
                        }
                    });
                }
                None => {
                    missing.insert(id);
                }
            }
        }
    }
    let missing_changed = *lib.missing.borrow() != missing;
    *lib.missing.borrow_mut() = missing;
    if sizes_changed {
        save_library(&state);
    }
    if sizes_changed || missing_changed {
        refresh_library(app, &state);
    }
}

/// Membuka file yang dipilih dengan aplikasi bawaan Windows.
fn open_selected(app: &AppWindow, state: &AppState) {
    let Some(path) = selected_path(state) else {
        return;
    };
    if !path.is_file() {
        show_status(
            app,
            state,
            "File tidak ditemukan. Mungkin sudah dipindah atau dihapus di luar DANload.",
            true,
        );
        check_library_files(app, state);
        return;
    }
    // Windows Shell bisa butuh waktu sebentar: jalankan di thread lain.
    let weak = app.as_weak();
    std::thread::spawn(move || {
        if let Err(error) = system::open_with_default_app(&path) {
            let _ = weak.upgrade_in_event_loop(move |app| {
                if let Some(state) = current_state() {
                    show_status(
                        &app,
                        &state,
                        format!("File tidak bisa dibuka: {error}"),
                        true,
                    );
                }
            });
        }
    });
}

/// Mengganti nama file yang dipilih menjadi `name` (ekstensinya tetap).
fn rename_selected(app: &AppWindow, state: &AppState, name: &str) {
    let Some(id) = state.library.selected.get() else {
        return;
    };
    let Some(path) = selected_path(state) else {
        return;
    };
    match library::rename_file(&path, name) {
        Ok(new_path) => {
            if new_path != path {
                let stale = {
                    let mut library = state.library.library.borrow_mut();
                    library.update(id, |item| item.path = new_path.clone());
                    library.remove_others_at(&new_path, id)
                };
                forget_items(state, &stale);
                save_library(state);
            }
            app.set_library_renaming(false);
            refresh_library(app, state);
            let file_name = new_path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            show_status(
                app,
                state,
                format!("Nama diganti menjadi {file_name}."),
                false,
            );
        }
        Err(message) => show_status(app, state, message, true),
    }
}

/// Mengisi form unduh dengan link, platform, dan format file yang dipilih, supaya bisa diunduh
/// lagi (formatnya boleh diganti dulu sebelum menekan Unduh).
fn redownload_selected(app: &AppWindow, state: &AppState) {
    let Some(item) = state
        .library
        .selected
        .get()
        .and_then(|id| state.library.library.borrow().get(id).cloned())
    else {
        return;
    };
    let platform = Platform::ALL
        .iter()
        .position(|platform| *platform == item.platform)
        .unwrap_or(0);
    app.set_platform(to_index(platform));
    app.set_url(item.source_url.clone().into());
    app.set_file_name(SharedString::new());
    {
        let mut config = state.config.borrow_mut();
        config.download_mode = item.mode;
        if let Some(format) = item.video_format() {
            config.video_format = format;
        }
        if let Some(format) = item.audio_format() {
            config.audio_format = format;
        }
    }
    app.set_download_mode(item.mode.index());
    show_format_controls(app, &state.config.borrow());
    save_config(&app.as_weak(), state);
    select_library_item(app, state, None);
    show_status(
        app,
        state,
        "Link dan format sudah diisi di panel kanan. Tekan Unduh untuk mengunduh ulang.",
        false,
    );
}

/// Menampilkan dialog konfirmasi untuk menghapus file `ids` (urut seperti di daftar).
fn request_delete(app: &AppWindow, state: &AppState, ids: Vec<u64>) {
    if ids.is_empty() || app.get_library_busy() {
        return;
    }
    let (title, message, can_recycle) = {
        let library = state.library.library.borrow();
        let items: Vec<&library::Item> = ids.iter().filter_map(|id| library.get(*id)).collect();
        let present = items.iter().filter(|item| item.path.is_file()).count();
        let title = match items.as_slice() {
            [item] => format!(
                "Hapus \"{}\"?",
                item.path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default()
            ),
            items => format!("Hapus {} file?", items.len()),
        };
        let message = if present == 0 {
            if items.len() == 1 {
                "File-nya sudah tidak ada di lokasi asalnya, jadi hanya bisa dihapus dari daftar Pustaka."
                    .to_string()
            } else {
                "File-filenya sudah tidak ada di lokasi asalnya, jadi hanya bisa dihapus dari daftar Pustaka."
                    .to_string()
            }
        } else {
            let mut text = "Recycle Bin: file dihapus dari folder, tetapi masih bisa dikembalikan dari Recycle Bin.\nHapus dari daftar saja: file tetap ada di folder, hanya tidak tampil lagi di Pustaka."
                .to_string();
            if present < items.len() {
                text.push_str(&format!(
                    "\n{} file sudah tidak ada di lokasinya; file itu hanya dihapus dari daftar.",
                    items.len() - present
                ));
            }
            text
        };
        (title, message, present > 0)
    };
    *state.library.pending_delete.borrow_mut() = ids;
    app.set_delete_dialog(DeleteDialog {
        visible: true,
        title: title.into(),
        message: message.into(),
        can_recycle,
    });
}

/// Jawaban dialog hapus: `recycle` = pindahkan file ke Recycle Bin; `false` = hapus dari
/// daftar saja (file tetap ada).
fn confirm_delete(app: &AppWindow, state: &AppState, recycle: bool) {
    app.set_delete_dialog(DeleteDialog::default());
    let ids = std::mem::take(&mut *state.library.pending_delete.borrow_mut());
    if ids.is_empty() {
        return;
    }
    if !recycle {
        let removed = remove_from_library(app, state, &ids);
        show_status(
            app,
            state,
            format!("{removed} file dihapus dari daftar Pustaka. File-nya tidak disentuh."),
            false,
        );
        return;
    }
    let targets: Vec<(u64, PathBuf)> = {
        let library = state.library.library.borrow();
        ids.iter()
            .filter_map(|id| library.get(*id).map(|item| (item.id, item.path.clone())))
            .collect()
    };
    app.set_library_busy(true);
    let count = targets.len();
    show_status(
        app,
        state,
        format!("Memindahkan {count} file ke Recycle Bin..."),
        false,
    );
    let weak = app.as_weak();
    std::thread::spawn(move || {
        let results: Vec<RecycleResult> = targets
            .into_iter()
            .map(|(id, path)| {
                let existed = path.is_file();
                let result = if existed {
                    system::move_to_recycle_bin(&path)
                } else {
                    Ok(()) // Sudah tidak ada: cukup dihapus dari daftar.
                };
                RecycleResult {
                    id,
                    path,
                    existed,
                    result,
                }
            })
            .collect();
        let _ = weak.upgrade_in_event_loop(move |app| finish_recycle(&app, results));
    });
}

/// Hasil memindahkan satu file ke Recycle Bin.
struct RecycleResult {
    id: u64,
    path: PathBuf,
    /// `false` jika file sudah tidak ada sebelum dipindah.
    existed: bool,
    result: Result<(), String>,
}

fn finish_recycle(app: &AppWindow, results: Vec<RecycleResult>) {
    let Some(state) = current_state() else { return };
    app.set_library_busy(false);
    let done: Vec<u64> = results
        .iter()
        .filter(|result| result.result.is_ok())
        .map(|result| result.id)
        .collect();
    remove_from_library(app, &state, &done);
    let moved = results
        .iter()
        .filter(|result| result.existed && result.result.is_ok())
        .count();
    let failed: Vec<(String, &String)> = results
        .iter()
        .filter_map(|result| {
            let error = result.result.as_ref().err()?;
            let name = result
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            Some((name, error))
        })
        .collect();
    let message = match failed.as_slice() {
        [] => format!("{moved} file dipindah ke Recycle Bin."),
        [(name, error)] if done.is_empty() => {
            format!("{name} gagal dipindah ke Recycle Bin: {error}")
        }
        [(name, error), ..] => format!(
            "{moved} file dipindah ke Recycle Bin, {} gagal ({name}: {error}).",
            failed.len()
        ),
    };
    let has_failures = !failed.is_empty();
    show_status(app, &state, message, has_failures);
}

/// Menghapus file `ids` dari Pustaka beserta thumbnailnya (filenya sendiri tidak disentuh).
/// Mengembalikan jumlah yang dihapus.
fn remove_from_library(app: &AppWindow, state: &AppState, ids: &[u64]) -> usize {
    if ids.is_empty() {
        return 0;
    }
    let ids: HashSet<u64> = ids.iter().copied().collect();
    let removed = state.library.library.borrow_mut().remove(&ids);
    forget_items(state, &removed);
    save_library(state);
    refresh_library(app, state);
    removed.len()
}

/// Menampilkan pesan singkat di status bar, yang hilang sendiri setelah beberapa detik.
fn show_status(app: &AppWindow, state: &AppState, message: impl Into<SharedString>, error: bool) {
    let number = state.library.message_number.get() + 1;
    state.library.message_number.set(number);
    app.set_status_message(message.into());
    app.set_status_error(error);
    let weak = app.as_weak();
    let duration = if error {
        STATUS_ERROR_DURATION
    } else {
        STATUS_DURATION
    };
    Timer::single_shot(duration, move || {
        let current =
            current_state().is_some_and(|state| state.library.message_number.get() == number);
        if let Some(app) = weak.upgrade()
            && current
        {
            app.set_status_message(SharedString::new());
        }
    });
}

// ---------------------------------------------------------------------------
// Pemasangan komponen
// ---------------------------------------------------------------------------

/// Apa yang diminta pengguna di langkah Komponen.
enum InstallRequest {
    /// Unduh dan pasang paket-paket ini dari sumber resmi.
    Download(Vec<Package>),
    /// Pasang dari file program yang dipilih pengguna (paketnya ditentukan dari nama file).
    FromFile(PathBuf),
}

/// Laporan dari thread pemasang ke UI thread.
enum InstallEvent {
    Progress(Progress),
    Finished(Result<(), InstallError>),
}

/// Menjalankan pemasangan di thread terpisah dan menampilkan kemajuannya di wizard.
fn start_install(app: &AppWindow, state: &Rc<AppState>, request: InstallRequest) {
    let wizard = app.global::<WizardState>();
    if wizard.get_installing() {
        return;
    }
    if matches!(&request, InstallRequest::Download(packages) if packages.is_empty()) {
        return; // Tidak ada yang perlu dipasang.
    }
    let Some(bin_dir) = deps::local_bin_dir() else {
        wizard.set_error("Folder data aplikasi (LOCALAPPDATA) tidak ditemukan.".into());
        return;
    };

    // Pemasangan baru: nomor baru dan tanda batal baru.
    let job_id = state.install_job.get() + 1;
    state.install_job.set(job_id);
    let cancel = Arc::new(AtomicBool::new(false));
    *state.install_cancel.borrow_mut() = Arc::clone(&cancel);

    wizard.set_installing(true);
    wizard.set_error("".into());
    wizard.set_progress(0.0);
    wizard.set_progress_text("Menyiapkan...".into());

    let weak = app.as_weak();
    std::thread::spawn(move || {
        let send = |event: InstallEvent| {
            let _ = weak.upgrade_in_event_loop(move |app| {
                apply_install_event(&app, job_id, event);
            });
        };

        let result = match request {
            InstallRequest::Download(packages) => {
                let agent = setup::http_agent();
                packages.into_iter().try_for_each(|package| {
                    setup::install(
                        &agent,
                        package,
                        &setup::sources(package),
                        &bin_dir,
                        &cancel,
                        job_id,
                        &mut |progress| send(InstallEvent::Progress(progress)),
                    )
                })
            }
            InstallRequest::FromFile(file) => {
                setup::install_from_file(&file, &bin_dir).map(|_package| ())
            }
        };
        send(InstallEvent::Finished(result));
    });
}

/// Menampilkan satu laporan pemasangan di wizard. Dipanggil di UI thread.
fn apply_install_event(app: &AppWindow, job_id: u64, event: InstallEvent) {
    let wizard = app.global::<WizardState>();
    let is_current = current_state().is_some_and(|state| state.install_job.get() == job_id);
    if !is_current {
        // Laporan dari pemasangan yang sudah dibatalkan tidak ditampilkan. Tetapi pemasangan
        // itu bisa saja sempat menyelesaikan langkah terakhirnya, jadi saat ia berhenti,
        // periksa ulang supaya status komponen tetap sesuai isi disk.
        if matches!(event, InstallEvent::Finished(_)) && !wizard.get_installing() {
            start_component_check(app);
        }
        return;
    }
    match event {
        InstallEvent::Progress(progress) => {
            let (text, fraction) = describe_progress(&progress);
            wizard.set_progress_text(text.into());
            wizard.set_progress(fraction);
        }
        InstallEvent::Finished(result) => {
            wizard.set_installing(false);
            match result {
                Ok(()) | Err(InstallError::Cancelled) => wizard.set_error("".into()),
                Err(InstallError::Failed(message)) => wizard.set_error(message.into()),
            }
            // Periksa ulang, supaya status komponen sesuai keadaan terbaru di disk.
            start_component_check(app);
        }
    }
}

/// Mengubah laporan kemajuan menjadi teks dan nilai 0..1 untuk batang kemajuan.
fn describe_progress(progress: &Progress) -> (String, f32) {
    match progress {
        Progress::Connecting { package, source } => (
            format!(
                "Menghubungi {source} untuk mengunduh {}...",
                package.display_name()
            ),
            0.0,
        ),
        Progress::Downloading {
            package,
            source,
            received,
            total,
        } => {
            let name = package.display_name();
            match total.filter(|total| *total > 0) {
                Some(total) => (
                    format!(
                        "Mengunduh {name} dari {source}: {} dari {}",
                        megabytes(*received),
                        megabytes(total)
                    ),
                    (*received as f64 / total as f64) as f32,
                ),
                None => (
                    format!("Mengunduh {name} dari {source}: {}", megabytes(*received)),
                    0.0,
                ),
            }
        }
        Progress::Installing { package } => (
            format!("Memeriksa dan memasang {}...", package.display_name()),
            1.0,
        ),
    }
}

/// Ukuran dalam MB dengan koma desimal, mis. `12,3 MB`.
fn megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0).replace('.', ",")
}

// ---------------------------------------------------------------------------
// Config dan dialog
// ---------------------------------------------------------------------------

/// Model daftar teks untuk Slint.
fn string_model(items: Vec<String>) -> ModelRc<SharedString> {
    let items: Vec<SharedString> = items.into_iter().map(SharedString::from).collect();
    ModelRc::new(VecModel::from(items))
}

/// Indeks Rust (`usize`) ke indeks Slint (`int`).
fn to_index(index: usize) -> i32 {
    i32::try_from(index).unwrap_or(0)
}

fn show_download_folder(app: &AppWindow, folder: &Path) {
    app.set_download_folder(folder.display().to_string().into());
}

/// Menyimpan config. Kalau gagal, pesannya ditampilkan di status bar.
fn save_config(weak: &Weak<AppWindow>, state: &AppState) {
    let Some(app) = weak.upgrade() else { return };
    match state.config.borrow().save_to(&state.config_path) {
        Ok(()) => app.set_notice("".into()),
        Err(error) => app.set_notice(format!("Pengaturan gagal disimpan: {error}").into()),
    }
}

/// Membuka dialog "pilih folder" bawaan Windows. `None` jika dibatalkan.
fn pick_folder(app: &AppWindow, start: &Path) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title("Pilih folder unduhan");

    // Buka dialog di folder yang sedang dipakai; kalau belum ada, di folder induknya.
    if let Some(existing) = start.ancestors().find(|folder| folder.is_dir()) {
        dialog = dialog.set_directory(existing);
    }

    // Menempelkan dialog ke jendela DANload, supaya tidak tersembunyi di belakangnya.
    let handle = app.window().window_handle();
    dialog.set_parent(&handle).pick_folder()
}

/// Membuka dialog "pilih file" untuk yt-dlp.exe, ffmpeg.exe, atau deno.exe.
/// `None` jika dibatalkan.
fn pick_program_file(app: &AppWindow) -> Option<PathBuf> {
    let handle = app.window().window_handle();
    rfd::FileDialog::new()
        .set_title("Pilih yt-dlp.exe, ffmpeg.exe, atau deno.exe")
        .add_filter("Program", &["exe"])
        .set_parent(&handle)
        .pick_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_text_names_the_files() {
        let one = vec![PathBuf::from("/unduhan/Lagu.mp3")];
        assert_eq!(done_text(&one, None), "Selesai · Lagu.mp3");
        let many = vec![
            PathBuf::from("/unduhan/Video by akun.mp4"),
            PathBuf::from("/unduhan/Video by akun (2).mp4"),
        ];
        assert_eq!(
            done_text(&many, Some("1 item dilewati: Tidak ada video.")),
            "Selesai · 2 file disimpan · Video by akun.mp4, … · 1 item dilewati: Tidak ada video."
        );
        assert_eq!(done_text(&[], None), "Selesai.");
    }

    #[test]
    fn long_texts_are_shortened() {
        assert_eq!(shorten("Pendek", 10), "Pendek");
        assert_eq!(shorten("Judul yang sangat panjang", 10), "Judul yan…");
        assert_eq!(shorten("é日本語テキストです", 5), "é日本語…");
        assert_eq!(
            shorten_middle("C:\\Users\\Jidan\\Downloads\\DANload\\Video", 25),
            "C:\\Users…ds\\DANload\\Video"
        );
        assert_eq!(shorten_middle("D:\\Unduhan", 25), "D:\\Unduhan");
    }

    #[test]
    fn library_texts() {
        assert_eq!(count_text(0, 0), "");
        assert_eq!(count_text(12, 12), "12 file");
        assert_eq!(count_text(3, 12), "3 dari 12 file");
        let item = library::Item {
            id: 1,
            path: PathBuf::from("/unduhan/Klip.mp4"),
            title: "Klip".into(),
            platform: Platform::Instagram,
            mode: DownloadMode::Video,
            format: "mp4".into(),
            quality: "1080p".into(),
            size: 45_200_000,
            duration: Some(205.4),
            downloaded_at: 0,
            source_url: String::new(),
            thumbnail: None,
        };
        assert_eq!(meta_text(&item), "Instagram · MP4 · 1080p · 45,2 MB · 3:25");
        let unknown = library::Item {
            quality: String::new(),
            size: 0,
            duration: None,
            ..item
        };
        assert_eq!(meta_text(&unknown), "Instagram · MP4");
    }

    #[test]
    fn megabytes_uses_comma() {
        assert_eq!(megabytes(0), "0,0 MB");
        assert_eq!(megabytes(12_340_000), "12,3 MB");
        assert_eq!(megabytes(109_000_000), "109,0 MB");
    }

    #[test]
    fn progress_text_and_fraction() {
        let (text, fraction) = describe_progress(&Progress::Downloading {
            package: Package::Ffmpeg,
            source: "gyan.dev".into(),
            received: 25_000_000,
            total: Some(100_000_000),
        });
        assert_eq!(
            text,
            "Mengunduh FFmpeg dari gyan.dev: 25,0 MB dari 100,0 MB"
        );
        assert!((fraction - 0.25).abs() < 1e-6);

        let (text, fraction) = describe_progress(&Progress::Downloading {
            package: Package::YtDlp,
            source: "GitHub".into(),
            received: 1_500_000,
            total: None,
        });
        assert_eq!(text, "Mengunduh yt-dlp dari GitHub: 1,5 MB");
        assert_eq!(fraction, 0.0);
    }
}
