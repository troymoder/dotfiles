{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.nvidia;
in {
  options.modules.nvidia = {
    enable = lib.mkEnableOption "nvidia GPU support";

    switchable = lib.mkEnableOption "switching the dGPU off and on with supergfxd (Hybrid/Integrated)";
  };

  config = lib.mkIf cfg.enable {
    hardware.nvidia = {
      open = true;
      package = config.boot.kernelPackages.nvidiaPackages.stable;
      nvidiaSettings = false; # X11 settings GUI; useless on Wayland
      modesetting.enable = true;
      # supergfxd must be able to unload the driver, which persistenced prevents.
      nvidiaPersistenced = !cfg.switchable;
    };

    services.xserver.videoDrivers = ["nvidia"];

    # supergfxctl -m Integrated removes the dGPU from the PCI bus (truly off);
    # -m Hybrid brings it back. Either switch takes effect at the next logout.
    services.supergfxd = lib.mkIf cfg.switchable {
      enable = true;
      settings = {
        mode = "Hybrid";
        vfio_enable = false;
        vfio_save = false;
        always_reboot = false;
        no_logind = false;
        logout_timeout_s = 180;
        hotplug_type = "None";
      };
    };

    # The driver runtime-suspends the dGPU (D3cold) when idle. Merely enumerating
    # the NVIDIA Vulkan ICD resumes it, which blocks every GTK4 / Chromium process
    # for ~2.3s on its first window even though they render on the iGPU. Hide the
    # ICD from the session by default; `nvidia-offload <cmd>` (or
    # `nvidia-offload %command%` in Steam launch options) opts a program back in.
    # Likewise glvnd tries EGL vendors in file order and NVIDIA sorts first, so
    # every EGL app (GTK4, Ghostty, Electron) would otherwise create its GL context
    # on the dGPU and keep it awake. List Mesa first; nvidia-offload reverses it.
    environment.sessionVariables = {
      VK_LOADER_DRIVERS_DISABLE = "nvidia*";
      __EGL_VENDOR_LIBRARY_FILENAMES = lib.concatStringsSep ":" [
        "/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json"
        "/run/opengl-driver/share/glvnd/egl_vendor.d/10_nvidia.json"
      ];
    };

    environment.systemPackages = [
      (pkgs.writeShellScriptBin "nvidia-offload" ''
        unset VK_LOADER_DRIVERS_DISABLE
        export __EGL_VENDOR_LIBRARY_FILENAMES=/run/opengl-driver/share/glvnd/egl_vendor.d/10_nvidia.json:/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json
        export __NV_PRIME_RENDER_OFFLOAD=1
        export __NV_PRIME_RENDER_OFFLOAD_PROVIDER=NVIDIA-G0
        export __GLX_VENDOR_LIBRARY_NAME=nvidia
        export __VK_LAYER_NV_optimus=NVIDIA_only
        exec "$@"
      '')
    ];
  };
}
