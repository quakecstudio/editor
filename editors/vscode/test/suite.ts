// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

// Runs in VS Code's extension host with the rerelease open as the workspace: asks VS Code for
// the definition of the `T_Damage` call in weapons.qc, through this extension and quakec-lsp,
// and checks it is the declaration in defs.qc, the first probe of
// docs/spec/vectors/lsp/rerelease.txt (`definition T_Damage weapons.qc:71:3: defs.qc:806:68`,
// 1-based).

import process from "node:process";
import * as vscode from "vscode";

export async function run(): Promise<void> {
  const folder = vscode.workspace.workspaceFolders?.[0];
  if (folder === undefined) {
    throw new Error("no workspace folder: run through test/run.ts");
  }
  const config = vscode.workspace.getConfiguration("quakec");
  const global = vscode.ConfigurationTarget.Global;
  await config.update("dialect", "fteqcc", global);
  await config.update("target", "rerelease", global);
  const server = process.env["QUAKEC_LSP"];
  if (server !== undefined) {
    await config.update("server.path", server, global);
  }

  const weapons = vscode.Uri.joinPath(folder.uri, "weapons.qc");
  const document = await vscode.workspace.openTextDocument(weapons);
  await vscode.window.showTextDocument(document);
  const extension = vscode.extensions.getExtension("quakecstudio.quakec");
  if (extension === undefined) {
    throw new Error("the extension is not loaded");
  }
  await extension.activate();

  const offset = document.getText().indexOf("T_Damage (trace_ent");
  if (offset < 0) {
    throw new Error("weapons.qc has no `T_Damage (trace_ent`");
  }
  const at = document.positionAt(offset);
  const found = await vscode.commands.executeCommand<(vscode.Location | vscode.LocationLink)[]>(
    "vscode.executeDefinitionProvider",
    weapons,
    at,
  );
  const first = found[0];
  if (first === undefined) {
    throw new Error("no definition");
  }
  const uri = "uri" in first ? first.uri : first.targetUri;
  const range = "range" in first ? first.range : (first.targetSelectionRange ?? first.targetRange);
  const got = `${vscode.workspace.asRelativePath(uri)}:${range.start.line + 1}:${range.start.character + 1}`;
  const from = `weapons.qc:${at.line + 1}:${at.character + 1}`;
  console.log(`definition T_Damage ${from}: ${got}`);
  if (from !== "weapons.qc:71:3" || got !== "defs.qc:806:68") {
    throw new Error(`expected weapons.qc:71:3 -> defs.qc:806:68, got ${from} -> ${got}`);
  }
}
