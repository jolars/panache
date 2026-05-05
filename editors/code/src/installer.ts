import * as fs from "node:fs/promises";
import * as path from "node:path";
import * as https from "node:https";
import AdmZip from "adm-zip";
import * as tar from "tar";

interface ReleaseAsset {
  name: string;
  browser_download_url: string;
}

interface ReleaseResponse {
  tag_name: string;
  assets: ReleaseAsset[];
  prerelease?: boolean;
  draft?: boolean;
}

interface TargetAsset {
  archiveNames: string[];
  binaryName: string;
}

const HTTP_TIMEOUT_MS = 15_000;
const MAX_REDIRECTS = 5;
const DOWNLOAD_RETRIES = 4;
const RETRY_DELAY_MS = 1_500;

function isCandidatePanacheCliReleaseTag(tag: string): boolean {
  return /^panache-v\d+\.\d+\.\d+/.test(tag) || /^v\d+\.\d+\.\d+/.test(tag);
}

function detectTargetAsset(): TargetAsset {
  const binaryName = process.platform === "win32" ? "panache.exe" : "panache";
  if (process.platform === "darwin" && process.arch === "arm64") {
    return { archiveNames: ["panache-aarch64-apple-darwin.tar.gz"], binaryName };
  }
  if (process.platform === "darwin" && process.arch === "x64") {
    return { archiveNames: ["panache-x86_64-apple-darwin.tar.gz"], binaryName };
  }
  if (process.platform === "linux" && process.arch === "arm64") {
    return {
      archiveNames: [
        "panache-aarch64-unknown-linux-gnu.tar.gz",
        "panache-aarch64-unknown-linux-musl.tar.gz",
      ],
      binaryName,
    };
  }
  if (process.platform === "linux" && process.arch === "x64") {
    return {
      archiveNames: [
        "panache-x86_64-unknown-linux-gnu.tar.gz",
        "panache-x86_64-unknown-linux-musl.tar.gz",
      ],
      binaryName,
    };
  }
  if (process.platform === "win32" && process.arch === "x64") {
    return { archiveNames: ["panache-x86_64-pc-windows-msvc.zip"], binaryName };
  }
  if (process.platform === "win32" && process.arch === "arm64") {
    return { archiveNames: ["panache-aarch64-pc-windows-msvc.zip"], binaryName };
  }
  throw new Error(`Unsupported platform: ${process.platform}-${process.arch}`);
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function httpGet(url: string, redirectCount = 0): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const request = https.get(
      url,
      {
        headers: {
          Accept: "application/vnd.github+json",
          "User-Agent": "panache-vscode",
        },
      },
      (response) => {
        if (
          response.statusCode &&
          response.statusCode >= 300 &&
          response.statusCode < 400 &&
          response.headers.location
        ) {
          if (redirectCount >= MAX_REDIRECTS) {
            reject(new Error(`Too many redirects when fetching ${url}`));
            return;
          }
          void httpGet(response.headers.location, redirectCount + 1).then(
            resolve,
            reject,
          );
          return;
        }
        if (response.statusCode !== 200) {
          reject(new Error(`HTTP ${response.statusCode ?? "unknown"}: ${url}`));
          return;
        }
        const chunks: Buffer[] = [];
        response.on("data", (chunk: Buffer) => chunks.push(chunk));
        response.on("end", () => resolve(Buffer.concat(chunks)));
      },
    );
    const timeout = setTimeout(() => {
      request.destroy(new Error(`Request timed out after ${HTTP_TIMEOUT_MS}ms`));
    }, HTTP_TIMEOUT_MS);
    request.on("close", () => {
      clearTimeout(timeout);
    });
    request.on("error", reject);
  });
}

async function withRetries<T>(
  operation: () => Promise<T>,
  operationName: string,
): Promise<T> {
  let lastError: unknown;
  for (let attempt = 1; attempt <= DOWNLOAD_RETRIES; attempt += 1) {
    try {
      return await operation();
    } catch (error) {
      lastError = error;
      if (attempt < DOWNLOAD_RETRIES) {
        await sleep(RETRY_DELAY_MS);
      }
    }
  }
  const reason = lastError instanceof Error ? lastError.message : String(lastError);
  throw new Error(`${operationName} failed after ${DOWNLOAD_RETRIES} attempts: ${reason}`);
}

async function findFileRecursive(
  dir: string,
  filename: string,
): Promise<string | undefined> {
  const entries = await fs.readdir(dir, { withFileTypes: true });
  for (const entry of entries) {
    const entryPath = path.join(dir, entry.name);
    if (entry.isFile() && entry.name === filename) {
      return entryPath;
    }
    if (entry.isDirectory()) {
      const nested = await findFileRecursive(entryPath, filename);
      if (nested) {
        return nested;
      }
    }
  }
  return undefined;
}

