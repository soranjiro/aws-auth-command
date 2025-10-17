class Awx < Formula
  desc "AWS CLI authentication wrapper for seamless multi-account and MFA workflows"
  homepage "https://github.com/soranjiro/aws-auth-command"
  license "MIT"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/soranjiro/aws-auth-command/releases/download/v0.2.0/awx-aarch64-apple-darwin.tar.gz"
      sha256 "SHA256_MAC_ARM"
    else
      url "https://github.com/soranjiro/aws-auth-command/releases/download/v0.2.0/awx-x86_64-apple-darwin.tar.gz"
      sha256 "SHA256_MAC_INTEL"
    end
  end

  on_linux do
    if Hardware::CPU.arm?
      url "https://github.com/soranjiro/aws-auth-command/releases/download/v0.2.0/awx-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_LINUX_ARM"
    else
      url "https://github.com/soranjiro/aws-auth-command/releases/download/v0.2.0/awx-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "SHA256_LINUX_INTEL"
    end
  end

  def install
    bin.install "awx"
  end

  test do
    assert_match "awx", shell_output("#{bin}/awx --version")
  end
end
