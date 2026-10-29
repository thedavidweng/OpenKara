// Runs the CLA result step from .github/workflows/cla.yml against a mocked
// GitHub API for the pass, fail, and stale-head paths. A failure must not
// remove the code-blocked sign phrase from the bot comment.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, test } from "vite-plus/test";

const projectRoot = fileURLToPath(new URL("../..", import.meta.url));
const workflow = readFileSync(
  join(projectRoot, ".github/workflows/cla.yml"),
  "utf8",
);

const SIGN_PHRASE = "I have read the CLA Document and I hereby sign the CLA";
const FENCED_PHRASE = "```\n" + SIGN_PHRASE + "\n```";
const GITHUB_ACTIONS_BOT_ID = 41898282;
// cla-action@2e483ec finds its own comment with these markers
// (src/pullrequest/commentContent.ts, src/pullrequest/pullRequestComment.ts).
const CLA_ACTION_MARKERS = [
  "Contributor License bot",
  "CLA Assistant Lite bot",
];
const OLD_FAILURE_BODY =
  "CLA signatures could not be verified for the checked commit. See the [CLA check](https://github.com/thedavidweng/OpenKara/actions/runs/1).\n\n<sub>CLA Assistant Lite bot</sub>";

/** Return the text after `start` up to the next step (or end of file). */
function stepSection(name: string): string {
  const start = workflow.indexOf(`- name: ${name}`);
  expect(start, `step "${name}" exists`).toBeGreaterThan(-1);
  const rest = workflow.slice(start + 1);
  const next = rest.search(/\n\s*- name: /);
  return next === -1 ? rest : rest.slice(0, next);
}

/** Read a one-line `key: value` from a section. */
function scalar(section: string, key: string): string {
  const escapedKey = key.replace(/[.*+?^${}()|[\]\\-]/g, "\\$&");
  const match = section.match(new RegExp(`^\\s*${escapedKey}: (.+)$`, "m"));
  expect(match, `${key} is set`).not.toBeNull();
  return match![1].trim();
}

/** Read a YAML literal block scalar (`key: |` or `key: |-`). */
function blockScalar(section: string, key: string): string {
  const lines = section.split(/\r?\n/);
  const index = lines.findIndex((line) =>
    new RegExp(`^\\s*${key}: \\|-?\\s*$`).test(line),
  );
  expect(index, `${key} is a block scalar`).toBeGreaterThan(-1);
  const keyIndent = lines[index].search(/\S/);
  const strip = lines[index].trimEnd().endsWith("|-");
  const body: string[] = [];
  for (const line of lines.slice(index + 1)) {
    if (line.trim() !== "" && line.search(/\S/) <= keyIndent) break;
    body.push(line);
  }
  while (body.length && body[body.length - 1].trim() === "") body.pop();
  const indent = Math.min(
    ...body.filter((l) => l.trim() !== "").map((l) => l.search(/\S/)),
  );
  const text = body.map((l) => l.slice(indent)).join("\n");
  return strip ? text : text + "\n";
}

const agreementStep = stepSection("Record and check signatures");
const resultStep = stepSection("Publish the CLA result on the checked commit");
const resultScript = blockScalar(resultStep, "script");
const notSignedPrompt = blockScalar(
  agreementStep,
  "custom-notsigned-prcomment",
);
const claDocument = scalar(agreementStep, "path-to-document");

/**
 * Mirror of cla-action@2e483ec commentContent() for a single unsigned CLA
 * committer with suggest-recheck left at its default.
 */
function claActionNotSignedBody(): string {
  const lineOne = notSignedPrompt.trim().replace("$you", "you");
  return (
    `${lineOne}\n   - - -\n   ${SIGN_PHRASE}\n   - - -\n   ` +
    "<sub>You can retrigger this bot by commenting **recheck** in this Pull Request. </sub>" +
    "<sub>Posted by the **Contributor License bot**.</sub>"
  );
}

interface Comment {
  id: number;
  user: { id: number };
  body: string;
}

interface RunOptions {
  agreement: "success" | "failure" | "skipped";
  checkedHead?: string;
  currentHead?: string;
  comments: Comment[];
}

const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor;

async function runResultStep(options: RunOptions) {
  const checkedHead = options.checkedHead ?? "a".repeat(40);
  const currentHead = options.currentHead ?? checkedHead;
  const statuses: Record<string, unknown>[] = [];
  const updates: { comment_id: number; body: string }[] = [];
  const comments = options.comments;
  const github = {
    rest: {
      pulls: {
        get: async () => ({ data: { head: { sha: currentHead } } }),
      },
      repos: {
        createCommitStatus: async (args: Record<string, unknown>) => {
          statuses.push(args);
        },
      },
      issues: {
        listComments: async () => ({ data: comments }),
        updateComment: async (args: { comment_id: number; body: string }) => {
          updates.push(args);
          const target = comments.find((c) => c.id === args.comment_id);
          if (target) target.body = args.body;
        },
      },
    },
    paginate: async (
      method: (args: unknown) => Promise<{ data: Comment[] }>,
      args: unknown,
    ) => (await method(args)).data,
  };
  const context = {
    repo: { owner: "thedavidweng", repo: "OpenKara" },
    issue: { number: 475 },
    serverUrl: "https://github.com",
    runId: 4242,
  };
  const env = {
    CHECKED_HEAD: checkedHead,
    AGREEMENT_RESULT: options.agreement,
    CLA_DOCUMENT_URL: scalar(resultStep, "CLA_DOCUMENT_URL"),
    CLA_SIGN_PHRASE: scalar(resultStep, "CLA_SIGN_PHRASE"),
  };
  const run = new AsyncFunction(
    "github",
    "context",
    "core",
    "process",
    resultScript,
  );
  await run(github, context, {}, { env });
  return { statuses, updates, comments };
}

