cask "yonder" do
  version "1.0.8"
  sha256 "8744c57f7310ae91ac2e6edf693920f7758434cc66aa8f66285f54a6e9871b09"

  url "https://github.com/RMoura98/yonder/releases/download/v#{version}/Yonder_#{version}_universal.dmg"
  name "Yonder"
  desc "Tiny, open-source break reminder for healthier screen habits"
  homepage "https://github.com/RMoura98/yonder"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on macos: ">= :catalina"

  app "Yonder.app"

  # Yonder isn't notarized yet. Clearing the quarantine flag lets macOS open it
  # without the "app is damaged / can't be opened" dialog. Remove this once
  # releases are signed and notarized.
  postflight do
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/Yonder.app"],
                   sudo: false
  end

  uninstall quit:       "app.yonder.desktop",
            launchctl: "Yonder"

  zap trash: [
    "~/Library/Application Support/app.yonder.desktop",
    "~/Library/Caches/app.yonder.desktop",
    "~/Library/LaunchAgents/Yonder.plist",
    "~/Library/Preferences/app.yonder.desktop.plist",
    "~/Library/Saved Application State/app.yonder.desktop.savedState",
    "~/Library/WebKit/app.yonder.desktop",
  ]
end
