import * as vscode from "vscode";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  Trace,
} from "vscode-languageclient/node";
import { resolvePanacheBinary } from "./installer";

let client: LanguageClient | undefined;

async function isNixOs(): Promise<boolean> {
  if (process.platform !== "linux") {
    return false;
  }
  try {
    const osRelease = await fs.readFile("/etc/os-release", "utf8");
    return /(^|\n)ID=nixos(\n|$)/.test(osRelease);
  } catch {
    return false;
  }
}

function isDownloadBinaryExplicitlyConfigured(
  config: vscode.WorkspaceConfiguration,
): boolean {
  const value = config.inspect<boolean>("downloadBinary");
  return (
    value?.globalValue !== undefined ||
    value?.workspaceValue !== undefined ||
    value?.workspaceFolderValue !== undefined
  );
}

function isReleaseTagExplicitlyConfigured(
  config: vscode.WorkspaceConfiguration,
): boolean {
  const value = config.inspect<string>("releaseTag");
  return (
    value?.globalValue !== undefined ||
    value?.workspaceValue !== undefined ||
    value?.workspaceFolderValue !== undefined
  );
}

function isExecutableStrategyExplicitlyConfigured(
  config: vscode.WorkspaceConfiguration,
): boolean {
  const value = config.inspect<string>("executableStrategy");
  return (
    value?.globalValue !== undefined ||
    value?.workspaceValue !== undefined ||
    value?.workspaceFolderValue !== undefined
  );
}

type ExecutableStrategy = "bundled" | "environment" | "path";

async function findBundledBinary(
  context: vscode.ExtensionContext,
): Promise<string | undefined> {
  const binaryName =
    process.platform === "win32" ? "panache.exe" : "panache";
  const candidate = path.join(context.extensionPath, "server", binaryName);
  try {
    await fs.access(candidate);
    return candidate;
  } catch {
    return undefined;
  }
}

async function resolveCommandPath(
  context: vscode.ExtensionContext,
  config: vscode.WorkspaceConfiguration,
  outputChannel: vscode.OutputChannel,
): Promise<string> {
  const githubRepo = config.get<string>("githubRepo", "jolars/panache");
  const version = config.get<string>("version", "latest");
  const releaseTag = config.get<string>("releaseTag", "latest");
  const releaseTagExplicit = isReleaseTagExplicitlyConfigured(config);
  const selectedRelease = releaseTagExplicit ? releaseTag : version;
  const versionPinExplicit =
    releaseTagExplicit ||
    config.inspect<string>("version")?.globalValue !== undefined ||
    config.inspect<string>("version")?.workspaceValue !== undefined ||
    config.inspect<string>("version")?.workspaceFolderValue !== undefined;

  if (isExecutableStrategyExplicitlyConfigured(config)) {
    const strategy = config.get<ExecutableStrategy>(
      "executableStrategy",
      "bundled",
    );
    const executablePath = config.get<string | null>("executablePath", null);

    if (strategy === "path") {
      if (!executablePath || executablePath.trim().length === 0) {
        void vscode.window.showWarningMessage(
          "panache.executableStrategy is set to 'path' but panache.executablePath is empty. Falling back to 'panache' on PATH.",
        );
        return "panache";
      }
      return executablePath;
    }

    if (strategy === "environment") {
      return "panache";
    }

    // strategy === "bundled"
    if (!versionPinExplicit) {
      const bundled = await findBundledBinary(context);
      if (bundled) {
        outputChannel.appendLine(`Using bundled Panache binary at ${bundled}.`);
        return bundled;
      }
    }
    try {
      return await resolvePanacheBinary(
        context.globalStorageUri.fsPath,
        githubRepo,
        selectedRelease,
      );
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "Unknown download error";
      void vscode.window.showWarningMessage(
        `Panache binary download failed (${message}). Falling back to 'panache' on PATH.`,
      );
      return "panache";
    }
  }

  // Legacy path: respects the deprecated downloadBinary + commandPath settings.
  const fallbackCommandPath = config.get<string>("commandPath", "panache");
  const downloadBinary = config.get<boolean>("downloadBinary", true);
  const downloadBinaryExplicit = isDownloadBinaryExplicitlyConfigured(config);
  const nixOs = await isNixOs();
  const shouldDownloadBinary =
    downloadBinary && (!nixOs || downloadBinaryExplicit);

  if (nixOs && !downloadBinaryExplicit) {
    outputChannel.appendLine(
      "Detected NixOS; skipping binary download and using panache.commandPath.",
    );
  }

  if (shouldDownloadBinary) {
    if (!versionPinExplicit) {
      const bundled = await findBundledBinary(context);
      if (bundled) {
        outputChannel.appendLine(`Using bundled Panache binary at ${bundled}.`);
        return bundled;
      }
    }
    try {
      return await resolvePanacheBinary(
        context.globalStorageUri.fsPath,
        githubRepo,
        selectedRelease,
      );
    } catch (error) {
      const message =
        error instanceof Error ? error.message : "Unknown download error";
      void vscode.window.showWarningMessage(
        `Panache binary download failed (${message}). Falling back to '${fallbackCommandPath}'.`,
      );
    }
  }

  return fallbackCommandPath;
}

