# Mandatory Security and Privacy Policy

These rules apply to every task performed in this repository.

## Mandatory Security Audit

Before completing any task, review all changes for sensitive information. This includes source code, configuration files, README files, changelogs, commit messages, documentation, comments, logs, tests and fixtures, generated files, Git diffs and staged changes, release notes, and build artifacts.

## Never Expose Sensitive Information

Never introduce or publish real:

- Passwords, API keys, tokens, credentials, private keys, or authentication material
- Personal information, private files, or local configuration
- Sensitive IP addresses or internal infrastructure details
- Steam IDs and private account identifiers
- User IDs, device IDs, or private resource identifiers
- Database credentials or connection strings containing secrets
- Private URLs, logs, screenshots, or debug output

Do not copy sensitive information from existing files into new files. Use fictional placeholders in documentation and examples.

## Required Workflow

Before declaring any task complete:

1. Inspect the complete working tree.
2. Review all staged and unstaged changes.
3. Scan for secrets using available security tools.
4. Check personal information, identifiers, IP addresses, and private files.
5. Review README, CHANGELOG, documentation, and commit messages.
6. Fix findings introduced by the current changes.
7. Run relevant checks again.
8. Report the results honestly.

Never claim a security check passed unless it was actually performed.

## Blocking Rule

If an unresolved high-risk exposure exists, do not create the affected commit or publish the affected release. Do not print discovered secrets in terminal output or reports. Report findings using their category and file location without reproducing the sensitive value.

If a check cannot be performed, explicitly report that limitation.

## Mandatory Skill

Use the `security-privacy-audit` skill for repository changes. The security audit is part of completing every task, not an optional step.

Never trade security or privacy for convenience.
