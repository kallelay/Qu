# Security Policy

## Reporting a vulnerability

If you find a security issue in Qu (the engine, Qu Studio, or the
tooling in this repo), please **do not open a public issue**. Use
[GitHub's private vulnerability reporting](../../security/advisories/new)
for this repository instead — it goes directly to the maintainer without
being visible to anyone else until it's resolved.

Include, if you can:

- What's affected (engine, `qu-cli`, Qu Studio, a specific builtin)
- A minimal reproduction
- What you'd expect to happen instead

## Scope

Qu is a scripting language and IDE for measurement/signal-processing
work, not a network service — most realistic security concerns are
things like: a malicious `.qu` script escaping intended sandboxing,
a crafted input file (WAV, CSV, MAT, image) triggering memory-unsafe
behaviour in a parser, or a dependency with a known CVE. All of these
are taken seriously; report them the same way.

## Supported versions

Only the latest released version is supported with security fixes.
This is a young (0.x) project without a long-term-support branch yet.
