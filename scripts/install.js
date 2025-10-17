const { execSync } = require('child_process');
const { existsSync, mkdirSync, chmodSync } = require('fs');
const { join } = require('path');
const https = require('https');
const { createWriteStream } = require('fs');

const version = require('../package.json').version;
const platform = process.platform;
const arch = process.arch;

// Map Node.js arch to Rust target
const archMap = {
  x64: 'x86_64',
  arm64: 'aarch64'
};

const rustArch = archMap[arch];
if (!rustArch) {
  console.error(`Unsupported architecture: ${arch}`);
  process.exit(1);
}

// Map platform to Rust target
const targetMap = {
  darwin: `${rustArch}-apple-darwin`,
  linux: `${rustArch}-unknown-linux-gnu`
};

const target = targetMap[platform];
if (!target) {
  console.error(`Unsupported platform: ${platform}`);
  process.exit(1);
}

const binDir = join(__dirname, '..', 'bin');
const binPath = join(binDir, 'awx');

// Create bin directory if it doesn't exist
if (!existsSync(binDir)) {
  mkdirSync(binDir, { recursive: true });
}

console.log(`Installing awx for ${platform}-${arch}...`);

// Helper to download with redirect support
function download(url, dest, cb) {
  const file = createWriteStream(dest);
  const handle = (link) => {
    https.get(link, (res) => {
      // Follow redirects
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        file.close();
        return handle(res.headers.location);
      }
      if (res.statusCode !== 200) {
        file.close();
        return cb(new Error(`HTTP ${res.statusCode} when downloading ${link}`));
      }
      res.pipe(file);
      file.on('finish', () => {
        file.close();
        try { chmodSync(dest, 0o755); } catch (_) {}
        cb(null);
      });
    }).on('error', (err) => {
      try { file.close(); } catch (_) {}
      cb(err);
    });
  };
  handle(url);
}

// Prefer downloading prebuilt binaries by default
const preferBuild = process.env.AWX_BUILD_FROM_SOURCE === '1';
const assetName = `awx-${target}`;
const releaseUrl = `https://github.com/soranjiro/aws-auth-command/releases/download/v${version}/${assetName}.tar.gz`;

const tryBuildFromSource = () => {
  try {
    // Build only if cargo is available and explicitly requested
    if (!preferBuild) throw new Error('Build not requested');
    execSync('cargo --version', { stdio: 'ignore' });
    console.log('Building from source...');
    execSync('cargo build --release', { cwd: join(__dirname, '..'), stdio: 'inherit' });
    const sourceBin = join(__dirname, '..', 'target', 'release', 'awx');
    execSync(`cp "${sourceBin}" "${binPath}"`, { stdio: 'inherit' });
    chmodSync(binPath, 0o755);
    console.log('✓ Successfully built and installed awx');
  } catch (error) {
    console.error('Failed to install awx without Rust.');
    console.error('Prebuilt binary not available or build not permitted.');
    console.error('If you have Rust installed and want to build from source, run:');
    console.error('  AWX_BUILD_FROM_SOURCE=1 npm install -g @soranjiro/awx');
    process.exit(1);
  }
};

console.log(`Attempting to download prebuilt binary: ${assetName}`);
// Download tar.gz then extract single binary
const tmpTar = join(binDir, `${assetName}.tar.gz`);
download(releaseUrl, tmpTar, (err) => {
  if (err) {
    console.error(`Download failed: ${err.message}`);
    return tryBuildFromSource();
  }
  try {
    // Extract using system tar
    execSync(`tar -xzf "${tmpTar}" -C "${binDir}"`);
    // Ensure binary name is 'awx' (tar should contain 'awx')
    chmodSync(binPath, 0o755);
    console.log('✓ Successfully downloaded and installed awx');
  } catch (e) {
    console.error('Extraction failed:', e.message);
    tryBuildFromSource();
  }
});
