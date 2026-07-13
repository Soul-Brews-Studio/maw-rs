import { expect, test } from "bun:test";
import handler, { LIFECYCLE_COMMANDS, handleMenubar } from "./plugin";

function run(args: string[]) {
  return handler({ source: "cli", args });
}

test("help exposes the complete lifecycle surface", () => {
  const result = run(["--help"]);
  expect(result).toMatchObject({ ok: true, exitCode: 0 });
  for (const command of LIFECYCLE_COMMANDS) expect(result.output).toContain(command);
});

test("PR-A lifecycle commands fail closed without side effects", () => {
  for (const command of LIFECYCLE_COMMANDS) {
    const lines: string[] = [];
    const exitCode = handleMenubar([command], (line) => lines.push(line));
    expect(exitCode).toBe(1);
    expect(lines.join("\n")).toContain("not available in the PR-A skeleton");
  }
});

test("no arguments defaults to status and unknown input shows usage", () => {
  expect(run([]).output).toContain("maw menubar status");
  const unknown = run(["bogus"]);
  expect(unknown).toMatchObject({ ok: false, exitCode: 1 });
  expect(unknown.output).toContain("Unknown subcommand: bogus");
  expect(unknown.output).toContain("Usage:");
});

test("writer mode streams output instead of returning duplicate text", () => {
  const lines: string[] = [];
  const result = handler({ source: "cli", args: ["install"], writer: (line) => lines.push(line) });
  expect(result.output).toBe("");
  expect(lines).toHaveLength(1);
});
