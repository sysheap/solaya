---
name: review-followup
description: Triage the AI Architectural Review comment on the current PR — fix or dismiss each open finding interactively, commit per fix, push at end.
---

# Review-Followup

You are driving the interactive fix-or-dismiss loop against the single
**AI Architectural Review** comment posted by the review bot (see
`.github/workflows/claude-code-review.yml` and
`.github/ai-review-prompt.md`).

## Ground rules

- The review lives in **one** comment per PR, edited in place by the bot.
- Each finding is a markdown task-list line that starts with `- [ ]`,
  `- [x]`, or `- [D]`:
  - `- [ ]` → open, needs triage.
  - `- [x]` → maintainer-accepted / implemented. **Never modify.**
  - `- [D]` → maintainer-dismissed (permanent won't-fix). **Never modify.**
- Four sections exist, in this order: `Must-fix`, `Consider`, `Noted`,
  `Skipped`. Only items in the first three are triaged; `Skipped` is
  informational.
- Line wording from the bot is authoritative — when flipping state,
  preserve the original line verbatim and only change the marker and
  append a suffix.

## Step 1 — Locate the PR

Run:

```bash
gh pr view --json number,url,headRefName
```

If this fails or returns no PR for the current branch, abort with a
clear message ("no open PR for branch `<name>`; push the branch and
open a PR first") and stop. Do **not** try to create one.

Record the PR number as `$PR`.

## Step 2 — Fetch the review comment

```bash
gh pr view "$PR" --json comments \
  --jq '[.comments[] | select(.author.login == "github-actions[bot]"
        and (.body | startswith("## AI Architectural Review")))] | last
        | {id, body}'
```

Capture both `id` and `body`. If no such comment exists, abort with
"no AI Architectural Review comment found on PR #$PR — has the review
job run yet?" Do **not** create one.

## Step 3 — Parse the body

Split the body into sections by the headings (`### Must-fix`,
`### Consider`, `### Noted`, `### Skipped`). Within each triaged
section, extract every `- [ ]` / `- [x]` / `- [D]` line and capture:

- state marker (` `, `x`, `D`)
- the full line text (verbatim, no rephrasing)
- any cited `path/to/file.rs:LINE` reference (best-effort regex)

Build a worklist of only the `- [ ]` items. If the worklist is empty,
tell the user "nothing to triage on PR #$PR" and stop — do not push,
do not commit.

## Step 4 — Triage loop

For each open item, one at a time in section order (Must-fix first):

1. Show the finding to the user along with the relevant code context.
   If the finding cites `path/to/file.rs:LINE`, Read a window around
   that line (roughly ±20 lines) so the user sees the code before
   deciding.
2. Ask the user: **fix** or **dismiss**?
3. On **fix**:
   - Plan the change, apply it with normal tools (Edit/Grep/etc.).
   - Run any obviously-relevant check (e.g. `just clippy` for a lint
     finding, `just test-unit` for a logic finding). Don't run the
     full `just ci` per item — that's overkill.
   - Create one commit for this fix. Commit message should be short
     and reference the finding — e.g.
     `review: fix scheduler race flagged in review`. The pre-commit
     hook runs fmt + clippy + shellcheck; if it blocks, fix and
     re-stage into a NEW commit (never `--amend`).
4. On **dismiss**:
   - Ask the user for a one-line reason.
   - Flip this line from `- [ ]` to `- [D]` and append
     ` — _dismissed: <reason>_`.
   - Update the comment body on GitHub (see Step 5).

Do **not** batch dismissals — apply each one to the GH comment as it
happens, so a mid-session abort still leaves consistent state.

## Step 5 — Editing the GH comment

When flipping an item to `- [D]`:

1. **Re-fetch the current body** by comment id — it may have changed
   since step 2 if the bot re-ran:

   ```bash
   gh api /repos/{owner}/{repo}/issues/comments/<id> --jq .body > /tmp/review-body.md
   ```

   (Get `{owner}/{repo}` from `gh repo view --json nameWithOwner --jq .nameWithOwner`.)

2. Do an exact-text line replacement on the file — match the full
   original `- [ ] ...` line and replace with
   `- [D] ... — _dismissed: <reason>_`. If the line can't be found
   verbatim (bot rewrote it on a concurrent run), abort this
   dismissal and tell the user — do not guess.

3. PATCH the comment:

   ```bash
   gh api -X PATCH /repos/{owner}/{repo}/issues/comments/<id> \
     -F body=@/tmp/review-body.md
   ```

   (`-F body=@file` reads the file as the body field — safer than
   `-f body="$(cat ...)"` which can blow up on shell-special chars.)

4. Verify success by re-reading the comment body and confirming the
   `- [D]` line is present.

## Step 6 — Session end

Once the worklist is exhausted:

1. `git push` the accumulated fix commits. If there were no fixes
   (only dismissals), skip the push — dismissals live on GitHub, not
   in git.
2. Tell the user:
   - How many items were fixed vs dismissed.
   - That the review bot will re-run on push and may surface new
     findings.
   - To re-invoke `/review-followup` once the next review comment
     lands.

## Safety rails (do not break)

- Never rewrite `- [x]` or `- [D]` lines — both are final.
- Never post a second top-level PR comment.
- Never `git push --force` from this skill.
- If a dismissed item resurfaces as `- [ ]` on a later run, flag it
  to the user ("this was dismissed before — the bot didn't honor it")
  rather than silently re-dismissing. That's a review-prompt bug
  worth surfacing.
- If the user course-corrects mid-fix ("no, don't change that"),
  revert the staged edits and return to triage of the next item.
  Don't commit a half-baked fix.
