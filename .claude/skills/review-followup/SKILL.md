---
name: review-followup
description: Triage the AI Architectural Review comment on the current PR — fix, dismiss, or defer-for-discussion each open finding interactively, commit per fix, push at end.
---

# Review-Followup

You are driving the interactive triage loop against the single
**AI Architectural Review** comment posted by the review bot (see
`.github/workflows/claude-code-review.yml` and
`.github/ai-review-prompt.md`).

## Ground rules

- The review lives in **one** comment per PR, edited in place by the bot.
- Each finding is a markdown task-list line that starts with `- [ ]`
  or `- [x]`, optionally followed by a suffix that records maintainer
  intent:
  - `- [ ] <finding>` → open, needs triage.
  - `- [ ] <finding> — _discuss: <note>_` → open, but the maintainer
    parked it for further discussion. Still surfaces in future triage
    runs; the note reminds you why it was left open.
  - `- [x] <finding>` → maintainer-accepted / implemented. **Never
    modify.**
  - `- [x] <finding> — _dismissed: <reason>_` → maintainer-dismissed
    (permanent won't-fix, expressed as a final tick with a reason
    suffix). **Never modify.**
- Four sections exist, in this order: `Must-fix`, `Consider`, `Noted`,
  `Skipped`. Only items in the first three are triaged; `Skipped` is
  informational.
- Line wording from the bot is authoritative — when flipping state,
  preserve the original line verbatim and only change the marker
  and/or append a suffix.

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
section, extract every `- [ ]` / `- [x]` line and capture:

- state marker (` ` or `x`)
- the full line text (verbatim, no rephrasing), including any
  `— _dismissed: ..._` / `— _discuss: ..._` suffix
- any cited `path/to/file.rs:LINE` reference (best-effort regex)

Build a worklist of all `- [ ]` items (including ones with a
`— _discuss: ..._` suffix — those were parked for later, and "later"
may be now). If the worklist is empty, tell the user "nothing to
triage on PR #$PR" and stop — do not push, do not commit.

## Step 4 — Triage loop

For each open item, one at a time in section order (Must-fix first):

1. Show the finding to the user along with the relevant code context.
   If the finding cites `path/to/file.rs:LINE`, Read a window around
   that line (roughly ±20 lines) so the user sees the code before
   deciding. If the line already has a `— _discuss: ..._` suffix,
   surface that note explicitly ("previously parked: <note>").
2. Ask the user: **fix**, **dismiss**, or **discuss**?
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
   - Flip this line from `- [ ]` to `- [x]` and append
     ` — _dismissed: <reason>_`. The `[x]` signals "final" to the
     review bot; the suffix records *why* it's final (won't-fix vs
     implemented).
   - Update the comment body on GitHub (see Step 5).
5. On **discuss**:
   - Ask the user for a one-line note ("what do you want to discuss
     about this?").
   - Leave the marker as `- [ ]` and append ` — _discuss: <note>_`.
     If the line already has a `— _discuss: ..._` suffix from a
     prior run, replace that suffix with the new note (don't stack
     them).
   - Update the comment body on GitHub (see Step 5).

Do **not** batch comment-body edits — apply each dismissal/discuss
flip to the GH comment as it happens, so a mid-session abort still
leaves consistent state.

## Step 5 — Editing the GH comment

Applies to both **dismiss** (flip `[ ]` → `[x]`, append
`— _dismissed: <reason>_`) and **discuss** (keep `[ ]`, append or
replace `— _discuss: <note>_`).

1. **Re-fetch the current body** by comment id — it may have changed
   since step 2 if the bot re-ran. Use `mktemp` so concurrent
   sessions don't clobber each other:

   ```bash
   BODY_FILE=$(mktemp --suffix=.md)
   trap 'rm -f "$BODY_FILE"' EXIT
   gh api "/repos/$(gh repo view --json nameWithOwner --jq .nameWithOwner)/issues/comments/<id>" \
     --jq .body > "$BODY_FILE"
   ```

2. Do an exact-text line replacement on the file — match the full
   original line verbatim and replace with the new line:
   - dismiss: `- [ ] <text>` → `- [x] <text> — _dismissed: <reason>_`
   - discuss (first time): `- [ ] <text>` → `- [ ] <text> — _discuss: <note>_`
   - discuss (replacing a prior discuss note):
     `- [ ] <text> — _discuss: <old>_` → `- [ ] <text> — _discuss: <new>_`

   If the line can't be found verbatim (bot rewrote it on a
   concurrent run), abort this flip and tell the user — do not guess.

3. PATCH the comment:

   ```bash
   gh api -X PATCH "/repos/$(gh repo view --json nameWithOwner --jq .nameWithOwner)/issues/comments/<id>" \
     -F body=@"$BODY_FILE"
   ```

   (`-F body=@file` reads the file as the body field — safer than
   `-f body="$(cat ...)"` which can blow up on shell-special chars.)

4. Verify success by re-reading the comment body and confirming the
   new line is present with the correct marker and suffix.

## Step 6 — Session end

Once the worklist is exhausted:

1. `git push` the accumulated fix commits. If there were no fixes
   (only dismissals and/or discuss flips), skip the push — those
   live on GitHub, not in git.
2. Tell the user:
   - How many items were fixed vs dismissed vs parked for discussion.
   - That the review bot will re-run on push and may surface new
     findings.
   - To re-invoke `/review-followup` once the next review comment
     lands, or sooner to revisit any `— _discuss: ..._` items.

## Safety rails (do not break)

- Never modify `- [x]` lines — they are final, regardless of whether
  they carry a `— _dismissed: ..._` suffix.
- Never strip a `— _dismissed: ..._` or `— _discuss: ..._` suffix from
  any line. Treat suffixes as maintainer intent that must be preserved.
- Never post a second top-level PR comment.
- Never `git push --force` from this skill.
- If a previously-dismissed item (was `- [x] ... — _dismissed: ..._`)
  resurfaces as `- [ ]` on a later run, flag it to the user ("this
  was dismissed before — the bot didn't honor it") rather than
  silently re-dismissing. That's a review-prompt bug worth surfacing.
- If the user course-corrects mid-fix ("no, don't change that"),
  revert the staged edits and return to triage of the next item.
  Don't commit a half-baked fix.
