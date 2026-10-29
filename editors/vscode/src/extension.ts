// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

// Starts quakec-lsp for QuakeC files. Every feature is the server's; this file only finds the
// binary and passes the settings of a project with no quakec.toml as `initializationOptions`,
// with the names quakec.toml uses (apps/lsp/README.md, Settings).

import * as vscode from "vscode";
import { LanguageClient, TransportKind } from "vscode-languageclient/node";

/** The options quakec-lsp reads from `initializationOptions`. */
interface InitializationOptions {
  dialect?: string;
  target?: string;
  autoproto: boolean;
}

let client: LanguageClient | undefined;

function initializationOptions(config: vscode.WorkspaceConfiguration): InitializationOptions {
  const options: InitializationOptions = {
    autoproto: config.get<boolean>("autoproto", false),
  };
  const dialect = config.get<string | null>("dialect", null);
  if (dialect !== null) {
    options.dialect = dialect;
  }
  const target = config.get<string | null>("target", null);
  if (target !== null) {
    options.target = target;
  }
  return options;
}

async function start(): Promise<void> {
  const config = vscode.workspace.getConfiguration("quakec");
  const command = config.get<string>("server.path", "quakec-lsp");
  client = new LanguageClient(
    "quakec",
    "QuakeC",
    { command, transport: TransportKind.stdio },
    {
      documentSelector: [{ scheme: "file", language: "quakec" }],
      initializationOptions: initializationOptions(config),
    },
  );
  await client.start();
}

/** The last restart; each waits for the one before, so only one client is ever running. */
let restarting: Promise<void> = Promise.resolve();

async function restart(): Promise<void> {
  if (client?.isRunning() === true) {
    await client.stop();
  }
  client = undefined;
  await start();
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  // The server reads the options once, in `initialize`, so a change starts it again.
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (!event.affectsConfiguration("quakec")) {
        return;
      }
      restarting = restarting.then(restart).catch(async (error: unknown) => {
        await vscode.window.showErrorMessage(`quakec-lsp did not start: ${String(error)}`);
      });
    }),
  );
  await start();
}

export async function deactivate(): Promise<void> {
  await client?.stop();
}
