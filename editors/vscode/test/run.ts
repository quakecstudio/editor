// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 QuakeC Studio contributors

// Downloads VS Code, opens the rerelease's QuakeC as the workspace with this extension loaded
// and runs test/suite.ts in it. The rerelease comes from `cargo xtask corpus fetch rerelease`;
// QUAKEC_RERELEASE names its directory, and quakec-lsp must be on PATH or in QUAKEC_LSP.

import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { runTests } from "@vscode/test-electron";

const here = path.dirname(fileURLToPath(import.meta.url));
const rerelease = process.env["QUAKEC_RERELEASE"];
if (rerelease === undefined) {
  throw new Error("set QUAKEC_RERELEASE to the rerelease's QuakeC directory");
}

await runTests({
  extensionDevelopmentPath: path.resolve(here, ".."),
  extensionTestsPath: path.resolve(here, "suite.cjs"),
  // A fresh profile, in a short path: the profile holds VS Code's IPC socket, whose path macOS
  // limits to 103 bytes.
  launchArgs: [
    rerelease,
    "--disable-extensions",
    `--user-data-dir=${mkdtempSync(path.join(tmpdir(), "quakec-"))}`,
  ],
});
