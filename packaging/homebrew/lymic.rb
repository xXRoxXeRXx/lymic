cask "lymic" do
  version "0.23.0"
  sha256 "3e4cf4621e458e0e6b083b2a1d8e6abe4444a40467f8b866d3ad54e6a0468849"

  url "https://github.com/xXRoxXeRXx/lymic/releases/download/v#{version}/Lymic_#{version}_aarch64.dmg"
  name "Lymic"
  desc "Desktop synchronization client for Immich"
  homepage "https://github.com/xXRoxXeRXx/lymic"
  # License: MIT

  depends_on arch: :arm64

  app "Lymic.app"
end
