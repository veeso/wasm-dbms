# AI Policy

This policy applies to contributions to wasm-dbms that use generative AI or
other code-generation systems.

## Human Responsibility

AI may be used as a development tool, but every contributor remains
responsible for the submitted code, documentation, tests, commit messages, and
licenses. A human contributor must understand the contribution well enough to
explain and maintain it.

Maintainers may ask for clarification, revisions, or removal of AI-generated
material. Contributions that cannot be responsibly reviewed may be rejected.

## Disclosure

Disclose AI assistance in the pull request using the template's disclosure
section. The disclosure is required when AI generated or substantially shaped
code, documentation, tests, configuration, commit messages, or other submitted
text. Ordinary autocomplete that completes a few words or symbols does not
require disclosure.

The disclosure should identify the tool and briefly describe how it was used.
Do not paste private prompts, credentials, proprietary source, or confidential
user data into a public pull request.

## Review and Provenance

- Review generated output as if it came from an untrusted contributor.
- Run the checks required by [`CONTRIBUTING.md`](./CONTRIBUTING.md), including
  the relevant build, test, formatting, documentation, and security checks.
- Confirm that dependencies, snippets, examples, and translations are
  compatible with their licenses and have appropriate attribution.
- If AI output includes quoted context from a source, preserve the quote as a
  block quote and explain why it is relevant; do not present generated text as
  an original quotation.
- Never disclose secrets or private project data to an AI system.

AI assistance does not change the project's [Code of Conduct](./CODE_OF_CONDUCT.md),
license, security-reporting process, or maintainer review requirements.
