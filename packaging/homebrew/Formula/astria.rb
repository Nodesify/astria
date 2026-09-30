class Astria < Formula
  desc "Knowledge graph builder for codebases"
  homepage "https://nodesify.github.io/astria/"
  url "https://registry.npmjs.org/@nodesify/astria/-/astria-1.0.10.tgz"
  sha256 "aa5d22a4ba9545c437f31ce7dd265fe1f643f4f6357bc03db409266f169b60e2"
  license "MIT"
  version "1.0.10"

  # astria ships native napi-rs binaries through npm optionalDependencies and
  # needs Node >= 22 (see README).
  depends_on "node@22"

  def install
    system "npm", "install", *std_npm_args
    bin.install_symlink libexec.glob("node_modules/.bin/astria")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/astria --version")
  end
end