export async function resolvePanacheBinary(
  globalStoragePath: string,
  repo: string,
  tag: string,
): Promise<string> {
  const target = detectTargetAsset();
  type Candidate =
    | { kind: "latest-pointer" }
    | { kind: "latest-list" }
    | { kind: "tag"; tag: string };
  const candidates: Candidate[] =
    tag === "latest"
      ? [{ kind: "latest-pointer" }, { kind: "latest-list" }]
      : [
          { kind: "tag", tag },
          ...(tag.startsWith("v")
            ? [{ kind: "tag" as const, tag: `panache-${tag}` }]
            : tag.startsWith("panache-v")
              ? [{ kind: "tag" as const, tag: tag.replace(/^panache-/, "") }]
              : []),
        ];
  const seenTags = new Set<string>();
  const uniqueCandidates = candidates.filter((candidate) => {
    if (candidate.kind !== "tag") {
      return true;
    }
    if (seenTags.has(candidate.tag)) {
      return false;
    }
    seenTags.add(candidate.tag);
    return true;
  });

  const findAsset = (assets: ReleaseAsset[] | undefined): ReleaseAsset | undefined =>
    target.archiveNames
      .map((archiveName) => assets?.find((item) => item.name === archiveName))
      .find((item): item is ReleaseAsset => item !== undefined);

  const candidateLabel = (candidate: Candidate): string =>
    candidate.kind === "tag" ? candidate.tag : candidate.kind;

  let selectedRelease: { release: ReleaseResponse; asset: ReleaseAsset } | undefined;
  const candidateErrors: string[] = [];

  for (const candidate of uniqueCandidates) {
    const releasesUrl =
      candidate.kind === "latest-pointer"
        ? `https://api.github.com/repos/${repo}/releases/latest`
        : candidate.kind === "latest-list"
          ? `https://api.github.com/repos/${repo}/releases?per_page=100`
          : `https://api.github.com/repos/${repo}/releases/tags/${encodeURIComponent(candidate.tag)}`;
    try {
      selectedRelease = await withRetries(
        async () => {
          const releaseBody = await httpGet(releasesUrl);
          if (candidate.kind === "latest-list") {
            const releases = JSON.parse(releaseBody.toString("utf8")) as ReleaseResponse[];
            if (!Array.isArray(releases)) {
              throw new Error(
                `Expected release list array for ${repo}@latest, received non-array response`,
              );
            }

            for (const release of releases) {
              if (release.draft || release.prerelease) {
                continue;
              }
              if (!isCandidatePanacheCliReleaseTag(release.tag_name)) {
                continue;
              }
              if (!Array.isArray(release.assets) || release.assets.length === 0) {
                continue;
              }
              const latestAsset = findAsset(release.assets);
              if (latestAsset) {
                return { release, asset: latestAsset };
              }
            }

            throw new Error(
              `No stable Panache CLI release asset '${target.archiveNames.join("' or '")}' found for ${repo}@latest`,
            );
          }

          const release = JSON.parse(releaseBody.toString("utf8")) as ReleaseResponse;
          if (
            candidate.kind === "latest-pointer" &&
            !isCandidatePanacheCliReleaseTag(release.tag_name)
          ) {
            throw new Error(
              `${repo}/releases/latest points at '${release.tag_name}', which is not a Panache CLI release`,
            );
          }
          const asset = findAsset(release.assets);
          if (!asset) {
            const ref =
              candidate.kind === "tag"
                ? `${repo}@${candidate.tag}`
                : `${repo}@${release.tag_name}`;
            throw new Error(
              `No release asset '${target.archiveNames.join("' or '")}' found for ${ref}`,
            );
          }
          return { release, asset };
        },
        `Fetching release metadata from ${releasesUrl}`,
      );
      break;
    } catch (error) {
      const reason = error instanceof Error ? error.message : String(error);
      candidateErrors.push(`${candidateLabel(candidate)}: ${reason}`);
    }
  }

  if (!selectedRelease) {
    throw new Error(
      `Unable to resolve release '${tag}' for ${repo}. Tried: ${uniqueCandidates.map(candidateLabel).join(", ")}. ${candidateErrors.join(" | ")}`,
    );
  }

  const { release, asset } = selectedRelease;

  const installRoot = path.join(globalStoragePath, "bin", release.tag_name);
  await fs.mkdir(installRoot, { recursive: true });
  const installedBinary = path.join(installRoot, target.binaryName);
  try {
    await fs.access(installedBinary);
    return installedBinary;
  } catch {
    // Download needed.
  }

  const archivePath = path.join(installRoot, asset.name);
  const archive = await withRetries(
    () => httpGet(asset.browser_download_url),
    `Downloading ${asset.name}`,
  );
  await fs.writeFile(archivePath, archive);

  if (asset.name.endsWith(".zip")) {
    const zip = new AdmZip(archivePath);
    zip.extractAllTo(installRoot, true);
  } else if (asset.name.endsWith(".tar.gz")) {
    await tar.x({
      file: archivePath,
      cwd: installRoot,
    });
  } else {
    throw new Error(`Unsupported archive format: ${asset.name}`);
  }

  const resolvedBinary =
    (await findFileRecursive(installRoot, target.binaryName)) ?? installedBinary;
  await fs.copyFile(resolvedBinary, installedBinary);
  if (process.platform !== "win32") {
    await fs.chmod(installedBinary, 0o755);
  }
  return installedBinary;
}
