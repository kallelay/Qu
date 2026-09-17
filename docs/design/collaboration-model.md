# How ten sessions share one repository

**Status: RATIFIED by Ahmed, 2026-09-16. This is policy.**

Provenance, because it matters and because getting it wrong caused an
incident: Ahmed asked for a fix to the collaboration problem ("different
clones … or something better"); **a Claude session wrote this document and
the hook**; three sessions then read the commit's author field, saw
`Ahmed`, and treated it as a ruling he had issued — one of them reported to
him that it had breached his policy, when he had not made one. He was then
asked directly and **ratified it as written**.

That failure is itself the point. **Every commit in this repo carried the
same git identity, so the author field distinguished nothing** — the
`Co-Authored-By` trailer was the only signal that a session wrote it.
Ahmed has since ruled that **lanes set `user.name` per clone**
(`git config user.name "qu-<lane>"`), so `git log` answers the question
instead of inviting a guess.

> **CAVEAT, found by a lane following that rule and reverting within ninety
> seconds: it does not work in a WORKTREE, and following it there overwrites
> every other lane's identity.**
>
> Worktrees share one `.git/config`. `git config user.name` inside
> `D:\qu-<lane>` sets it for the parked tree and for every other lane too —
> silently, last-writer-wins. Verified: all `D:\qu-*` trees report
> `git rev-parse --git-common-dir` = `D:/QuWorkspace/.git`, while the
> Dropbox checkout is a separate clone with its own `.git`. **The rule was
> written from the one environment where it works and applied to the
> environment where it does not** — the same one-door-over error the rest of
> this document is about.
>
> The mechanism git provides, which needs enabling once, **by the integrator
> only**:
>
> ```sh
> git config extensions.worktreeConfig true          # ONCE, repo-wide
> git config --worktree user.name "qu-<lane>"        # then per worktree
> ```
>
> **Until the integrator has enabled the first line, worktree lanes cannot
> set a per-lane identity and should not try.** For them the
> `Co-Authored-By` trailer remains the only signal. Separate clones are
> unaffected and may set `user.name` normally.
>
> **Verify from outside your own tree** — `git -C <another worktree> config
> user.name` — because setting someone else's identity is silent and
> invisible from where you did it.

**A rule with authority in practice and nobody who signed it is the same
failure as a measurement everyone believes and nobody re-ran.** If you find
yourself obeying a document here, check who ratified it.

Every rule below was paid for. Nothing is here because it sounded tidy.

## The failures this replaces

One evening, 2026-09-16, all on real work by careful sessions:

- A **feature branch was published as `master`** on all four mirrors. The
  session checked the branch name earlier in the session; the shared
  worktree's `HEAD` moved in between; the push named `HEAD`.
- **Two sessions independently made the same reconciliation merge.** Only a
  stale `index.lock` stopped a duplicate merge commit.
- **Two sessions independently built the same feature** — a generated
  builtin-docs table — one of them reimplementing a shared helper and
  reintroducing a bug the original never had.
- **Five branches carrying real work existed on exactly one disk each.**
  `tools/backup.sh` pushes `master`; nobody had asked it to push branches.
- **One commit ended up published under two branch names.**
- A merge **silently carried a 15-row corruption** because the branch tip
  moved while the merge was in flight.
- Four separate commits **swept another session's uncommitted work**,
  because a shared checkout has one index and one working tree.

The common cause is not carelessness. It is that **a shared checkout has
one `HEAD`, one index, and one working tree, and none of them belong to
you.**

## The model

**One lane, one worktree, one branch. `master` belongs to the integrator.**

```
D:\QuWorkspace                     PARKED. Read-only. Never build here.
D:\qu-<lane>                       one per lane. Your branch. Yours alone.
<integration worktree>             master. Nobody works here.
```

- **Work only in a tree nobody else has checked out.** If two sessions can
  write to it, it is not yours.
- **`master` is checked out in exactly one place**, and that place is not a
  workspace. Reading master is `git show master:<path>`.
- **Only the integration lane writes `master`.** Everyone else publishes a
  branch and requests a merge.

## Publishing your work

Push your branch, under its own name, to every mirror — for backup, not for
integration:

```sh
for r in backup-d backup-h onedrive; do
  git push $r refs/heads/claude/<branch>:refs/heads/claude/<branch>
done
```

**Explicit refs. Never `HEAD:`.** `HEAD` is a reading with a timestamp; in a
shared tree the timestamp matters and the reading expires.

**`dropbox` is non-bare with `master` checked out — it cannot be pushed to.
It pulls from its own side.**

Verify per remote, from a checkout that has all four:

```sh
git ls-remote <remote> refs/heads/<branch>
```

"Pushed" is per-remote, not global. A local remote-tracking ref is not
evidence.

## Requesting a merge

1. **Freeze the tip.** Stop committing. A tip that moves during a merge
   produces a merge that is correct about a commit and wrong about a branch.
2. Post to `board2.txt`: branch, tip SHA, what it does, what it changes that
   already worked.
3. The integrator runs the dry run first:
   ```sh
   git merge-tree --write-tree <the head it will actually merge into> <branch>
   ```
   It resolves entirely in memory and touches no worktree. **Valid only
   against the exact head you will merge into** — master moves hourly here.
4. Merge **whole branches, `--no-ff`**. See below.
5. **Build and run the full test suite against the merged tree. `merge-tree`
   clean is not this step — it is a different question, and a clean answer
   to it has now produced a broken master twice in one night by two
   distinct mechanisms, neither a bad resolution:**

   - **Stacked dependency**, predictable from the branch graph — a child
     branch calls code that only exists on its own parent, so a dry run of
     the child against `master` alone is reassuring and wrong. The fix is
     to know the graph, not to trust the dry run.
   - **Two branches inserting adjacent, non-overlapping blocks into the same
     construct** — no shared ancestry, no overlapping lines, so `merge-tree`
     reports clean and the textual merge silently eats a closing brace (or
     similar) that belonged to one insertion. Not predictable from the
     branch graph; only building the *combination* finds it.
   - **A branch lands a new invariant that older, already-cut branches
     retroactively violate** — a test added by branch C fails against code
     from branches A and B, neither of which could have satisfied a
     requirement that did not exist when they were written. Both A and B
     were green in isolation. Only the merged tree, tested in full, shows
     the failure.

   `merge-tree` answers *"do these edits overlap textually"*. It cannot
   answer *"does the result compile"* or *"does the result still satisfy
   invariants that landed after these branches were cut"* — only a real
   build and a full test run answer either. **Fix the first failure found,
   then re-run the whole suite, not just confirm it builds** — a compile
   failure masks every test failure behind it, so "it builds now" is not
   "it is fixed".
6. Push `master:master` from the integration clone. Verify per remote.

## Why whole branches, and not line-by-line integration

Taking changes line by line, or cherry-picking hunks out of a branch,
produces a combination **nobody built and nobody tested**. The author's
suite ran against their branch; it did not run against your selection from
it. It also destroys authorship and breaks `git bisect`, which is the one
tool that finds a regression in a repository this active.

A branch is a unit that somebody stands behind. **Merge the unit or reject
it.** If a branch is too big to merge as a unit, the fix is a smaller
branch, not a finer merge.

The exception is a genuine conflict, where you resolve the conflicting
hunks — and even then you resolve them, you do not re-author the change.

## Claiming work

**Before claiming a lane, search the refs. Not your memory of who owns
what, and not what a peer told you — refs.**

    git log --all --since="2 days ago" --format='%h %ad %an %s' --date=short
    git branch -a --contains <a commit in the area you are about to touch>

**"Nobody mentioned it" is not evidence. A branch that exists is.** People
forget and misattribute; refs do not. This has been a real collision three
times in two days: `claude/unit-dimensions` (caught with ~90 seconds of
margin), a duplicate `builtin_docs` implementation (not caught — two
sessions built the same feature independently), and an 8-commit,
4000-line ECM circuit-fitting branch that two sessions in a row concluded
was "nobody's" because a third session had correctly said it wasn't
*theirs* — "not theirs" was misread as "not anybody's". Check the refs
before you claim, every time, even when a peer has just told you something
sounds free.

**Say "doing X now", not "X is the right thing to do."**

The first is a claim. The second is an opinion, and two sessions can hold it
at the same time — which is exactly how the duplicate merge and the
duplicate feature happened. **Agreement feels like confirmation rather than
a race.**

Before starting anything shared, check `board2.txt` and post the claim. It
costs one line and it is the only thing that has actually prevented a
collision here.

## Never pause with uncommitted work in a shared tree

Anything uncommitted in a tree others touch will be committed by someone
else, usually within the hour — four times in one day, across three files,
with at least two different sessions as sweeper. Commit it, move it to your
own worktree, or do not write it there.

Gitignored files are worse, not better: git will not protect them, will not
restore them, and `backup.sh` will not push them.

## Enable the guard

```sh
git config core.hooksPath tools/githooks     # every clone, once
git config qu.integrator true                # integration clone only
```

`tools/githooks/pre-push` refuses: pushing `master` from a non-integration
clone, pushing `HEAD`, and pushing a branch under a different name. Each
rule exists because someone did that thing, for a good reason, and lost
work.

A guard that narrates is not a guard. This one refuses.
