//! Penghubung antara tampilan (Slint) dan logika (config, deps).
//!
//! Aturan penting: komponen UI hanya boleh disentuh dari UI thread.
//! Pekerjaan lambat (menjalankan yt-dlp/FFmpeg) dikerjakan di thread lain,
//! lalu hasilnya dikirim balik lewat `upgrade_in_event_loop`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use slint::{ComponentHandle, Model, ModelRc, VecModel, Weak};

use crate::config::{self, Config, DownloadMode};
use crate::deps::{self, Component, Status};
use crate::{AppWindow, ComponentInfo, ComponentState};

/// Mengisi data awal ke jendela dan memasang semua penangan aksi pengguna.
pub fn setup(app: &AppWindow) {
    let config_path = config::config_file_path();
    // `Rc<RefCell<..>>`: config dipakai bersama oleh beberapa closure di UI thread.
    let config = Rc::new(RefCell::new(Config::load_from(&config_path)));

    // Nomor versi diambil dari Cargo.toml, jadi cukup diubah di satu tempat.
    app.set_app_version(env!("CARGO_PKG_VERSION").into());
    app.set_download_mode(config.borrow().download_mode.index());
    show_download_folder(app, &config.borrow().effective_download_folder());

    // Daftar komponen; awalnya semua berstatus "sedang diperiksa".
    let rows: Vec<ComponentInfo> = Component::ALL.iter().map(|c| checking_row(*c)).collect();
    app.set_components(ModelRc::from(Rc::new(VecModel::from(rows))));

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
        let config = Rc::clone(&config);
        let config_path = config_path.clone();
        move |index| {
            config.borrow_mut().download_mode = DownloadMode::from_index(index);
            save_config(&weak, &config.borrow(), &config_path);
        }
    });

    app.on_choose_download_folder({
        let weak = app.as_weak();
        let config = Rc::clone(&config);
        move || {
            let Some(app) = weak.upgrade() else { return };
            // Ambil folder awal dulu, supaya config tidak sedang "dipinjam"
            // selama dialog terbuka.
            let current = config.borrow().effective_download_folder();
            let Some(chosen) = pick_folder(&app, &current) else {
                return; // Pengguna menekan Batal.
            };
            config.borrow_mut().download_folder = Some(chosen.clone());
            show_download_folder(&app, &chosen);
            save_config(&weak, &config.borrow(), &config_path);
        }
    });

    start_component_check(app);
}

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

    let states: Vec<ComponentState> = model.iter().map(|row| row.state).collect();
    let still_checking = states.contains(&ComponentState::Checking);
    app.set_checking_components(still_checking);
    app.set_components_ready(states.iter().all(|state| *state == ComponentState::Found));
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

fn show_download_folder(app: &AppWindow, folder: &Path) {
    app.set_download_folder(folder.display().to_string().into());
}

/// Menyimpan config. Kalau gagal, pesannya ditampilkan di status bar.
fn save_config(weak: &Weak<AppWindow>, config: &Config, path: &Path) {
    let Some(app) = weak.upgrade() else { return };
    match config.save_to(path) {
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
    dialog = dialog.set_parent(&handle);

    dialog.pick_folder()
}
