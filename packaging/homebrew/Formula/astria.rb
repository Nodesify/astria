class Astria < Formula
  desc "Knowledge graph builder for codebases"
  homepage "https://nodesify.github.io/astria/"
  url "https://registry.npmjs.org/@nodesify/astria/-/astria-1.1.1.tgz"
  sha256 "f6d1cd8b4f1e804ec1dc043dff9925d984b1aa68645394d656ec3b108de4c144"
  license "MIT"
  version "1.1.1"

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
