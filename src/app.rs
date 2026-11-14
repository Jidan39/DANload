//! Penghubung antara tampilan (Slint) dan logika (config, deps, setup, downloader).
//!
//! Aturan penting: komponen UI hanya boleh disentuh dari UI thread.
//! Pekerjaan lambat (menjalankan yt-dlp/FFmpeg, mengunduh) dikerjakan di thread lain,
//! lalu hasilnya dikirim balik lewat `upgrade_in_event_loop`.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, VecModel, Weak};

use crate::config::{self, Config, DownloadMode};
use crate::deps::{self, Component, Status};
use crate::downloader::{self, CancelHandle, Outcome, Request, Tools, Update};
use crate::formats::{self, OutputFormat};
use crate::platform::{self, Platform};
use crate::process;
use crate::setup::{self, InstallError, Package, Progress};
use crate::{
    AppWindow, ComponentInfo, ComponentState, DownloadItem, DownloadState, UrlFeedback, WizardState,
};

/// Batas waktu menunggu unduhan berhenti saat aplikasi ditutup.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

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
    /// Unduhan yang sedang berjalan (Fase 3: paling banyak satu).
    active_download: RefCell<Option<ActiveDownload>>,
    next_download_id: Cell<i32>,
}

/// Data satu unduhan yang perlu diingat (untuk "Coba lagi" dan "Tampilkan di folder").
struct DownloadEntry {
    id: i32,
    request: Request,
    /// Lokasi file hasil, setelah berhasil.
    file: Option<PathBuf>,
}

/// Unduhan yang sedang berjalan.
struct ActiveDownload {
    id: i32,
    cancel: CancelHandle,
    /// `true` setelah tombol Batal ditekan; laporan kemajuan berikutnya diabaikan.
    cancel_requested: bool,
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
    let state = Rc::new(AppState {
        config: RefCell::new(Config::load_from(&config_path)),
        config_path,
        install_job: Cell::new(0),
        install_cancel: RefCell::new(Arc::new(AtomicBool::new(false))),
        first_check_handled: Cell::new(false),
        component_paths: RefCell::new(vec![None; Component::ALL.len()]),
        download_model: Rc::new(VecModel::default()),
        download_entries: RefCell::new(Vec::new()),
        active_download: RefCell::new(None),
        next_download_id: Cell::new(1),
    });
    STATE.with(|slot| *slot.borrow_mut() = Some(Rc::clone(&state)));

