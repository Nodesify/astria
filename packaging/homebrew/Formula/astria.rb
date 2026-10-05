class Astria < Formula
  desc "Knowledge graph builder for codebases"
  homepage "https://nodesify.github.io/astria/"
  url "https://registry.npmjs.org/@nodesify/astria/-/astria-1.1.0.tgz"
  sha256 "e250946f5f661a8e5edee4e573608b5807dc5d7599dd2df14e49bdf87a9ca85d"
  license "MIT"
  version "1.1.0"

  # astria ships native napi-rs binaries through npm optionalDependencies and
  # needs Node >= 22 (see README).
  depends_on "node@22"

  def install
    # std_npm_args installs global-style into libexec: the package lands at
    # libexec/lib/node_modules and npm links its executables at libexec/bin —
    # libexec/node_modules/.bin never exists (that layout is local-install
    # only), and globbing it in 1.0.10 installed no astria command at all.
    system "npm", "install", *std_npm_args
    bin.install_symlink libexec.glob("bin/*")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/astria --version")
  end
end
