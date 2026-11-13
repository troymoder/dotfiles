# cabin-ec: the USB-C charger as the Framework EC sees it, on the system bus,
# so the cabin Battery menu can say "240 W charger" where the kernel stops at
# the standard 100 W profiles. It runs as root for /dev/cros_ec, so it is
# kept to that one device and to answering ChargerWatts.
{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.modules.cabin-ec;
  cabin-ec = pkgs.callPackage ./cabin-ec/package.nix {};
  busName = "dev.troy.Cabin.EC";
in {
  options.modules.cabin-ec.enable = lib.mkEnableOption "cabin-ec, the Framework EC's charger readings for the cabin panels";

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [cabin-ec];

    # Only root owns the name; anyone may ask it, and only through its interface.
    services.dbus.packages = [
      (pkgs.writeTextDir "share/dbus-1/system.d/${busName}.conf" ''
        <!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-BUS Bus Configuration 1.0//EN"
          "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
        <busconfig>
          <policy user="root">
            <allow own="${busName}"/>
          </policy>
          <policy context="default">
            <allow send_destination="${busName}" send_interface="dev.troy.Cabin.EC1"/>
            <allow send_destination="${busName}" send_interface="org.freedesktop.DBus.Introspectable"/>
            <allow send_destination="${busName}" send_interface="org.freedesktop.DBus.Peer"/>
          </policy>
        </busconfig>
      '')
    ];

    # A device unit for the EC, so the service starts once it's there:
    # DeviceAllow below is looked up as the service starts.
    services.udev.extraRules = ''
      SUBSYSTEM=="misc", KERNEL=="cros_ec", TAG+="systemd"
    '';

    systemd.services.cabin-ec = {
      description = "Cabin: the Framework EC's charger readings";
      wantedBy = ["dev-cros_ec.device"];
      after = ["dev-cros_ec.device"];
      serviceConfig = {
        Type = "dbus";
        BusName = busName;
        ExecStart = lib.getExe cabin-ec;
        Restart = "on-failure";
        RestartSec = 5;

        # Root for the device's owner bits, and nothing else: no capabilities,
        # no other devices, no network, a read-only system.
        CapabilityBoundingSet = "";
        NoNewPrivileges = true;
        DevicePolicy = "closed";
        DeviceAllow = ["/dev/cros_ec rw"];
        ProtectSystem = "strict";
        ProtectHome = true;
        PrivateTmp = true;
        PrivateNetwork = true;
        RestrictAddressFamilies = ["AF_UNIX"];
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        ProtectProc = "invisible";
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = ["@system-service"];
        SystemCallErrorNumber = "EPERM";
      };
    };
  };
}