    {
        let config = state.config.borrow();
        // Nomor versi diambil dari Cargo.toml, jadi cukup diubah di satu tempat.
        app.set_app_version(env!("CARGO_PKG_VERSION").into());
        app.set_download_mode(config.download_mode.index());
        app.set_separate_subfolders(config.separate_subfolders);
        show_download_folder(app, &config.effective_download_folder());

        app.set_video_quality_options(string_model(formats::video_height_labels()));
        app.set_audio_bitrate_options(string_model(formats::audio_bitrate_labels()));
        app.set_video_quality_index(to_index(formats::video_height_index(
            config.video_max_height,
        )));
        app.set_audio_bitrate_index(to_index(formats::audio_bitrate_index(
            config.audio_bitrate_kbps,
        )));
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
    connect_wizard(app, &state);

    start_component_check(app);
}

/// Dipanggil setelah jendela ditutup: pastikan tidak ada yt-dlp/FFmpeg yang tertinggal.
pub fn shutdown() {
    if let Some(cancel) = current_state().and_then(|state| active_cancel_handle(&state)) {
        cancel.cancel_and_wait(SHUTDOWN_TIMEOUT);
    }
}

/// Penangan aksi di jendela utama dan halaman Pengaturan.
fn connect_main_window(app: &AppWindow, state: &Rc<AppState>) {
    // Platform yang dipilih selalu dimulai dari YouTube (bawaan, lihat CLAUDE.md 2.3).
    app.set_platform(0);

    app.on_check_url(|platform_index, url| {
        let (valid, message) = platform::feedback(Platform::from_index(platform_index), &url);
        UrlFeedback {
            valid,
            message: message.into(),
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

/// Penangan aksi unduhan: pilihan kualitas, tombol Unduh, Batal, Coba lagi,
/// Tampilkan di folder, dan konfirmasi saat jendela ditutup di tengah unduhan.
fn connect_downloads(app: &AppWindow, state: &Rc<AppState>) {
    app.on_video_quality_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |index| {
            let height = usize::try_from(index)
                .ok()
                .and_then(|index| formats::VIDEO_HEIGHTS.get(index).copied())
                .unwrap_or(None);
            state.config.borrow_mut().video_max_height = height;
            save_config(&weak, &state);
        }
    });

    app.on_audio_bitrate_changed({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |index| {
            let bitrate = usize::try_from(index)
                .ok()
                .and_then(|index| formats::AUDIO_BITRATES.get(index).copied())
                .unwrap_or(formats::DEFAULT_AUDIO_BITRATE);
            state.config.borrow_mut().audio_bitrate_kbps = bitrate;
            save_config(&weak, &state);
        }
    });

    app.on_start_download({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            if let Some(app) = weak.upgrade() {
                start_download(&app, &state);
            }
        }
    });

    app.on_cancel_download({
        let state = Rc::clone(state);
        move |id| cancel_download(&state, id)
    });

    app.on_retry_download({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move |id| {
            if let Some(app) = weak.upgrade() {
                retry_download(&app, &state, id);
            }
        }
    });

    app.on_show_in_folder({
        let state = Rc::clone(state);
        move |id| {
            let entries = state.download_entries.borrow();
            if let Some(entry) = entries.iter().find(|entry| entry.id == id) {
                let target = entry.file.as_deref().unwrap_or(&entry.request.folder);
                process::show_in_file_manager(target);
            }
        }
    });

    // Menutup jendela saat unduhan berjalan: tanya dulu, lalu hentikan unduhannya dengan rapi.
    app.window().on_close_requested({
        let weak = app.as_weak();
        let state = Rc::clone(state);
        move || {
            let Some(cancel) = active_cancel_handle(&state) else {
                return CloseRequestResponse::HideWindow;
            };
            let Some(app) = weak.upgrade() else {
                return CloseRequestResponse::HideWindow;
            };
            let handle = app.window().window_handle();
            let answer = rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Warning)
                .set_title("DANload")
                .set_description("Unduhan masih berjalan. Hentikan unduhan dan tutup DANload?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .set_parent(&handle)
                .show();
            if answer == rfd::MessageDialogResult::Yes {
                cancel.cancel_and_wait(SHUTDOWN_TIMEOUT);
                CloseRequestResponse::HideWindow
            } else {
                CloseRequestResponse::KeepWindowShown
            }
        }
    });
}

/// Penanda batal unduhan yang sedang berjalan, jika ada.
fn active_cancel_handle(state: &AppState) -> Option<CancelHandle> {
    state
        .active_download
        .borrow()
        .as_ref()
        .map(|active| active.cancel.clone())
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
        ffmpeg_dir: path_of(Component::Ffmpeg)?.parent()?.to_path_buf(),
        deno: path_of(Component::Deno)?,
    })
}

/// Tombol Unduh ditekan.
fn start_download(app: &AppWindow, state: &Rc<AppState>) {
    if state.active_download.borrow().is_some() {
        return;
    }
    let selected = Platform::from_index(app.get_platform());
    let url = app.get_url().trim().to_string();
    // Diperiksa lagi di sini, walaupun tombolnya sudah dimatikan di UI untuk kasus ini.
    if !platform::feedback(selected, &url).0 {
        return;
    }

    let mode = DownloadMode::from_index(app.get_download_mode());
    let request = {
        let config = state.config.borrow();
        let format = match mode {
            DownloadMode::Video => OutputFormat::Mp4 {
                max_height: config.video_max_height,
            },
            DownloadMode::Audio => OutputFormat::Mp3 {
                bitrate_kbps: config.audio_bitrate_kbps,
            },
        };
        Request {
            url,
            format,
            folder: config.target_folder(mode),
        }
    };

    let id = state.next_download_id.get();
    state.next_download_id.set(id + 1);
    // Unduhan terbaru tampil paling atas.
    state.download_model.insert(
        0,
        DownloadItem {
            id,
            title: request.url.clone().into(),
            subtitle: format!("{} · {}", selected.name(), request.format.label()).into(),
            state: DownloadState::Preparing,
            progress: 0.0,
            detail: "".into(),
        },
    );
    state.download_entries.borrow_mut().push(DownloadEntry {
        id,
        request: request.clone(),
        file: None,
    });
    launch_download(app, state, id, request);
}

