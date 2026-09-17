# Start Here

This page assumes you have never programmed. If you have, skip to
[From Zero](from-zero.md), which moves faster.

You will need about twenty minutes. By the end you will have measured
something and drawn a graph of it.

## Why do this at all

You probably already do this work in a spreadsheet, and spreadsheets are
good. They stop being good at a particular point: when you have forty
files instead of one, when you need the same twelve steps done to each,
or when someone asks six months later *exactly* what you did and the
answer is buried in a cell you cannot find.

A program is a written-down recipe. It says what to do, in order, in a
file you can read, keep, send to someone, and run again next year on new
data and get the same answer. That is the whole idea.

## Opening the door

Install Qu (see the downloads page), then open a terminal and type:

```
qu repl
```

`repl` is short for read-eval-print loop, which is a long name for a
prompt that waits for you. Type something and press Enter:

```qu
2 + 2
```

It says `4`. That is not a trick — you can use it as a calculator all day.
Try `17 * 3`, or `2 ^ 10`, or `sqrt(2)`.

To leave, type `exit` or press Ctrl+D.

## Names for things

Typing the same number repeatedly is tedious, so you give things names:

```qu
sample_rate = 1000
duration = 2
total_samples = sample_rate * duration
print("I will record {total_samples} samples")
```

Three things are happening.

`sample_rate = 1000` puts the number 1000 in a box labelled `sample_rate`.
The `=` does not mean "is equal to" the way it does in mathematics. It
means "put this in that box". You can put something else in later and the
box keeps the newest thing.

`print(...)` shows you something. The quotation marks make text.

`{total_samples}` inside the text means "and here, put whatever is in that
box". Without the braces you would literally see the word.

Names may contain letters, digits and underscores, and must start with a
letter. `sample_rate` and `SampleRate` are different boxes. Choose names
that say what the thing is; you are writing for the person who reads this
in a year, and that person is you.

## Many numbers at once

Measurement is never one number. It is thousands, in order. Qu calls that
a **vector**, and it is the thing the whole language is built around.

```qu
readings = [2.1, 2.3, 2.2, 2.6, 2.4]
print("{len(readings)} readings, average {mean(readings):.2f}")
```

`[...]` with commas makes a vector. `len` is how many. `mean` is the
average. The `:.2f` says "two digits after the point" — without it you get
the full unrounded number, which is usually more than you wanted to read.

The important part is what happens when you do arithmetic on it:

```qu
doubled = readings * 2
print(doubled)
```

Every element doubled, in one step. You did not have to say "for each
reading, multiply it". This is the thing that makes the language worth
learning: **an operation on a whole set of numbers looks like an operation
on one number.**

Making a long vector by hand would be silly, so:

```qu
t = (0 to 999) / 1000
print("{len(t)} values, from {t[0]} to {t[999]}")
```

`0 to 999` means every whole number from 0 to 999, both ends included —
one thousand of them. Dividing by 1000 divides every one. So `t` is now a
thousand time-points, one millisecond apart.

`t[0]` is the first one. **Counting starts at zero**, so the last of a
thousand is `t[999]`. This trips up everyone at first, including people
who have programmed for years.

## Making a signal and looking at it

```qu
signal = sin(2 * pi * 5 * t)
print("goes from {min(signal):.2f} to {max(signal):.2f}")
```

That is a five-hertz sine wave: `sin` of two-pi-times-frequency-times-time,
the formula from the textbook, written the way the textbook writes it. It
swings between −1 and 1, which is what the print confirms.

Real measurements are not clean, so add some mess:

```qu
noisy = signal + 0.3 * randn(1000, seed = 1)
print("clean {rms(signal):.3f}, noisy {rms(noisy):.3f}")
```

`randn` makes random numbers. `seed = 1` means *the same* random numbers
every time you run it — which sounds like it defeats the point, and is
exactly right: you want your results to be reproducible. `rms` is the
root-mean-square, a standard measure of how big a wobbling signal is.

Now draw it:

```qu
plot(t, noisy, color = "#b0b0b0")
plot(t, signal, color = "#0072BD")
xlabel("time [s]")
ylabel("amplitude")
savefig("my-first-figure.pdf")
```

Two `plot` calls put two lines on the same axes. `savefig` writes the file.
Open it — that is a real, publication-quality PDF.

## Doing something to every item

Sometimes you genuinely need to handle things one at a time:

```qu
levels = [0.5, 1.2, 0.8, 2.1, 1.7]
count = 0
for k in 0 to len(levels) - 1
    if levels[k] > 1.0
        count = count + 1
    end if
end for
print("{count} readings above 1.0")
```

`for k in 0 to len(levels) - 1` repeats the middle part once for each
position, with `k` being 0, then 1, then 2, and so on. `if` runs its part
only when the test is true. `end if` and `end for` mark where each one
stops — Qu wants you to say so, rather than guessing from indentation.

Note `len(levels) - 1` again: five items, positions 0 to 4.

Most of the time you do not need the loop at all:

```qu
above = levels[levels > 1.0]
print("{len(above)} readings above 1.0, they are {above}")
```

`levels > 1.0` asks the question of every element at once, and the square
brackets keep the ones where the answer was yes.

## Naming a recipe

When you do the same thing more than once, give it a name:

```qu
function celsius_to_kelvin(c)
    return c + 273.15
end function

print("{celsius_to_kelvin(25):.2f} K")
print("{celsius_to_kelvin(-40):.2f} K")
```

`function` starts it, `c` is the placeholder for whatever you hand it,
`return` says what comes back, `end function` closes it. Now the
conversion exists in one place, and if it is wrong it is wrong in one
place.

A function's own names are private to it. If you use `c` inside, you are
not disturbing any `c` outside.

## When something goes wrong

It will. Qu tries to tell you what and where:

```
qu: runtime error: `redings` is not defined
```

A typo. Qu will not guess what you meant, on purpose: guessing is how you
get an answer that looks fine and is wrong.

```
qu: runtime error: plot: unknown keyword argument `colour=` -- did you mean `color=`?
```

British spelling. It tells you the nearest real name.

The rule the whole language follows: **it would rather stop and tell you
than continue and be quietly wrong.** When you see an error, you have been
saved some trouble, not given some.

## Putting it in a file

The prompt is for trying things. Real work goes in a file. Put this in
`first.qu`:

```qu
fs = 1000
t  = (0 to 999) / fs
signal = sin(2 * pi * 5 * t)
noisy  = signal + 0.3 * randn(1000, seed = 1)

print("rms: clean {rms(signal):.4f}, noisy {rms(noisy):.4f}")

plot(t, noisy, color = "#b0b0b0")
plot(t, signal, color = "#0072BD")
xlabel("time [s]")
ylabel("amplitude")
savefig("first.pdf")
```

and run it:

```
qu run first.qu
```

Lines starting with `#` are notes to yourself; Qu ignores them. Use them.

## What you now know

You can store numbers, work on thousands at once, repeat things, make
decisions, name a recipe, draw a graph, and read an error. That is most of
what programming is. Everything else is more of the same, plus a larger
vocabulary.

Where to go next:

- **[From Zero](from-zero.md)** — the same ground faster, then filtering,
  curve fitting, and tables.
- **[Book 1 — Fundamentals](book1-fundamentals.md)** — the language
  properly, if you have some programming behind you.
- `catalog/` in the repository — about a hundred complete programs. Read
  one near your own work and change it.
