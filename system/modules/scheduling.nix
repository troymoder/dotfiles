{
  config,
  lib,
  ...
}: let
  cfg = config.modules.scheduling;
in {
  options.modules.scheduling = {
    enable = lib.mkEnableOption "desktop-first CPU scheduling (sched_ext lavd, idle nix builds)";
  };

  config = lib.mkIf cfg.enable {
    # Per-task scheduler that favours latency-critical tasks (compositor, input,
    # render threads) over CPU hogs, by behaviour. It ignores cgroup weights, so
    # system76-scheduler's nice rules (compilers -> 19, compositor -> -3) apply
    # system-wide instead of only within each app's scope.
    services.scx-loader = {
      enable = true;
      config.default_sched = "scx_lavd";
    };

    # Builds only get CPU/IO nobody else wants.
    nix.daemonCPUSchedPolicy = "idle";
    nix.daemonIOSchedClass = "idle";

    services.system76-scheduler.exceptions = [
      # greetd is a system service (nice 12) and the whole session inherits it.
      "greetd"
      # Keep the SCHED_IDLE set above instead of the system-services profile.
      "nix-daemon"
      "include descends=\"nix-daemon\""
    ];
  };
}