/// Tombol "Coba lagi" pada unduhan yang gagal atau dibatalkan.
fn retry_download(app: &AppWindow, state: &Rc<AppState>, id: i32) {
    if state.active_download.borrow().is_some() {
        return;
    }
    let request = {
        let mut entries = state.download_entries.borrow_mut();
        let Some(entry) = entries.iter_mut().find(|entry| entry.id == id) else {
            return;
        };
        entry.file = None;
        entry.request.clone()
    };
    launch_download(app, state, id, request);
}

/// Menjalankan unduhan di thread terpisah dan menyambungkan laporannya ke kartu `id`.
fn launch_download(app: &AppWindow, state: &Rc<AppState>, id: i32, request: Request) {
    let Some(tools) = current_tools(state) else {
        update_card(state, id, |item| {
            item.state = DownloadState::Failed;
            item.detail = "Komponen belum lengkap. Buka Pengaturan untuk memasangnya.".into();
        });
        return;
    };
    update_card(state, id, |item| {
        item.state = DownloadState::Preparing;
        item.progress = 0.0;
        item.detail = "Mengambil info video...".into();
    });

    let cancel = CancelHandle::default();
    *state.active_download.borrow_mut() = Some(ActiveDownload {
        id,
        cancel: cancel.clone(),
        cancel_requested: false,
    });
    app.set_downloading(true);
    app.set_current_page(0);

    let weak = app.as_weak();
    let job_id = u64::try_from(id).unwrap_or_default();
    std::thread::spawn(move || {
        let outcome = downloader::run(&request, &tools, job_id, &cancel, &mut |update| {
            let _ = weak.upgrade_in_event_loop(move |_app| apply_download_update(id, update));
        });
        let _ = weak.upgrade_in_event_loop(move |app| finish_download(&app, id, outcome));
    });
}

/// Menampilkan satu laporan kemajuan di kartu `id`. Dipanggil di UI thread.
fn apply_download_update(id: i32, update: Update) {
    let Some(state) = current_state() else { return };
    let ignored = state
        .active_download
        .borrow()
        .as_ref()
        .is_none_or(|active| active.id != id || active.cancel_requested);
    if ignored {
        return;
    }
    update_card(&state, id, |item| match update {
        Update::Title(title) => {
            item.title = title.into();
            item.detail = "Menyiapkan unduhan...".into();
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
            item.detail = text.into();
        }
        Update::Processing(step) => {
            item.state = DownloadState::Processing;
            item.progress = 1.0;
            item.detail = downloader::describe_step(step).into();
        }
    });
}

/// Unduhan `id` selesai (berhasil, gagal, atau dibatalkan). Dipanggil di UI thread.
fn finish_download(app: &AppWindow, id: i32, outcome: Outcome) {
    let Some(state) = current_state() else { return };
    let is_active = state
        .active_download
        .borrow()
        .as_ref()
        .is_some_and(|active| active.id == id);
    if is_active {
        *state.active_download.borrow_mut() = None;
    }
    app.set_downloading(state.active_download.borrow().is_some());

    let file = match &outcome {
        Outcome::Done { file } => file.clone(),
        Outcome::Failed(_) | Outcome::Cancelled => None,
    };
    if let Some(entry) = state
        .download_entries
        .borrow_mut()
        .iter_mut()
        .find(|entry| entry.id == id)
    {
        entry.file = file.clone();
    }
    update_card(&state, id, |item| match outcome {
        Outcome::Done { .. } => {
            item.state = DownloadState::Done;
            item.progress = 1.0;
            item.detail = match file.as_deref().and_then(Path::file_name) {
                Some(name) => format!("Selesai · {}", name.to_string_lossy()).into(),
                None => "Selesai.".into(),
            };
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
}

/// Tombol Batal pada kartu `id`.
fn cancel_download(state: &AppState, id: i32) {
    let cancel = {
        let mut active = state.active_download.borrow_mut();
        match active.as_mut() {
            Some(active) if active.id == id && !active.cancel_requested => {
                active.cancel_requested = true;
                active.cancel.clone()
            }
            _ => return,
        }
    };
    cancel.cancel();
    update_card(state, id, |item| item.detail = "Membatalkan...".into());
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
