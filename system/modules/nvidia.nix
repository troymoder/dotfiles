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
  };

  config = lib.mkIf cfg.enable {
    hardware.nvidia = {
      open = true;
      package = config.boot.kernelPackages.nvidiaPackages.stable;
      nvidiaSettings = true;
      modesetting.enable = true;
      nvidiaPersistenced = true;
    };

    services.xserver.videoDrivers = ["nvidia"];

    # The driver runtime-suspends the dGPU (D3cold) when idle. Merely enumerating
    # the NVIDIA Vulkan ICD resumes it, which blocks every GTK4 / Chromium process
    # for ~2.3s on its first window even though they render on the iGPU. Hide the
    # ICD from the session by default; `nvidia-offload <cmd>` (or
    # `nvidia-offload %command%` in Steam launch options) opts a program back in.
    environment.sessionVariables.VK_LOADER_DRIVERS_DISABLE = "nvidia*";

    environment.systemPackages = [
      (pkgs.writeShellScriptBin "nvidia-offload" ''
        unset VK_LOADER_DRIVERS_DISABLE
        export __NV_PRIME_RENDER_OFFLOAD=1
        export __NV_PRIME_RENDER_OFFLOAD_PROVIDER=NVIDIA-G0
        export __GLX_VENDOR_LIBRARY_NAME=nvidia
        export __VK_LAYER_NV_optimus=NVIDIA_only
        exec "$@"
      '')
    ];
  };
}
