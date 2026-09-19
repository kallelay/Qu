# DSP course — writer's brief (v2)

Internal. Not published in the book's navigation. Read before drafting any
of Lessons 1–24, then follow it literally. The course must read as one
person wrote it.

## The throughline (revised — generic first, Qu second)

Every lesson has exactly two parts, in this order. Do not interleave them.

**Part 1 — The idea.** Generic, textbook DSP: the concept, a real generated
diagram illustrating it, and the equation, exactly the way a standard
reference (Oppenheim, or the classic "topics of signal processing"
infographic style Ahmed shared) presents it — concept card, then math. This
part contains **zero Qu code**. It should stand alone as correct DSP
teaching even to a reader who has never heard of Qu. A one-sentence
concrete hook is fine to open with, but do not stretch it into a story —
get to the generic explanation fast.

**Part 2 — In Qu.** A clearly headed `## In Qu` section. This is where Qu
appears for the first time: the worked example, the real code, the real
output, the real generated figure. Everything measurable gets measured
here. This is the payoff — "here is that same idea, running" — not the
lesson's spine.

**Every lesson ends** on a real limitation Part 2 exposed, naming the next
lesson. Never end on a summary of what was covered.

## Non-negotiables

**Visuals are not optional, and one is a minimum, not a target.** Every
lesson needs at least 2-3 real generated figures, produced by actual Qu
plotting calls (`plot`, `spectrogram`, `imshow`, `nyquist`,
`bode_magnitude`, `subplot`, etc.) inside ```qu code blocks — the build
pipeline auto-captures whatever a block draws, no `savefig` needed. Default
to showing, not just telling: a "before vs. after," a waveform next to its
spectrum, a filter's response curve, two panels compared side by side.
Typical split: one or two generic illustrative plots in Part 1 (a tiny
throwaway Qu snippet is fine if its only job is drawing the textbook
picture — clean, labeled axes, no narrative framing), and at least one
result plot in Part 2 showing the actual measured outcome. A wall of text
and printed numbers with no picture is a failed lesson under this brief,
even if the prose is otherwise good.

**Equations are real LaTeX, rendered by MathJax** (now wired into the
site). Use `\(...\)` for inline math and `\[...\]` for display equations —
e.g. `\(X(f) = \int_{-\infty}^{\infty} x(t) e^{-j2\pi ft}\,dt\)`. Do not use
plain-text/unicode equations anymore, and do not use `$...$` (not
configured). Inside Qu plot-label strings, Qu's own typesetter still
applies — that's separate from prose equations.

**Real output only.** Run every snippet and paste its actual printed
output in a fenced block. If a number is uglier than the theory predicts,
keep it and explain the gap. Never invent output.

**History is reported, not dramatised.** Fourier, Nyquist, Shannon,
Cooley and Tukey, Kalman and Wiener all appear where relevant. Correct
name, year, result, one or two sentences. No invented quotes, no scenes.

## Voice

Short declarative sentences. Second person in Part 2, more neutral/textbook
register in Part 1. Confident, precise, never breathless: no exclamation
marks, no "amazing", no "simply", no "just". Assume an intelligent reader
new to this material — explain everything, condescend to nothing.

Cut ruthlessly for length — the audience skims. Prefer short paragraphs
(2-4 sentences), bullet/definition lists over prose where the infographic
style would use a card, and one worked example carried through Part 2
rather than four disconnected snippets. Target 900–1,400 words total
(shorter than v1 — generic content is denser per word than narrative).

Wit is dry, rare, load-bearing (at most once per lesson) and never at the
expense of Part 1's clarity.

## Mechanics

H2 for `## The idea` and `## In Qu` (plus any needed subheads within
each), H3 sparingly. Use Qu's existing stdlib functions rather than
hand-rolling — if a lesson needs something Qu lacks, say so plainly. Seed
every random call.
