# Security policy

Opal downloads packages from a public registry and unpacks them onto your machine, so a bug in how it fetches, verifies, or writes them can be a security bug. Reports are welcome.

## Supported versions

Opal is pre-1.0. A security fix ships in the next release, and older releases aren't patched. Run `opal upgrade` to get the latest.

## Report a vulnerability

Please don't open a public issue or pull request for a security problem. Report it privately, either way:

- **GitHub:** [report a vulnerability](https://github.com/saintparish4/opal/security/advisories/new). Only you and the maintainer can see the report.
- **Email:** [blueskylabx@gmail.com](mailto:blueskylabx@gmail.com)

A useful report says:

- which version (`opal --version`), and your OS and CPU
- what an attacker can do, and what they have to control to do it: a package, a registry response, a project's files
- how to reproduce it. A `package.json`, a tarball, or a script is ideal.

## What happens next

Opal has one maintainer. You'll get a reply within 3 days. A confirmed vulnerability goes ahead of all other work: the fix is released as soon as it's ready, and the details are then published as a [GitHub security advisory](https://github.com/saintparish4/opal/security/advisories), crediting you unless you'd rather not be named. Please keep the details private until then.

## What counts

Anything that lets a package, a registry, or someone on the network do more than a correct install allows. For example:

- a tarball that writes outside its own package directory
- an install that accepts bytes that don't match the registry's `dist.integrity`
- a package that changes files in the shared cache, and so in every other project that uses them
- a registry response or a `package.json` that makes `opal.lock` record something other than what was resolved
- `install.sh` or `opal upgrade` installing a binary that doesn't match its release's `SHA256SUMS`
- a way to tamper with a release, or with this repository's CI

A malicious package is the registry's to remove, not Opal's: [report it to npm](https://docs.npmjs.com/reporting-malware-in-an-npm-package).
