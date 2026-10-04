class Astria < Formula
  desc "Knowledge graph builder for codebases"
  homepage "https://nodesify.github.io/astria/"
  url "https://registry.npmjs.org/@nodesify/astria/-/astria-1.0.12.tgz"
  sha256 "8127138955bf7b11caccc0ef64652e03b755e22b6cea59e5de065f3e4373254b"
  license "MIT"
  version "1.0.12"

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