function mergeServerEnvironment(
  baseEnv: NodeJS.ProcessEnv,
  overrides: Record<string, string>,
  extraPathEntries: string[],
): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = { ...baseEnv, ...overrides };
  const normalizedExtraPath = extraPathEntries
    .map((entry) => entry.trim())
    .filter((entry) => entry.length > 0);

  if (normalizedExtraPath.length === 0) {
    return env;
  }

  const pathKey =
    process.platform === "win32"
      ? Object.keys(env).find((key) => key.toLowerCase() === "path") ?? "Path"
      : "PATH";

  for (const key of Object.keys(env)) {
    if (key !== pathKey && key.toLowerCase() === "path") {
      delete env[key];
    }
  }

  const existingPath = env[pathKey]?.trim() ?? "";
  env[pathKey] =
    normalizedExtraPath.join(path.delimiter) +
    (existingPath ? `${path.delimiter}${existingPath}` : "");

  return env;
}

async function startClient(
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel,
): Promise<void> {
  const config = vscode.workspace.getConfiguration("panache");
  const commandPath = await resolveCommandPath(context, config, outputChannel);

  const serverArgs = config.get<string[]>("serverArgs", []);
  const userServerEnv = config.get<Record<string, string>>("serverEnv", {});
  const logLevel = config.get<string | null>("logLevel", null);
  const serverEnv: Record<string, string> = {
    ...(logLevel ? { RUST_LOG: logLevel } : {}),
    ...userServerEnv,
  };
  const extraPath = config.get<string[]>("extraPath", []);
  const traceLevel = config.get<"off" | "messages" | "verbose">(
    "trace.server",
    "off",
  );
  const experimentalIncrementalParsing = config.get<boolean>(
    "experimental.incrementalParsing",
    false,
  );

  const serverOptions: ServerOptions = {
    command: commandPath,
    args: ["lsp", ...serverArgs],
    options: {
      env: mergeServerEnvironment(process.env, serverEnv, extraPath),
    },
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: "file", language: "markdown" },
      { scheme: "untitled", language: "markdown" },
      { scheme: "file", language: "quarto" },
      { scheme: "untitled", language: "quarto" },
      { scheme: "file", language: "rmarkdown" },
      { scheme: "untitled", language: "rmarkdown" },
      { scheme: "file", pattern: "**/*.qmd" },
      { scheme: "file", language: "plaintext", pattern: "**/*.qmd" },
      { scheme: "file", pattern: "**/*.Rmd" },
      { scheme: "file", language: "plaintext", pattern: "**/*.Rmd" },
      { scheme: "file", pattern: "**/*.rmd" },
      { scheme: "file", language: "plaintext", pattern: "**/*.rmd" },
    ],
    outputChannel,
    traceOutputChannel: outputChannel,
    initializationOptions: {
      settings: {
        panache: {
          experimental: {
            incrementalParsing: experimentalIncrementalParsing,
          },
        },
      },
    },
  };

  client = new LanguageClient(
    "panacheLanguageServer",
    "Panache Language Server",
    serverOptions,
    clientOptions,
  );

  try {
    await client.start();
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    outputChannel.appendLine(`Failed to start Panache language server: ${message}`);
    void vscode.window.showErrorMessage(
      `Panache language server failed to start: ${message}`,
    );
    client = undefined;
    return;
  }
  if (traceLevel === "messages") {
    void client.setTrace(Trace.Messages);
  } else if (traceLevel === "verbose") {
    void client.setTrace(Trace.Verbose);
  }
}

async function restartClient(
  context: vscode.ExtensionContext,
  outputChannel: vscode.OutputChannel,
): Promise<void> {
  if (client) {
    try {
      await client.stop();
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      outputChannel.appendLine(`Error stopping Panache language server: ${message}`);
    }
    client = undefined;
  }
  await startClient(context, outputChannel);
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  const outputChannel = vscode.window.createOutputChannel(
    "Panache Language Server",
  );
  context.subscriptions.push(outputChannel);

  context.subscriptions.push(
    vscode.commands.registerCommand("panache.restart", () =>
      restartClient(context, outputChannel),
    ),
  );

  await startClient(context, outputChannel);
}

export async function deactivate(): Promise<void> {
  if (client) {
    await client.stop();
  }
}
