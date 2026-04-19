---
name: review-followup
description: Triage the AI Architectural Review comment on the current PR — collect fix/dismiss/discuss decisions for every open finding upfront, then plan and execute the accepted fixes, commit per fix, push at end.
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

Split the body into sections by **prefix-matching** the headings
`### Must-fix`, `### Consider`, `### Noted`, `### Skipped`. The bot's
template appends a parenthetical to each (e.g.
`### Must-fix (correctness / safety / policy violations)`), so match
on `startswith` rather than equality. Within each triaged section,
extract every `- [ ]` / `- [x]` line and capture:

- state marker (` ` or `x`)
- the full line text (verbatim, no rephrasing), including any
  `— _dismissed: ..._` / `— _discuss: ..._` suffix
- any cited `path/to/file.rs:LINE` reference (best-effort regex)

Build a worklist of all `- [ ]` items (including ones with a
`— _discuss: ..._` suffix — those were parked for later, and "later"
may be now). If the worklist is empty, tell the user "nothing to
triage on PR #$PR" and stop — do not push, do not commit.

## Step 4 — Triage upfront (collect all decisions before acting)

The goal of this step is to get a disposition for **every** open
finding *before* you write any code or edit the GH comment. Deciding
upfront lets the user see the full slate at once, and lets you plan
the fixes as a whole rather than one at a time.

1. **Load code context for every finding.** For each open item, if it
   cites `path/to/file.rs:LINE`, Read a window around that line
   (roughly ±20 lines). Batch these reads — they're independent, so
   issue them in parallel.

2. **Present the findings in batches of up to 4 via the
   `AskUserQuestion` tool.** `AskUserQuestion` accepts 1–4 questions
   per call, so if there are more than 4 open items, make multiple
   calls — but do all of them *before* moving on to Step 5.

   For each finding, one question. Put the finding text (and any
   `— _discuss: ..._` note from a prior run, surfaced as
   "previously parked: <note>") in the question body. Use these three
   options, in this order:

   - **Fix** — apply the change now, commit it.
   - **Dismiss** — permanent won't-fix; flip to `- [x] ... — _dismissed: <reason>_`.
   - **Discuss** — park for later; keep `- [ ]`, append
     `— _discuss: <note>_`.

   Tell the user they can add a free-text note on their selection —
   `AskUserQuestion` returns these in its `annotations` map, keyed by
   question text, under a `notes` field. After the call, for every
   **Dismiss** / **Discuss** answer check `annotations[question].notes`:
   if present and non-empty, use it as the reason/note. If missing or
   empty, queue a plain-text follow-up prompt — and batch all such
   follow-ups into one prompt at the end of triage, not per-item.

3. **Record the decisions in a worklist**, keeping section + original
   line text per finding. You now have three buckets:
   - `fix`: list of findings + file/line references.
   - `dismiss`: findings + reasons.
   - `discuss`: findings + notes.

   If every finding landed in `dismiss` / `discuss`, skip Step 5
   (nothing to plan) and go straight to Step 6 to apply the GH
   comment edits.

## Step 5 — Plan the fixes

With the full `fix` bucket in hand, produce a short plan before
editing any code:

1. Group related findings if they touch the same file/subsystem —
   fixing them together avoids churn and lets one test run cover
   multiple items. Still commit one logical fix per commit (per
   `CLAUDE.md`'s incremental-commit rule), but ordering matters:
   land prerequisite fixes before dependent ones.
2. For each fix, name the file(s) you expect to touch and the check
   you'll run (e.g. `just clippy` for a lint finding,
   `just test-unit` for a logic finding — not the full `just ci`
   per item).
3. Show the plan to the user as a terse bullet list and proceed —
   you don't need per-item approval here, they already decided at
   Step 4. Only pause if something in the plan surprises you (e.g.
   two findings contradict each other) and the user needs to
   arbitrate.

## Step 6 — Apply fixes, then comment edits

Execute in this order so the git history and the GH comment stay in
sync even if you abort mid-session:

1. **Fixes first.** For each item in the `fix` bucket, in plan order:
   - Apply the change with normal tools (Edit/Grep/etc.).
   - Run the check named in the plan.
   - Create one commit per fix. Commit message should be short and
     reference the finding — e.g.
     `review: fix scheduler race flagged in review`. The pre-commit
     hook runs fmt + clippy + shellcheck; if it blocks, fix and
     re-stage into a NEW commit (never `--amend`).
   - If the user course-corrects mid-fix ("no, don't change that"),
     revert the staged edits for this one item and move to the next.
     Don't abort the whole session — the remaining decisions are
     still valid.

2. **Dismissal / discuss flips next.** For each item in the
   `dismiss` / `discuss` buckets, apply the GH comment edit
   individually (see Step 7) — do **not** pack multiple flips into
   one PATCH. Per-flip PATCHes mean a mid-session abort still leaves
   the comment in a consistent state.

## Step 7 — Editing the GH comment

Applies to both **dismiss** (flip `[ ]` → `[x]`, append
`— _dismissed: <reason>_`) and **discuss** (keep `[ ]`, append or
replace `— _discuss: <note>_`).

1. **Re-fetch the current body** by comment id — it may have changed
   since step 2 if the bot re-ran. Capture the repo nwo once and
   reuse it; use `mktemp` so concurrent sessions don't clobber each
   other:

   ```bash
   REPO_NWO=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
   BODY_FILE=$(mktemp --suffix=.md)
   trap 'rm -f "$BODY_FILE"' EXIT
   gh api "/repos/$REPO_NWO/issues/comments/<id>" --jq .body > "$BODY_FILE"
   ```

2. **Re-parse the freshly-fetched body** to find the line you're
   about to flip. Do not reuse the line text captured at Step 3 — a
   benign cosmetic edit by the bot would invalidate it. Locate the
   item by its position in the parsed worklist (section + index, or
   by a stable substring like the cited `path:LINE`) and use the
   *current* line text as the match target.

3. Do an exact-text line replacement on the file — match the full
   current line verbatim and replace with the new line:
   - dismiss: `- [ ] <text>` → `- [x] <text> — _dismissed: <reason>_`
   - discuss (first time): `- [ ] <text>` → `- [ ] <text> — _discuss: <note>_`
   - discuss (replacing a prior discuss note):
     `- [ ] <text> — _discuss: <old>_` → `- [ ] <text> — _discuss: <new>_`

   If the line still can't be found (the finding itself was rewritten
   or removed on a concurrent run), abort this flip and tell the user
   — do not guess.

4. PATCH the comment, reusing `$REPO_NWO`:

   ```bash
   gh api -X PATCH "/repos/$REPO_NWO/issues/comments/<id>" \
     -F body=@"$BODY_FILE"
   ```

   (`-F body=@file` reads the file as the body field — safer than
   `-f body="$(cat ...)"` which can blow up on shell-special chars.)

5. Verify success by re-reading the comment body and confirming the
   new line is present with the correct marker and suffix.

## Step 8 — Session end

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
  revert the staged edits for that one item and move to the next
  planned fix. The other Step 4 decisions are still valid — don't
  re-triage.
- Do not commit a half-baked fix.
