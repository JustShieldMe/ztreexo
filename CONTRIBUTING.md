# Contributing to justshield.me

Thanks for taking the time to contribute. This project covers both the justshield.me website (this repo) and the research tools linked from it (Cyze, Crosslink Indexer, Ztreexo, Zcash-TFM, and others under the [JustShieldMe](https://github.com/JustShieldMe) org) — most of the guidance below applies to all of them.

## Code of Conduct

Be respectful, assume good faith, and keep disagreements focused on the technical substance. Cryptography and protocol work attracts strong opinions — back yours with the spec, a reference, or a reproducible result rather than volume.

## Reporting Bugs

1. Search existing issues first to avoid duplicates.
2. Open a new issue with:
   - What you expected to happen vs. what actually happened
   - Steps to reproduce (commands, inputs, environment/OS)
   - Relevant logs or error output
3. For a bug in a specific tool (e.g. `crosslink-indexer`), file it in that tool's repo, not this one.

## Suggesting Enhancements

Open an issue describing the problem you're trying to solve before proposing a specific solution — it's easier to evaluate "here's a gap" than "please implement X" without the context. Link to relevant ZIPs, specs, or prior art where it exists.

## Development Setup (this website)

```bash
npm install
npm run dev      # local dev server at localhost:3000
npm run lint      # must pass before opening a PR
npm run build     # static export to out/ — must succeed before opening a PR
```

## Adding Content

This site is content-driven via MDX frontmatter — most contributions here don't touch React code at all:

- **Blog posts** — copy `content/posts/TEMPLATE.mdx`, fill in the frontmatter, drop your MDX slug in `content/posts/`.
- **Open-source project pages** — copy `content/projects/TEMPLATE.mdx`.
- **Protocol primers** (`/docs/ZK-xx`) — copy `content/primers/TEMPLATE.mdx`.

Keep prose factual and cite sources (protocol spec, ZIPs, papers) via the `referenceLinks`/`docLinks` frontmatter fields rather than asserting claims inline without support.

## Pull Request Process

1. Fork the repo and create a branch off `main` (`feat/short-description` or `fix/short-description`).
2. Keep PRs focused — one logical change per PR is easier to review than a bundle of unrelated fixes.
3. Match the existing code style: TypeScript, Tailwind utility classes, no new abstractions unless they remove real duplication (see the codebase's existing `DetailPage`/`EntryRow` components for the pattern).
4. Run `npm run lint` and `npm run build` locally and confirm both pass.
5. Write a PR description that explains *why* the change is needed, not just what changed — link the issue it addresses if one exists.
6. Expect at least one review and be responsive to feedback; small, well-scoped PRs get merged faster than large ones.

## Commit Messages

Write present-tense, descriptive commit messages ("Add ZK-05 primer on Orchard nullifiers", not "updates"). Squash fixup commits before requesting review where practical.

## Reviewing Cryptographic / Protocol Code

Any change touching consensus-critical logic, key derivation, or proof circuits (in the linked tool repos) gets held to a higher bar:

- Cite the spec section or ZIP the change implements.
- Include or update tests that exercise the change.
- Call out any deviation from the reference implementation explicitly in the PR description — silent deviations are the hardest bugs to catch in review.

## Reporting a Security Vulnerability

**Do not open a public issue for a security vulnerability.** Email the details privately to **security@justshield.me** (or the security contact listed in the affected repo's README, if different) with enough detail to reproduce the issue. Give the maintainers a reasonable window to address it before any public disclosure.

## License

Unless a repo states otherwise, contributions are made under that repo's existing license. Check the `LICENSE` file (or the license badge on the repo) before contributing if this matters to you.
