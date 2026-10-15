// One version for binaries, installers, archives and release tags.
import fs from 'node:fs';

const root = new URL('../', import.meta.url);
const cargo = fs.readFileSync(new URL('backend/Cargo.toml', root), 'utf8');
const version = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const frontend = JSON.parse(fs.readFileSync(new URL('frontend/package.json', root), 'utf8'));
if (!version || !/^\d+\.\d+\.\d+$/.test(version) || frontend.version !== version) {
  throw new Error('Use the same numeric major.minor.patch version in backend/Cargo.toml and frontend/package.json.');
}
if (process.env.GITHUB_REF_TYPE === 'tag' && process.env.GITHUB_REF_NAME !== `v${version}`) {
  throw new Error(`Release tag must be v${version}, got ${process.env.GITHUB_REF_NAME}`);
}
console.log(version);
