# SPDX-License-Identifier: GPL-3.0-or-later
#
# The Homebrew cask of QuakeC Studio (P4-40), kept in quakecstudio/editor as
# packaging/homebrew/quakec-studio.rb. The release workflow fills in the version and the
# installer package's SHA-256 and writes the result to Casks/quakec-studio.rb in
# quakecstudio/homebrew-tap, so a change is made in the editor, never in the tap.
cask "quakec-studio" do
  version "@VERSION@"
  sha256 "@SHA256@"

  url "https://github.com/quakecstudio/editor/releases/download/v#{version}/quakec-studio-#{version}-macos-arm64.pkg",
      verified: "github.com/quakecstudio/editor/"
  name "QuakeC Studio"
  desc "IDE for QuakeC with its own compiler, live in-game preview and a built-in agent"
  homepage "https://www.quakecstudio.com/"

  depends_on arch: :arm64
  depends_on :macos

  pkg "quakec-studio-#{version}-macos-arm64.pkg"

  # The package's postinstall script makes the two links, so its receipt does not list them.
  uninstall pkgutil: "com.quakecstudio.QuakeCStudio",
            delete:  [
              "/usr/local/bin/quakec-ide",
              "/usr/local/bin/quakec-lsp",
            ]

  zap trash: [
    "~/Library/Application Support/QuakeC Studio",
    "~/Library/Caches/QuakeC Studio",
    "~/Library/Logs/QuakeC Studio",
  ]
end
