class Astria < Formula
  desc "Knowledge graph builder for codebases"
  homepage "https://nodesify.github.io/astria/"
  url "https://registry.npmjs.org/@nodesify/astria/-/astria-1.0.8.tgz"
  sha256 "507c16bc61377b6e81f3eec42a2cd7b93492a9b10d04e8bdd351f4337b490f72"
  license "MIT"
  version "1.0.8"

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
