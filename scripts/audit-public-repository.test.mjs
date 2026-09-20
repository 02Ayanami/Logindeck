import assert from "node:assert/strict";
import test from "node:test";

import { auditPath, auditText } from "./audit-public-repository.mjs";

test("public audit detects secret material without returning its value", () => {
  const samples = [
    ["-----BEGIN PRIVATE", " KEY-----\nsecret"].join(""),
    ["token=gh", "p_abcdefghijklmnopqrstuvwxyz1234567890"].join(""),
    ["token=github_", "pat_abcdefghijklmnopqrstuvwxyz_1234567890"].join(""),
    ["aws=AK", "IAABCDEFGHIJKLMNOP"].join(""),
    ["api=s", "k-abcdefghijklmnopqrstuvwxyz123456"].join(""),
  ];
  for (const sample of samples) {
    const findings = auditText(sample);
    assert.ok(findings.includes("secret-pattern"));
    assert.ok(!JSON.stringify(findings).includes(sample));
  }
});

test("public audit classifies sensitive, generated and local-path files", () => {
  assert.ok(auditPath("config/.env").includes("sensitive-file"));
  assert.ok(auditPath("secrets/signing.p12").includes("sensitive-file"));
  assert.ok(auditPath("data/accounts.sqlite").includes("sensitive-file"));
  assert.ok(auditPath("dist/LoginDeck.msi").includes("generated-installer"));
  assert.ok(auditPath("dist/LoginDeck.dmg").includes("generated-installer"));
  assert.deepEqual(auditPath("Cargo.lock"), []);

  assert.ok(auditText(["C:\\", "Users\\real-person\\Desktop\\file.txt"].join(""))
    .includes("local-absolute-path"));
  assert.ok(auditText(["/", "Users/real-person/Desktop/file.txt"].join(""))
    .includes("local-absolute-path"));
});

test("clearly synthetic local paths are allowed fixtures", () => {
  assert.ok(!auditText("/Users/demo/Applications/Example.app").includes("local-absolute-path"));
  assert.ok(!auditText("/Users/alice/Applications/Example.app").includes("local-absolute-path"));
  assert.ok(!auditText("C:\\Users\\fixture\\AppData\\Local").includes("local-absolute-path"));
});