function botComment(body: string, id = 1): Comment {
  return { id, user: { id: GITHUB_ACTIONS_BOT_ID }, body };
}

const runUrl = "https://github.com/thedavidweng/OpenKara/actions/runs/4242";

describe("CLA result step", () => {
  test("pass: marks the commit verified and reports success", async () => {
    const { statuses, updates } = await runResultStep({
      agreement: "success",
      comments: [botComment(claActionNotSignedBody())],
    });
    expect(statuses).toHaveLength(1);
    expect(statuses[0]).toMatchObject({
      sha: "a".repeat(40),
      context: "CLA",
      state: "success",
      target_url: runUrl,
    });
    expect(updates).toHaveLength(1);
    expect(updates[0].body).toBe(
      "All contributors have signed the CLA. Signatures have been recorded. ✍️ ✅\n\n<sub>CLA Assistant Lite bot</sub>",
    );
  });

  test("fail: keeps cla-action's prompt and adds a note with the run link", async () => {
    const prompt = claActionNotSignedBody();
    const { statuses, updates } = await runResultStep({
      agreement: "failure",
      comments: [botComment(prompt)],
    });
    expect(statuses[0]).toMatchObject({ state: "failure", target_url: runUrl });
    expect(updates).toHaveLength(1);
    const body = updates[0].body;
    expect(body).toContain(FENCED_PHRASE);
    expect(body).toContain(prompt);
    expect(body).toContain(`[CLA check](${runUrl})`);
    expect(body).toContain("could not be verified for commit `aaaaaaa`");
  });

  test("fail: restores the instructions the old step overwrote (PR #475)", async () => {
    const { updates } = await runResultStep({
      agreement: "failure",
      comments: [botComment(OLD_FAILURE_BODY)],
    });
    const body = updates[0].body;
    expect(body).toContain(FENCED_PHRASE);
    expect(body).toContain(`](${claDocument})`);
    expect(body).toContain("**recheck**");
    // The restored text matches the prompt cla-action posts.
    expect(body).toContain(notSignedPrompt.trim().replace("$you", "you"));
    expect(body).not.toContain("for the checked commit.");
  });

  test("fail: restores the instructions when cla-action did not run", async () => {
    const { statuses, updates } = await runResultStep({
      agreement: "skipped",
      comments: [botComment(OLD_FAILURE_BODY)],
    });
    expect(statuses[0]).toMatchObject({ state: "failure" });
    expect(updates[0].body).toContain(FENCED_PHRASE);
  });

  test("stale head: fails the checked commit but keeps the instructions", async () => {
    const { statuses, updates } = await runResultStep({
      agreement: "success",
      checkedHead: "b".repeat(40),
      currentHead: "c".repeat(40),
      comments: [
        botComment(
          "CLA acceptance received.<br/><sub>Posted by the **Contributor License bot**.</sub>",
        ),
      ],
    });
    expect(statuses[0]).toMatchObject({
      sha: "b".repeat(40),
      state: "failure",
    });
    const body = updates[0].body;
    expect(body).toContain(FENCED_PHRASE);
    expect(body).toContain("head moved after commit `bbbbbbb` was checked");
    expect(body).toContain(`[CLA check](${runUrl})`);
  });

  test("repeated failures keep a single note", async () => {
    const comments = [botComment(claActionNotSignedBody())];
    await runResultStep({ agreement: "failure", comments });
    await runResultStep({ agreement: "failure", comments });
    const body = comments[0].body;
    expect(body.match(/<!-- cla-result-note -->/g)).toHaveLength(1);
    expect(body.match(/could not be verified/g)).toHaveLength(1);
    expect(body).toContain(FENCED_PHRASE);
  });

  test("accepts a prompt stored with CRLF line endings", async () => {
    const prompt = claActionNotSignedBody();
    const { updates } = await runResultStep({
      agreement: "failure",
      comments: [botComment(prompt.replace(/\n/g, "\r\n"))],
    });
    expect(updates[0].body).toContain(prompt);
  });

  test("the edited comment is still found by both cla-action and this step", async () => {
    for (const agreement of ["success", "failure"] as const) {
      const { updates } = await runResultStep({
        agreement,
        comments: [botComment(OLD_FAILURE_BODY)],
      });
      const body = updates[0].body;
      expect(
        CLA_ACTION_MARKERS.some((marker) =>
          new RegExp(`.*${marker}.*`, "m").test(body),
        ),
      ).toBe(true);
    }
  });

  test("only edits the github-actions bot comment", async () => {
    const { updates } = await runResultStep({
      agreement: "failure",
      comments: [
        { id: 7, user: { id: 95214375 }, body: "Contributor License bot" },
      ],
    });
    expect(updates).toHaveLength(0);
  });
});
