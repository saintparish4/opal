---
title: "Opal v0.3.0"
description: "Opal now installs the versions npm would install. On six edge-case fixtures the trees match npm's package for package, and on a 365-package Next.js scaffold 432 of npm's 433 package versions match."
date: 2026-09-19T16:21:02Z
release: "https://github.com/saintparish4/opal/releases/tag/v0.3.0"
lede: "Opal now installs the versions npm would install. On six edge-case fixtures the trees match npm's package for package, and on a 365-package Next.js scaffold 432 of npm's 433 package versions match."
summary: "Opal now installs the versions npm would: on a Next.js scaffold, 432 of npm's 433 package versions match. Linking runs in parallel, about 2.7× faster than v0.2.1, and an install names every package whose install scripts it didn't run."
commands:
  - title: "To install Opal v0.3.0"
    id: post-install
    command: "curl -fsSL https://raw.githubusercontent.com/saintparish4/opal/master/install.sh | OPAL_VERSION=v0.3.0 bash"
---

## Matching npm {#matching-npm}

Version preference agrees with `npm-pick-manifest` itself on all 12,032 cases of a generated registry.

## Faster linking {#faster-linking}

Linking runs in parallel, one `node_modules` depth at a time, about 2.7× faster than v0.2.1 on the same tree and machine.

## Install scripts are named {#install-scripts-are-named}

The install names every package whose install scripts it didn't run, the project's own included.

## A contained `exports` {#a-contained-exports}

A package's `exports` can no longer resolve to a file outside the package.

## The suites behind it {#the-suites-behind-it}

- Randomized SIGKILLs during install that must converge.
- A cross-check of resolution against npm's.
- A fuzz target and a proptest for the resolver.
- A benchmark job that records numbers on every push.

**Full Changelog**: [https://github.com/saintparish4/opal/compare/v0.2.1...v0.3.0](https://github.com/saintparish4/opal/compare/v0.2.1...v0.3.0)
