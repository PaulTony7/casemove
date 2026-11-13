// Generates tests/fixtures/conf-encrypted.bin — a store file encrypted with
// the exact algorithm electron-store (conf v10) uses, for the Rust migration
// tests. Run from this directory: node generate.js
const crypto = require('crypto');
const fs = require('fs');

const plain = JSON.stringify({
  fastmove: false,
  os: 'linux',
  devmode: false,
  hasSeenInfoModal: true,
  pricing: { currency: 'USD' },
  account: {
    alice: {
      displayName: 'Alice',
      imageURL: 'https://example.org/avatar.jpg',
      position: 0,
      // safeStorage blobs are latin1-encoded buffers; simulate high bytes
      refreshToken: 'FAKE_TOKEN_latin1_\u00ff\u00fe\u0080end',
    },
    bob: {
      displayName: 'Bob',
      position: 1,
      safeData: 'FAKE_SAFEDATA_\u00ff\u0081end',
    },
    carol: {
      displayName: 'Carol',
      position: 2,
    },
  },
});

const iv = crypto.randomBytes(16);
const key = crypto.pbkdf2Sync('this_only_obfuscates', iv.toString(), 10000, 32, 'sha512');
const cipher = crypto.createCipheriv('aes-256-cbc', key, iv);
const ciphertext = Buffer.concat([cipher.update(plain, 'utf8'), cipher.final()]);

fs.writeFileSync(
  __dirname + '/conf-encrypted.bin',
  Buffer.concat([iv, Buffer.from(':'), ciphertext])
);
console.log('wrote conf-encrypted.bin');
