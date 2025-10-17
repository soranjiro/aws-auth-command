const { join } = require('path');
const { existsSync } = require('fs');

const binPath = join(__dirname, '..', 'bin', 'awx');

if (existsSync(binPath)) {
  console.log('\n✓ awx has been installed successfully!');
  console.log('\nYou can now run: awx --version');
  console.log('\nFor usage instructions, run: awx --help');
} else {
  console.error('\n✗ Installation failed. Binary not found.');
  process.exit(1);
}
