// Downloads a pinned PDFium build (bblanchon/pdfium-binaries) for the current
// platform into src-tauri/resources/pdfium and verifies its SHA-256.
// Usage: node scripts/fetch-pdfium.mjs [target]   e.g. linux-x64, win-x64
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

// Pin version and checksums together. To bump: change TAG and recompute all hashes.
const TAG = "chromium/8152"; // PDFium 159.0.8152.0
const TARGETS = {
  "linux-x64": { sha256: "384ab5c51882408a143a21f9c14dcd2caf1f75280ae41c554d0489b46c3dba9f", lib: "lib/libpdfium.so" },
  "linux-arm64": { sha256: "c6ed1a91234aede0cfafdc09c401b83bd44ac69deafc4599fda75d511c36403e", lib: "lib/libpdfium.so" },
  "win-x64": { sha256: "cee2b0e9990bc67c0721774932961b9dbf2766046c10eec925acd4cfba6fdfe3", lib: "bin/pdfium.dll" },
  "win-arm64": { sha256: "8614b10c6a780acc3ae9fdd425d6b61f9d22ae84301b19e1ac82f573930c217c", lib: "bin/pdfium.dll" },
  // macOS: development only (Linux and Windows are the release targets).
  "mac-x64": { sha256: "acb975e637ffa1f41430205c62baacac70e34b60811e22fc0e0274dfcd601fcc", lib: "lib/libpdfium.dylib" },
  "mac-arm64": { sha256: "30b0c3f55448840f0947b54bedbe35185bbaccbadd35b8cd960270d6fa6caf80", lib: "lib/libpdfium.dylib" },
};

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const dest = join(root, "src-tauri", "resources", "pdfium");

function detect() {
  const os = { win32: "win", linux: "linux", darwin: "mac" }[process.platform] ?? null;
  const arch = process.arch === "x64" ? "x64" : process.arch === "arm64" ? "arm64" : null;
  if (!os || !arch) throw new Error(`Unsupported platform ${process.platform}/${process.arch}`);
  return `${os}-${arch}`;
}

const target = process.argv[2] ?? detect();
const info = TARGETS[target];
if (!info) throw new Error(`Unknown target "${target}". Known: ${Object.keys(TARGETS).join(", ")}`);

const libName = info.lib.split("/").pop();
if (existsSync(join(dest, libName)) && existsSync(join(dest, "PDFIUM_VERSION")) &&
    readFileSync(join(dest, "PDFIUM_VERSION"), "utf8").trim() === `${TAG} ${target}`) {
  console.log(`PDFium ${TAG} (${target}) already present.`);
  process.exit(0);
}

const url = `https://github.com/bblanchon/pdfium-binaries/releases/download/${encodeURIComponent(TAG)}/pdfium-${target}.tgz`;
console.log(`Downloading ${url}`);
const res = await fetch(url);
if (!res.ok) throw new Error(`Download failed: HTTP ${res.status}`);
const data = Buffer.from(await res.arrayBuffer());

const hash = createHash("sha256").update(data).digest("hex");
if (hash !== info.sha256) {
  throw new Error(`Checksum mismatch for ${target}: expected ${info.sha256}, got ${hash}`);
}

const tmp = mkdtempSync(join(tmpdir(), "pdfium-"));
try {
  const archive = join(tmp, "pdfium.tgz");
  writeFileSync(archive, data);
  // bsdtar (Windows 10+) and GNU tar both understand this.
  execFileSync("tar", ["-xzf", archive, "-C", tmp, info.lib, "LICENSE", "VERSION"], { stdio: "inherit" });
  mkdirSync(dest, { recursive: true });
  copyFileSync(join(tmp, info.lib), join(dest, libName));
  copyFileSync(join(tmp, "LICENSE"), join(dest, "PDFIUM-LICENSE"));
  writeFileSync(join(dest, "PDFIUM_VERSION"), `${TAG} ${target}\n`);
} finally {
  rmSync(tmp, { recursive: true, force: true });
}
console.log(`PDFium ${TAG} (${target}) installed in ${dest}`);
