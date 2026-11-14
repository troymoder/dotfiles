// SPDX-License-Identifier: GPL-3.0-only

//! Cabin: Troy's panels for COSMIC, laid out once and for all. One daemon
//! (`cabind`) holds the state and draws every panel on a layer-shell
//! overlay: the controls and the audio under the status icons, the date
//! menu under the clock, the System panel under its button. The panel
//! buttons (`cabin-applet --kind …`) are thin; `cabinctl` drives it all
//! from a shell.

pub mod app;
pub mod applet;
pub mod backend;
pub mod bus;
pub mod capture;
pub mod clock;
pub mod daemon;
pub mod switcher;
pub mod ui;
pub mod workspaces;

/// The status icons' button, and the app's own id.
pub const APP_ID: &str = "dev.troy.Cabin";
pub const CLOCK_ID: &str = "dev.troy.Cabin.Clock";
pub const SYSTEM_ID: &str = "dev.troy.Cabin.System";
pub const WORKSPACES_ID: &str = "dev.troy.Cabin.Workspaces";

/// Drawing on the integrated GPU, and never so much as opening the discrete
/// one. iced asks wgpu for the high-performance adapter unless told
/// otherwise, which on a hybrid laptop is the dGPU, kept awake (and drawing
/// power) for as long as the process lives; and the NVIDIA drivers open the
/// card just to list it. Whatever started us may have asked for the dGPU
/// (nvidia-offload, an editor), so this overrides rather than defaults.
/// Run first thing in `main`, before any thread.
pub fn prefer_integrated_gpu() {
    const MESA_EGL: &str = "/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json";
    let var = |key: &str| std::env::var(key).unwrap_or_default();
    let mut set: Vec<(&str, String)> = vec![("WGPU_POWER_PREF", "low".to_owned())];
    let mut unset: Vec<&str> = Vec::new();
    // Whatever started us may have asked for the dGPU (nvidia-offload, an
    // editor's environment): a select list wins over the disable list.
    let select: Vec<String> = var("VK_LOADER_DRIVERS_SELECT")
        .split(',')
        .filter(|s| !s.is_empty() && !s.contains("nvidia"))
        .map(str::to_owned)
        .collect();
    if select.is_empty() {
        unset.push("VK_LOADER_DRIVERS_SELECT");
    } else {
        set.push(("VK_LOADER_DRIVERS_SELECT", select.join(",")));
    }
    let disabled = var("VK_LOADER_DRIVERS_DISABLE");
    if !disabled.contains("nvidia") {
        set.push((
            "VK_LOADER_DRIVERS_DISABLE",
            if disabled.is_empty() {
                "*nvidia*".to_owned()
            } else {
                format!("{disabled},*nvidia*")
            },
        ));
    }
    // NVIDIA's implicit Vulkan layers load its driver into every Vulkan
    // process, its ICD disabled or not.
    let layers = var("VK_LOADER_LAYERS_DISABLE");
    if !layers.contains("VK_LAYER_NV") {
        set.push((
            "VK_LOADER_LAYERS_DISABLE",
            if layers.is_empty() {
                "VK_LAYER_NV_*".to_owned()
            } else {
                format!("{layers},VK_LAYER_NV_*")
            },
        ));
    }
    if std::path::Path::new(MESA_EGL).exists() {
        set.push(("__EGL_VENDOR_LIBRARY_FILENAMES", MESA_EGL.to_owned()));
    }
    unset.extend([
        "__NV_PRIME_RENDER_OFFLOAD",
        "__NV_PRIME_RENDER_OFFLOAD_PROVIDER",
        "__VK_LAYER_NV_optimus",
        "__GLX_VENDOR_LIBRARY_NAME",
        "DRI_PRIME",
    ]);
    // SAFETY: called at the very start of main, while the process has one
    // thread and nothing reads the environment.
    unsafe {
        for (key, value) in set {
            std::env::set_var(key, value);
        }
        for key in unset {
            std::env::remove_var(key);
        }
    }
}

/// Logging to stderr, which the panel or systemd passes on to the journal.
pub fn init_logging() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,cabin=info".into()),
        )
        .init();
}
