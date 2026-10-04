# The DIT cask (ADR 0029), published to the tap repository faridlab/homebrew-tap:
#
#   brew install --cask faridlab/tap/dit
#
# The release workflow fills in VERSION and SHA256 for each tagged release;
# this file in the repository is the template.
cask "dit" do
  version "VERSION"
  sha256 "SHA256"

  url "https://github.com/faridlab/dit-cli/releases/download/v#{version}/DIT-macos.zip"
  name "DIT"
  desc "Local-first project management kept as Markdown in git"
  homepage "https://github.com/faridlab/dit-cli"

  depends_on macos: :big_sur

  app "DIT.app"
  # The same binary the app runs, on PATH for terminals and AI agents.
  binary "#{appdir}/DIT.app/Contents/MacOS/dit"

  # DIT is not notarized yet (ADR 0029). Homebrew marks every download as
  # quarantined, and macOS refuses to open a quarantined app that Apple has
  # not notarized. The mark is cleared from DIT's own bundle while it is
  # still staged — after the sha256 above was checked, before the bundle is
  # moved to Applications, which copies its attributes as they then are.
  # This goes away with notarization.
  preflight_steps do
    run "/usr/bin/xattr",
        args:           ["-dr", "com.apple.quarantine", "DIT.app"],
        chdir:          ".",
        writable_paths: ["DIT.app"]
  end

  uninstall launchctl: "dev.dit.tray",
            quit:      "dev.dit.tray"

  # Workspaces are never touched: they are the person's repositories.
  zap trash: [
    "~/.config/dit",
    "~/Library/LaunchAgents/dev.dit.tray.plist",
  ]
end
