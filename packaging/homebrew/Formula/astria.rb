class Astria < Formula
  desc "Knowledge graph builder for codebases"
  homepage "https://nodesify.github.io/astria/"
  url "https://registry.npmjs.org/@nodesify/astria/-/astria-1.0.11.tgz"
  sha256 "5eb98db5a556543b5e9f504858432c079c644400bdf7eeea3979cc2a794e71bf"
  license "MIT"
  version "1.0.11"

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
