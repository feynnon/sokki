# Home-manager module for Sokki speech-to-text
#
# Provides a systemd user service for autostart.
# Usage: imports = [ sokki.homeManagerModules.default ];
#        services.sokki.enable = true;
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.sokki;
in
{
  options.services.sokki = {
    enable = lib.mkEnableOption "Sokki speech-to-text user service";

    package = lib.mkOption {
      type = lib.types.package;
      defaultText = lib.literalExpression "sokki.packages.\${system}.sokki";
      description = "The Sokki package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.sokki = {
      Unit = {
        Description = "Sokki speech-to-text";
        After = [ "graphical-session.target" ];
        PartOf = [ "graphical-session.target" ];
      };
      Service = {
        ExecStart = "${cfg.package}/bin/sokki";
        Restart = "on-failure";
        RestartSec = 5;
      };
      Install.WantedBy = [ "graphical-session.target" ];
    };
  };
}
